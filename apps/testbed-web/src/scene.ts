// Scene: renderer, lights, sky colour from TimeOfDay, ground + 10 m grid, the city greybox
// (data/city/<id>.greybox.glb, prompt 05), police helicopters, barriers, fading contact
// markers and intent arrows. Traffic and police cars are drawn by render/lod.ts.
// Sim space is +Z up / +Y forward; Three.js is +Y up, so `world` is rotated -90° about X and
// every sim-space object is parented under it.
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import type { Barrier, SimEvent } from "./wasm/sim";
import { D, DRAW_STRIDE, INTENT_STRIDE, POLICE_DRAW_FLAG } from "./wasm/layout";
import { layers } from "./settings";
import { SignalView } from "./signals_view";
import { CityTiles } from "./render/city_tiles";
import { Quality } from "./render/quality";

const DAY_SKY = new THREE.Color(0x8fb8de);
const NIGHT_SKY = new THREE.Color(0x070a14);

interface ContactMarker {
  obj: THREE.Group;
  dot: THREE.Mesh<THREE.SphereGeometry, THREE.MeshBasicMaterial>;
  arrow: THREE.ArrowHelper;
  age: number;
  active: boolean;
}

const CONTACT_LIFETIME_S = 1.5;
/** Contact markers are pooled (a crash emits dozens of contacts per frame): oldest reused. */
const MAX_CONTACT_MARKERS = 64;
const CONTACT_DOT = new THREE.SphereGeometry(0.08, 8, 6);
const tmpNormal = new THREE.Vector3();

export class SceneView {
  readonly renderer: THREE.WebGLRenderer;
  /** Graphics quality: MSAA, resolution scale (adaptive in auto mode), draw distance. */
  readonly quality = new Quality();
  readonly scene = new THREE.Scene();
  readonly world = new THREE.Group();
  readonly hemi: THREE.HemisphereLight;
  readonly sun: THREE.DirectionalLight;
  private readonly barriers = new THREE.Group();
  private readonly contacts: ContactMarker[] = [];
  private nextContact = 0;
  /** Contact markers created since the page loaded (for automation). */
  contactsSeen = 0;
  private readonly intentGroup = new THREE.Group();
  private readonly cities = new Map<string, THREE.Object3D>();
  /** Tiled meshes of each loaded city (render/city_tiles.ts). */
  private readonly cityTiles = new Map<string, CityTiles>();
  private cityShown: string | null = null;
  /** City tiles past this distance (m) are not drawn; fog can only shorten it. From the
   * graphics quality (high: the camera's far plane, no visible change); `?draw=<m>` overrides. */
  drawDistanceM = Infinity;
  private readonly camPos = new THREE.Vector3();
  private readonly policeUnits: { obj: THREE.Group; red: THREE.MeshStandardMaterial; blue: THREE.MeshStandardMaterial }[] = [];
  /** Light-bar glow multiplier for the police helicopters (art style). */
  policeGlow = 1;
  /** Replaces renderer.render in render() (art style post stack); null = plain render. The
   * city-tile update runs before it either way. */
  renderOverride: ((camera: THREE.Camera) => void) | null = null;

  private debugGrid!: THREE.GridHelper;
  /** The one siren point light (browser perf pass): every point light costs every lit pixel,
   * even at intensity 0, and a changing light count recompiles the lit shaders. It follows
   * the nearest police car with its siren on and is hidden when no siren is on. */
  private readonly sirenLight = new THREE.PointLight(0xff2244, 0, 18, 2);
  /** Traffic signal heads and stop lines (signals_view.ts). */
  readonly signals = new SignalView();
  /** The flat world's ground material (the weather view makes it wet-capable). */
  readonly groundMaterial = new THREE.MeshStandardMaterial({ color: 0x3a3f44 });
  /** Called once with each city's glTF scene when it has loaded (wet road materials). */
  onCityLoaded: ((root: THREE.Object3D, cityId: string) => void) | null = null;

  /**
   * `postAA: true` when the art style's post stack does the antialiasing (its own MSAA target,
   * only if the graphics quality allows MSAA): the canvas is then created without MSAA, which
   * saves resolving a second multisampled buffer every frame.
   */
  constructor(parent: HTMLElement, opts: { postAA?: boolean } = {}) {
    // MSAA, resolution and draw distance come from the graphics quality (render/quality.ts).
    this.renderer = new THREE.WebGLRenderer({ antialias: this.quality.antialias && !opts.postAA, preserveDrawingBuffer: true });
    this.quality.attach(this.renderer);
    const draw = Number(new URLSearchParams(location.search).get("draw"));
    this.drawDistanceM = draw > 0 ? draw : this.quality.drawDistanceM;
    this.renderer.setSize(window.innerWidth, window.innerHeight);
    parent.appendChild(this.renderer.domElement);
    window.addEventListener("resize", () => this.renderer.setSize(window.innerWidth, window.innerHeight));

    this.scene.background = DAY_SKY.clone();
    this.hemi = new THREE.HemisphereLight(0xffffff, 0x333333, 2.0);
    this.sun = new THREE.DirectionalLight(0xffffff, 1.5);
    this.sun.position.set(30, 60, 20);
    this.scene.add(this.hemi, this.sun);

    this.world.rotation.x = -Math.PI / 2;
    this.scene.add(this.world);
    const ground = new THREE.Mesh(new THREE.PlaneGeometry(4000, 4000), this.groundMaterial);
    // Below the city's zone ground (-0.05 m) and roads (0 m), so they never z-fight with it.
    ground.position.z = -0.2;
    this.world.add(ground);
    // 10 m grid (GridHelper lives in Three's XZ plane, i.e. the ground).
    const grid = new THREE.GridHelper(2000, 200, 0xb0b0b0, 0x5c6268);
    grid.position.y = 0.01;
    this.scene.add(grid);
    this.debugGrid = grid;
    // Axis hint at the origin: +X red, +Y (forward) green.
    const axes = new THREE.AxesHelper(3);
    axes.position.z = 0.02;
    this.sirenLight.visible = false;
    this.world.add(axes, this.barriers, this.intentGroup, this.signals.group, this.sirenLight);
  }

  /**
   * Show the greybox mesh for `cityId` (`data/city/<id>.greybox.glb`, prompt 05), loading it on
   * first use; `null` hides every city. The greybox is only shown when the session loaded that
   * city, so flat-world sessions are not drawn inside buildings.
   */
  async setCity(cityId: string | null): Promise<void> {
    for (const [id, obj] of this.cities) obj.visible = id === cityId;
    this.cityShown = cityId;
    // The debug grid sits on the road surface and flickers through it; hide it in the city.
    this.debugGrid.visible = !cityId;
    if (!cityId || this.cities.has(cityId)) return;
    const holder = new THREE.Group();
    this.cities.set(cityId, holder);
    this.scene.add(holder);
    try {
      // The published (artifact) build cannot serve binary files, so it ships the same bytes
      // base64-encoded as `<id>.greybox.b64.txt`.
      let gltf;
      try {
        gltf = await new GLTFLoader().loadAsync(`data/city/${cityId}.greybox.glb`);
      } catch {
        const b64 = await (await fetch(`data/city/${cityId}.greybox.b64.txt`)).text();
        const bytes = Uint8Array.from(atob(b64.trim()), (c) => c.charCodeAt(0));
        gltf = await new GLTFLoader().parseAsync(bytes.buffer, "");
      }
      // glTF is +Y up, same as Three: add to the scene, not the Z-up world group.
      // Layers that share a plane in the greybox (bus lanes and parking painted on asphalt,
      // sidewalks on zone ground) draw over the layer below instead of flickering.
      // City v2 adds terrain (ground_* / rock / seabed, smooth-shaded) and a sea plane 0.25 m
      // above sea level (no polygon offset: at long range it would win against the land);
      // walls, kerbs and decks come without normals and are flat-shaded.
      const layer: Record<string, number> = { bus_lane: 2, parking: 2, sidewalk: 1, asphalt: 1, highway_concrete: 1 };
      gltf.scene.traverse((o) => {
        const m = (o as THREE.Mesh).material as THREE.Material | THREE.Material[] | undefined;
        for (const mat of m ? (Array.isArray(m) ? m : [m]) : []) {
          const l = layer[mat.name] ?? 0;
          if (l > 0) {
            mat.polygonOffset = true;
            mat.polygonOffsetFactor = -l;
            mat.polygonOffsetUnits = -4 * l;
          }
        }
      });
      holder.add(gltf.scene);
      this.cityTiles.set(cityId, new CityTiles(gltf.scene));
      this.onCityLoaded?.(gltf.scene, cityId);
    } catch (e) {
      console.warn("city greybox not available", cityId, e);
    }
  }

  /**
   * Draw police helicopters from police_draw() records (layout.ts D; flags 1 helicopter,
   * 2 pursuing): a box at altitude with a flashing light bar while pursuing. Police cars, bound
   * or not, are drawn by render/lod.ts.
   */
  updateHelicopters(units: Float32Array, timeS: number): void {
    let n = 0;
    for (let o = 0; o < units.length; o += DRAW_STRIDE) {
      const flags = units[o + D.flags];
      if (!(flags & POLICE_DRAW_FLAG.helicopter)) continue;
      if (n >= this.policeUnits.length) {
        const g = new THREE.Group();
        const body = new THREE.Mesh(new THREE.BoxGeometry(1.9, 4.8, 1.4), new THREE.MeshStandardMaterial({ color: 0xe9ebf0 }));
        body.position.z = 0.7;
        const red = new THREE.MeshStandardMaterial({ color: 0x661018 });
        const blue = new THREE.MeshStandardMaterial({ color: 0x102266 });
        const barR = new THREE.Mesh(new THREE.BoxGeometry(0.6, 0.25, 0.12), red);
        barR.position.set(-0.32, -0.1, 1.48);
        const barB = new THREE.Mesh(new THREE.BoxGeometry(0.6, 0.25, 0.12), blue);
        barB.position.set(0.32, -0.1, 1.48);
        g.add(body, barR, barB);
        g.scale.setScalar(1.4);
        this.world.add(g);
        this.policeUnits.push({ obj: g, red, blue });
      }
      const u = this.policeUnits[n];
      u.obj.position.set(units[o + D.pos], units[o + D.pos + 1], units[o + D.pos + 2]);
      u.obj.quaternion.set(units[o + D.rot], units[o + D.rot + 1], units[o + D.rot + 2], units[o + D.rot + 3]);
      const chasing = (flags & POLICE_DRAW_FLAG.chasing) !== 0;
      const phase = (timeS * 2.5 + n * 0.17) % 1;
      u.red.emissive.setHex(chasing && phase < 0.5 ? 0xff2040 : 0x000000);
      u.blue.emissive.setHex(chasing && phase >= 0.5 ? 0x2060ff : 0x000000);
      u.red.emissiveIntensity = u.blue.emissiveIntensity = this.policeGlow;
      n++;
    }
    this.policeUnits.forEach((u, i) => (u.obj.visible = i < n));
  }

  /** Siren light at `pos` (sim space) with this colour / intensity; `null` = no siren on. */
  setSiren(pos: THREE.Vector3 | null, colour = 0, intensity = 0): void {
    this.sirenLight.visible = pos !== null;
    if (!pos) return;
    this.sirenLight.position.copy(pos);
    this.sirenLight.color.setHex(colour);
    this.sirenLight.intensity = intensity;
  }

  /** Sky + light colour from the night factor (0 noon … 1 midnight); `sunDimming` (weather
   * clouds, 0..1, default 1) dims the sun and part of the sky light. */
  setTimeOfDay(night: number, sunDimming = 1): void {
    (this.scene.background as THREE.Color).copy(DAY_SKY).lerp(NIGHT_SKY, night);
    this.hemi.intensity = 2.0 * (1 - 0.8 * night) * (0.55 + 0.45 * sunDimming);
    this.sun.intensity = 1.5 * (1 - night) * sunDimming;
  }

  setBarriers(list: Barrier[]): void {
    this.barriers.clear();
    for (const b of list) {
      const m = new THREE.Mesh(
        new THREE.BoxGeometry(...b.size),
        new THREE.MeshStandardMaterial({ color: 0xe8e8e8, emissive: 0x220000 }),
      );
      m.position.set(...b.pos);
      m.rotation.z = b.yaw ?? 0;
      const stripe = new THREE.Mesh(
        new THREE.BoxGeometry(b.size[0] * 1.001, b.size[1] * 1.001, b.size[2] * 0.25),
        new THREE.MeshStandardMaterial({ color: 0xd02020 }),
      );
      m.add(stripe);
      this.barriers.add(m);
    }
  }

  /** Feed drained events: Contact events become fading point + impulse arrows. */
  addEvents(events: SimEvent[]): void {
    for (const ev of events) {
      const c = ev.Contact as
        | { point: [number, number, number]; normal: [number, number, number]; impulse_ns: number }
        | undefined;
      if (!c) continue;
      const len = Math.min(Math.max(c.impulse_ns / 2000, 0.2), 5);
      tmpNormal.set(...c.normal).normalize();
      let m = this.contacts[this.nextContact];
      if (!m) {
        const g = new THREE.Group();
        const dot = new THREE.Mesh(CONTACT_DOT, new THREE.MeshBasicMaterial({ color: 0xff00ff, transparent: true, depthTest: false }));
        const arrow = new THREE.ArrowHelper(tmpNormal, new THREE.Vector3(), len, 0xff00ff);
        for (const mat of [arrow.line.material, arrow.cone.material] as THREE.Material[]) mat.transparent = true;
        g.add(dot, arrow);
        g.renderOrder = 6;
        this.world.add(g);
        m = { obj: g, dot, arrow, age: 0, active: true };
        this.contacts[this.nextContact] = m;
      }
      this.nextContact = (this.nextContact + 1) % MAX_CONTACT_MARKERS;
      m.obj.position.set(...c.point);
      m.arrow.setDirection(tmpNormal);
      m.arrow.setLength(len);
      m.age = 0;
      m.active = true;
      this.contactsSeen++;
    }
  }

  update(dt: number, intents: Float32Array): void {
    const show = layers.on("contacts");
    for (const c of this.contacts) {
      if (!c.active) continue;
      c.age += dt;
      const fade = 1 - c.age / CONTACT_LIFETIME_S;
      if (fade <= 0) c.active = false;
      c.obj.visible = show && c.active;
      const o = Math.max(fade, 0);
      c.dot.material.opacity = o;
      (c.arrow.line.material as THREE.Material).opacity = o;
      (c.arrow.cone.material as THREE.Material).opacity = o;
    }
    // Intent arrows: [entity, kind, from xyz, to xyz] (placeholder hook, empty for now).
    this.intentGroup.clear();
    this.intentGroup.visible = layers.on("intents");
    if (this.intentGroup.visible) {
      for (let i = 0; i + INTENT_STRIDE <= intents.length; i += INTENT_STRIDE) {
        const from = new THREE.Vector3(intents[i + 2], intents[i + 3], intents[i + 4]);
        const to = new THREE.Vector3(intents[i + 5], intents[i + 6], intents[i + 7]);
        const d = to.clone().sub(from);
        this.intentGroup.add(
          new THREE.ArrowHelper(d.clone().normalize(), from, d.length(), intents[i + 1] === 1 ? 0x3399ff : 0xffaa00),
        );
      }
    }
  }

  clearContacts(): void {
    for (const c of this.contacts) {
      c.active = false;
      c.obj.visible = false;
    }
  }

  /** Contact markers currently shown (automation). */
  activeContacts(): number {
    return this.contacts.filter((c) => c.active).length;
  }

  /** City tiles: total, hidden by the draw distance, drawn (frustum too; needs the camera). */
  cityStats(camera?: THREE.Camera): { tiles: number; hidden: number; drawn: number } {
    const t = this.cityShown ? this.cityTiles.get(this.cityShown) : undefined;
    return { tiles: t?.meshes.length ?? 0, hidden: t?.hidden ?? 0, drawn: t && camera ? t.drawn(camera) : 0 };
  }

  render(camera: THREE.PerspectiveCamera): void {
    const tiles = this.cityShown ? this.cityTiles.get(this.cityShown) : undefined;
    if (tiles) {
      camera.getWorldPosition(this.camPos);
      let far = Math.min(this.drawDistanceM, camera.far);
      // Fully fogged geometry (FogExp2 factor > 0.999) looks like the fog-coloured sky.
      const fog = this.scene.fog;
      if (fog instanceof THREE.FogExp2 && fog.density > 0) far = Math.min(far, 2.7 / fog.density);
      else if (fog instanceof THREE.Fog) far = Math.min(far, fog.far);
      tiles.update(this.camPos, far);
    }
    if (this.renderOverride) this.renderOverride(camera);
    else this.renderer.render(this.scene, camera);
  }
}
