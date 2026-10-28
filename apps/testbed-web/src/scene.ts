// Scene: renderer, lights, sky colour from TimeOfDay, ground + 10 m grid, the city greybox
// (data/city/<id>.greybox.glb, prompt 05), police helicopters, barriers, fading contact
// markers, garage markers and intent arrows. Traffic and police cars are drawn by render/lod.ts.
// Sim space is +Z up / +Y forward; Three.js is +Y up, so `world` is rotated -90° about X and
// every sim-space object is parented under it.
import * as THREE from "three";
import { tagFeature } from "./perf";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { setCarEnvironment } from "./render/panels";
import type { Barrier, GarageSite, SafehouseSite, SimEvent } from "./wasm/sim";
import { D, DRAW_STRIDE, INTENT_STRIDE, POLICE_DRAW_FLAG, TL, TOOL_KIND, TOOL_STRIDE } from "./wasm/layout";
import { layers } from "./settings";
import { SignalView } from "./signals_view";
import { CityTiles } from "./render/city_tiles";
import { Quality } from "./render/quality";
import { MarkingsView } from "./markings_view";

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
/** Garage marker colours: legit (blue) and crooked (amber) shops. */
const GARAGE_COLOUR = { Legit: 0x3399ff, Crooked: 0xffaa22 } as const;
/** Safehouse ring and beam colour (career runs). */
const SAFEHOUSE_COLOUR = 0x33dd88;
/** Car lots (pink) and the player's own garages (white) in career runs. */
const LOT_COLOUR = 0xff7ac8;
const HOME_COLOUR = 0xf2f2f2;
/** Height of a garage's light beam, m (visible over the greybox blocks). */
const GARAGE_BEAM_M = 40;
const CONTACT_DOT = new THREE.SphereGeometry(0.08, 8, 6);
const tmpNormal = new THREE.Vector3();
const groundRay = new THREE.Raycaster();
const groundFrom = new THREE.Vector3();
const groundDown = new THREE.Vector3(0, -1, 0);
/** Floats per helicopter in police_helicopters(). */
const HELO_STRIDE = 8;
/** The spotlight is on from this night factor (0 noon … 1 midnight). */
const SPOTLIGHT_NIGHT = 0.35;

interface HeloAirframe {
  obj: THREE.Group;
  rotor: THREE.Object3D;
  tailRotor: THREE.Object3D;
  nav: THREE.MeshStandardMaterial;
  /** Light bar (flashes while the unit is pursuing). */
  red: THREE.MeshStandardMaterial;
  blue: THREE.MeshStandardMaterial;
}

/** A helicopter spotlight. Unlit meshes only, no THREE light: a changing light count recompiles
 * every lit shader mid-chase and every light costs every lit pixel (see `sirenLight`). */
interface HeloBeam {
  /** Visible beam: unit cone (apex at the origin, opening along +Z), scaled to length/radius. */
  cone: THREE.Mesh;
  /** Pool of light on the ground at the aim point. */
  pool: THREE.Mesh;
}

/** Police helicopter: fuselage, tail boom, skids, main + tail rotor, a blinking nav light and a
 * red / blue light bar under the nose. */
function makeHelicopter(): HeloAirframe {
  const g = new THREE.Group();
  const body = new THREE.MeshStandardMaterial({ color: 0x1d2433 });
  const white = new THREE.MeshStandardMaterial({ color: 0xe9ebf0 });
  const glass = new THREE.MeshStandardMaterial({ color: 0x223344, emissive: 0x0a1220 });
  const fuselage = new THREE.Mesh(new THREE.BoxGeometry(2.2, 5.0, 2.2), body);
  fuselage.position.set(0, 0.5, 1.6);
  const stripe = new THREE.Mesh(new THREE.BoxGeometry(2.25, 4.0, 0.4), white);
  stripe.position.set(0, 0.3, 1.4);
  const nose = new THREE.Mesh(new THREE.BoxGeometry(1.9, 1.4, 1.5), glass);
  nose.position.set(0, 3.4, 1.4);
  const boom = new THREE.Mesh(new THREE.BoxGeometry(0.5, 5.5, 0.5), body);
  boom.position.set(0, -4.6, 2.1);
  const fin = new THREE.Mesh(new THREE.BoxGeometry(0.15, 0.9, 1.6), body);
  fin.position.set(0, -7.2, 2.7);
  const skidGeo = new THREE.BoxGeometry(0.15, 5.0, 0.15);
  const skidL = new THREE.Mesh(skidGeo, white);
  skidL.position.set(-1.2, 0.5, 0.1);
  const skidR = new THREE.Mesh(skidGeo, white);
  skidR.position.set(1.2, 0.5, 0.1);
  const rotor = new THREE.Group();
  const bladeMat = new THREE.MeshStandardMaterial({ color: 0x111111 });
  for (let k = 0; k < 2; k++) {
    const blade = new THREE.Mesh(new THREE.BoxGeometry(10.5, 0.35, 0.06), bladeMat);
    blade.rotation.z = (k * Math.PI) / 2;
    rotor.add(blade);
  }
  rotor.position.set(0, 0.5, 2.95);
  const tailRotor = new THREE.Group();
  const tb = new THREE.Mesh(new THREE.BoxGeometry(0.06, 0.2, 1.8), bladeMat);
  tailRotor.add(tb);
  tailRotor.position.set(0.3, -7.2, 2.9);
  const nav = new THREE.MeshStandardMaterial({ color: 0x400000, emissive: 0x000000 });
  const beacon = new THREE.Mesh(new THREE.BoxGeometry(0.25, 0.25, 0.25), nav);
  beacon.position.set(0, -1.2, 2.8);
  const red = new THREE.MeshStandardMaterial({ color: 0x661018 });
  const blue = new THREE.MeshStandardMaterial({ color: 0x102266 });
  const barR = new THREE.Mesh(new THREE.BoxGeometry(0.6, 0.25, 0.12), red);
  barR.position.set(-0.32, 2.2, 0.4);
  const barB = new THREE.Mesh(new THREE.BoxGeometry(0.6, 0.25, 0.12), blue);
  barB.position.set(0.32, 2.2, 0.4);
  g.add(fuselage, stripe, nose, boom, fin, skidL, skidR, rotor, tailRotor, beacon, barR, barB);
  return { obj: g, rotor, tailRotor, nav, red, blue };
}

/** One spotlight's beam and ground pool, hidden, added to `parent`. */
function makeBeam(parent: THREE.Object3D): HeloBeam {
  // Unit cone: apex at the origin, opening along +Z (lookAt's axis), length 1, base radius 1.
  // Three's cone has its apex at +Y 0.5; rotating -90° about X puts it at -Z 0.5.
  const geo = new THREE.ConeGeometry(1, 1, 24, 1, true);
  geo.rotateX(-Math.PI / 2);
  geo.translate(0, 0, 0.5);
  const cone = new THREE.Mesh(
    geo,
    new THREE.MeshBasicMaterial({
      color: 0xfff6d8,
      transparent: true,
      opacity: 0.07,
      blending: THREE.AdditiveBlending,
      depthWrite: false,
      side: THREE.DoubleSide,
      // One pass: transparent + DoubleSide would flip `side` and re-resolve the program twice a frame.
      forceSinglePass: true,
    }),
  );
  const pool = new THREE.Mesh(
    new THREE.CircleGeometry(1, 32),
    new THREE.MeshBasicMaterial({
      color: 0xfff6d8,
      transparent: true,
      opacity: 0.18,
      blending: THREE.AdditiveBlending,
      depthWrite: false,
    }),
  );
  cone.visible = pool.visible = false;
  parent.add(cone, pool);
  return { cone, pool };
}

export class SceneView {
  readonly renderer: THREE.WebGLRenderer;
  /** Graphics quality: MSAA, resolution scale (adaptive in auto mode), draw distance. */
  readonly quality = new Quality();
  readonly scene = new THREE.Scene();
  readonly world = new THREE.Group();
  readonly hemi: THREE.HemisphereLight;
  readonly sun: THREE.DirectionalLight;
  private readonly barriers = new THREE.Group();
  private readonly garages = new THREE.Group();
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
  /** Small city detail (city_tiles.ts CITY_DETAIL) past this distance is not drawn, m. */
  detailDistanceM = Infinity;
  private readonly camPos = new THREE.Vector3();
  /** Police helicopter airframes (police_draw()), pooled; see updateHelicopters(). */
  private readonly helicopters: HeloAirframe[] = [];
  /** Helicopter spotlights (police_helicopters()), pooled; see updateHelicopterBeams(). */
  private readonly beams: HeloBeam[] = [];
  /** Helicopters drawn last frame and spotlights lit (for automation). */
  helicoptersShown = 0;
  spotlightsLit = 0;
  /** Roadblock barriers (police_tools()), pooled; see updatePoliceTools(). */
  private readonly tools = new THREE.Group();
  private readonly toolPool: { barrier: THREE.Group; body: THREE.Mesh; bulb: THREE.Mesh; lamp: THREE.MeshStandardMaterial }[] = [];
  /** Light-bar glow multiplier for the police helicopters (art style). */
  policeGlow = 1;
  /** Replaces renderer.render in render() (art style post stack); null = plain render. The
   * city-tile update runs before it either way. */
  renderOverride: ((camera: THREE.Camera) => void) | null = null;
  /** Replaces renderer.compile in prewarm() (the art style compiles for its HDR target). */
  compileOverride: ((camera: THREE.Camera, objects: THREE.Scene) => void) | null = null;
  /** Draw kinds (PREWARM_* bits) each material was prewarmed for. */
  private readonly prewarmed = new WeakMap<THREE.Material, number>();
  /** Stand-ins of the materials being prewarmed (lights come from the main scene). */
  private readonly prewarmScene = new THREE.Scene();
  private lastPrewarm = -Infinity;
  /** Material / draw-kind pairs prewarmed so far (automation). */
  prewarmCount = 0;
  /** Subtrees prewarmed even while hidden (crash effects: hidden until the first spark). */
  readonly prewarmRoots: THREE.Object3D[] = [];

  private debugGrid!: THREE.GridHelper;
  /** Axis hint at the sim origin (hidden in the workshop). */
  private originAxes!: THREE.AxesHelper;
  /** A car lot visit is shown (setLot). */
  private lotOn = false;
  /** The flat world's ground plane (hidden inside the garage workshop). */
  private flatGround!: THREE.Mesh;
  private workshopOn = false;
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
  /** Painted road markings (markings_view.ts). */
  readonly markings = new MarkingsView();

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
    // Image-based lighting for the car materials (detailed-car-models), so metallic paint,
    // chrome and bare metal read as metal instead of black: they only reflect an environment,
    // never the direct lights. Car materials only (render/panels.ts applyCarEnv), not
    // `scene.environment`, which would relight the whole city.
    const pmrem = new THREE.PMREMGenerator(this.renderer);
    setCarEnvironment(pmrem.fromScene(new RoomEnvironment(), 0.04).texture);
    pmrem.dispose();

    this.scene.background = DAY_SKY.clone();
    this.hemi = new THREE.HemisphereLight(0xffffff, 0x333333, 2.0);
    this.sun = new THREE.DirectionalLight(0xffffff, 1.5);
    this.sun.position.set(30, 60, 20);
    this.scene.add(this.hemi, this.sun);

    this.world.rotation.x = -Math.PI / 2;
    this.scene.add(this.world);
    const ground = new THREE.Mesh(new THREE.PlaneGeometry(4000, 4000), this.groundMaterial);
    this.flatGround = ground;
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
    this.originAxes = axes;
    this.sirenLight.visible = false;
    tagFeature(this.signals.group, "signals");
    this.world.add(axes, this.barriers, this.garages, this.tools, this.intentGroup, this.signals.group, this.markings.group, this.sirenLight);
    // One helicopter spotlight exists from the start (hidden) so its two programs are prewarmed,
    // not compiled in the frame the first helicopter switches it on.
    const beam = makeBeam(this.world);
    this.beams.push(beam);
    this.prewarmRoots.push(beam.cone, beam.pool);
    // The first contact marker exists from the start (hidden), so its shader programs are
    // prewarmed instead of compiled in the frame of the first contact (browser-perf.md 12b).
    const first = this.newContactMarker();
    first.active = false;
    first.obj.visible = false;
    this.contacts[0] = first;
    this.prewarmRoots.push(first.obj);
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
    this.debugGrid.visible = !cityId && !this.workshopOn && !this.lotOn;
    if (!cityId || this.cities.has(cityId)) return;
    const holder = new THREE.Group();
    this.cities.set(cityId, holder);
    tagFeature(holder, "city");
    this.scene.add(holder);
    try {
      // A city shipped as parts (`<id>.greybox.parts.json`: Port Vesper's shared part is
      // loaded by both `vesper_v2` and `ashpeak`, city-import `parts`) loads them all into one
      // group; any other city is one greybox.
      const gltf = { scene: await loadGreybox(cityId) };
      // glTF is +Y up, same as Three: add to the scene, not the Z-up world group.
      // Layers that share a plane in the greybox (bus lanes and parking painted on asphalt,
      // sidewalks, their corner fills and the level lots behind them on zone ground) draw over
      // the layer below instead of flickering.
      // City v2 adds terrain (ground_* / rock / seabed, smooth-shaded) and a sea plane 0.25 m
      // above sea level (no polygon offset: at long range it would win against the land);
      // walls, kerbs and decks come without normals and are flat-shaded.
      const layer: Record<string, number> = {
        bus_lane: 2,
        parking: 2,
        sidewalk: 1,
        sidewalk_corner: 1,
        verge: 1,
        lot: 1,
        lot_skirt: 1,
        // Car parks lie 2 cm over the sidewalk and lot they cover.
        car_park: 2,
        asphalt: 1,
        highway_concrete: 1,
        gravel_road: 1,
        dirt_road: 1,
        ramp_taper: 2,
      };
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
      this.cityTiles.set(cityId, new CityTiles(gltf.scene, this.quality.cityTileM));
      this.onCityLoaded?.(gltf.scene, cityId);
      this.warmLightVariants();
    } catch (e) {
      // Forget the empty holder so the next session in this city tries again (a cached empty
      // holder kept the city invisible for the rest of the page's life).
      this.scene.remove(holder);
      this.cities.delete(cityId);
      console.warn("city greybox not available", cityId, e);
    }
  }

  /**
   * Height of the shown city's surface under sim-space `(x, y)` (m), by a downward ray against
   * the greybox: the topmost surface, or with `nearZ` the one nearest that height; null without
   * a city or a hit. For rarely placed objects (spike strips), not per frame per car.
   */
  /** Garage workshop: hide the flat ground and grid (the bay has its own floor). */
  setWorkshop(on: boolean): void {
    this.workshopOn = on;
    this.flatGround.visible = !on;
    this.debugGrid.visible = !on && !this.cityShown && !this.lotOn;
    this.originAxes.visible = !on && !this.lotOn;
  }

  /** Car lot visit: hide the origin axes and the 10 m debug grid (the lot has its own paving). */
  setLot(on: boolean): void {
    this.lotOn = on;
    this.originAxes.visible = !on && !this.workshopOn;
    this.debugGrid.visible = !on && !this.workshopOn && !this.cityShown;
  }

  groundZ(x: number, y: number, nearZ?: number): number | null {
    const city = this.cityShown ? this.cities.get(this.cityShown) : null;
    if (!city) return null;
    // Sim (x, y, z) is Three (x, z, -y) (world rotated -90 deg about X).
    groundRay.set(groundFrom.set(x, 2000, -y), groundDown);
    groundRay.far = 4000;
    const hits = groundRay.intersectObject(city, true);
    if (hits.length === 0) return null;
    // Without a reference height the topmost surface; with one, the surface nearest it (a
    // tunnel's floor, not the hillside over it; the road under a bridge, not the deck).
    if (nearZ === undefined) return hits[0].point.y;
    let best = hits[0].point.y;
    for (const h of hits) if (Math.abs(h.point.y - nearZ) < Math.abs(best - nearZ)) best = h.point.y;
    return best;
  }

  /**
   * Draw police helicopters from police_draw() records (layout.ts D; flags 1 helicopter,
   * 2 pursuing): an airframe at altitude with a spinning rotor, a blinking beacon and a flashing
   * light bar while pursuing. Police cars, bound or not, are drawn by render/lod.ts.
   */
  updateHelicopters(units: Float32Array, timeS: number): void {
    let n = 0;
    for (let o = 0; o < units.length; o += DRAW_STRIDE) {
      const flags = units[o + D.flags];
      if (!(flags & POLICE_DRAW_FLAG.helicopter)) continue;
      if (n >= this.helicopters.length) {
        const h = makeHelicopter();
        h.obj.scale.setScalar(1.4);
        this.world.add(h.obj);
        this.helicopters.push(h);
      }
      const h = this.helicopters[n];
      h.obj.position.set(units[o + D.pos], units[o + D.pos + 1], units[o + D.pos + 2]);
      h.obj.quaternion.set(units[o + D.rot], units[o + D.rot + 1], units[o + D.rot + 2], units[o + D.rot + 3]);
      // Rotor speed is a look, not a simulation: about 6 rev/s main, 30 rev/s tail.
      h.rotor.rotation.z = (timeS * 38) % (Math.PI * 2);
      h.tailRotor.rotation.x = (timeS * 190) % (Math.PI * 2);
      h.nav.emissive.setHex((timeS + n * 0.3) % 1 < 0.12 ? 0xff2020 : 0x000000);
      const chasing = (flags & POLICE_DRAW_FLAG.chasing) !== 0;
      const phase = (timeS * 2.5 + n * 0.17) % 1;
      h.red.emissive.setHex(chasing && phase < 0.5 ? 0xff2040 : 0x000000);
      h.blue.emissive.setHex(chasing && phase >= 0.5 ? 0x2060ff : 0x000000);
      h.red.emissiveIntensity = h.blue.emissiveIntensity = this.policeGlow;
      n++;
    }
    this.helicopters.forEach((h, i) => (h.obj.visible = i < n));
    this.helicoptersShown = n;
  }

  /**
   * Draw roadblock barriers from police_tools() as red/white striped boxes with a flashing amber
   * lamp. Their pose follows the physics body, so they visibly move when hit. Spike strips in
   * the same list are skipped: render/police_tools.ts already draws them (from map_dynamic()).
   */
  updatePoliceTools(tools: Float32Array, timeS: number): void {
    const barriers: number[] = [];
    for (let o = 0; o < tools.length; o += TOOL_STRIDE) if (tools[o + TL.kind] === TOOL_KIND.barrier) barriers.push(o);
    while (this.toolPool.length < barriers.length) {
      const barrier = new THREE.Group();
      const white = new THREE.MeshStandardMaterial({ color: 0xf2f2f2 });
      const red = new THREE.MeshStandardMaterial({ color: 0xd01818 });
      // Unit box scaled to the barrier size; the red bands are its children so they scale too.
      const body = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), white);
      for (let k = 0; k < 5; k += 2) {
        const band = new THREE.Mesh(new THREE.BoxGeometry(0.2, 1.02, 0.5), red);
        band.position.set(-0.4 + k * 0.2, 0, 0.1);
        body.add(band);
      }
      const lamp = new THREE.MeshStandardMaterial({ color: 0x664400, emissive: 0x000000 });
      const bulb = new THREE.Mesh(new THREE.SphereGeometry(0.09, 10, 8), lamp);
      barrier.add(body, bulb);
      this.tools.add(barrier);
      this.toolPool.push({ barrier, body, bulb, lamp });
    }
    const flash = timeS % 1 < 0.5;
    this.toolPool.forEach((t, i) => {
      t.barrier.visible = i < barriers.length;
      if (!t.barrier.visible) return;
      const o = barriers[i];
      t.barrier.position.set(tools[o + TL.x], tools[o + TL.y], tools[o + TL.z]);
      t.barrier.quaternion.set(tools[o + TL.qx], tools[o + TL.qy], tools[o + TL.qz], tools[o + TL.qw]);
      const hx = tools[o + TL.hx], hy = tools[o + TL.hy], hz = tools[o + TL.hz];
      // The body (with its bands) scales to the barrier; the lamp sits on top at fixed size.
      t.body.scale.set(2 * hx, 2 * hy, 2 * hz);
      t.bulb.position.set(0, 0, hz + 0.09);
      t.lamp.emissive.setHex(flash ? 0xffaa00 : 0x000000);
    });
  }

  /**
   * Helicopter spotlights (police_helicopters(): [x, y, z, aimX, aimY, aimZ, coneHalfAngle,
   * flags], flag 1 = has visual). At night each tracking helicopter lights its target: a faint
   * visible beam and a pool of light on the road (additive, unlit: no THREE light). Without
   * visual the beam stays on the last known position, dimmer, so the player can see that the
   * helicopter has lost them.
   */
  updateHelicopterBeams(helos: Float32Array, night: number): void {
    const n = helos.length / HELO_STRIDE;
    while (this.beams.length < n) this.beams.push(makeBeam(this.world));
    const on = night >= SPOTLIGHT_NIGHT;
    let lit = 0;
    const from = new THREE.Vector3();
    const to = new THREE.Vector3();
    this.beams.forEach((b, i) => {
      const show = on && i < n;
      b.cone.visible = b.pool.visible = show;
      if (!show) return;
      lit++;
      const o = i * HELO_STRIDE;
      from.set(helos[o], helos[o + 1], helos[o + 2]);
      to.set(helos[o + 3], helos[o + 4], helos[o + 5]);
      const half = helos[o + 6];
      const visual = (helos[o + 7] & 1) !== 0;
      const k = Math.min(1, (night - SPOTLIGHT_NIGHT) / 0.3) * (visual ? 1 : 0.45);
      const len = from.distanceTo(to);
      const r = Math.tan(half) * len;
      b.cone.position.copy(from);
      b.cone.lookAt(this.world.localToWorld(to.clone()));
      b.cone.scale.set(r, r, len);
      (b.cone.material as THREE.MeshBasicMaterial).opacity = 0.07 * k;
      b.pool.position.set(to.x, to.y, to.z + 0.08);
      b.pool.scale.setScalar(r);
      (b.pool.material as THREE.MeshBasicMaterial).opacity = 0.18 * k;
    });
    this.spotlightsLit = lit;
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

  /** Repair shop markers (`garages_json()`): a ring on the ground at the shop's radius and a
   * light beam, blue for legit shops, amber for crooked ones. Stop inside the ring to repair.
   * Safehouses (`safehouses_json()`, career runs) get the same marker in green; car lots and the
   * player's own garages (`places_json()` kinds Lot / Home) in pink and white. */
  setGarages(list: GarageSite[], safehouses: SafehouseSite[] = [], others: { kind: string; name: string; pos: [number, number, number]; radius_m: number }[] = []): void {
    for (const old of this.garages.children) {
      old.traverse((o) => {
        if (o instanceof THREE.Mesh) {
          o.geometry.dispose();
          (o.material as THREE.Material).dispose();
        }
      });
    }
    this.garages.clear();
    const places = [
      ...list.map((g) => ({ name: `garage:${g.name}`, pos: g.pos, radius_m: g.radius_m, colour: GARAGE_COLOUR[g.kind] ?? GARAGE_COLOUR.Legit })),
      ...safehouses.map((h) => ({ name: `safehouse:${h.name}`, pos: h.pos, radius_m: h.radius_m, colour: SAFEHOUSE_COLOUR })),
      ...others
        .filter((p) => p.kind === "Lot" || p.kind === "Home")
        .map((p) => ({ name: `${p.kind.toLowerCase()}:${p.name}`, pos: p.pos, radius_m: p.radius_m, colour: p.kind === "Lot" ? LOT_COLOUR : HOME_COLOUR })),
    ];
    for (const g of places) {
      const colour = g.colour;
      const holder = new THREE.Group();
      holder.position.set(...g.pos);
      const ring = new THREE.Mesh(
        new THREE.RingGeometry(g.radius_m - 0.6, g.radius_m, 48),
        // Single pass (transparent + DoubleSide would flip `side` and re-resolve the program twice a frame).
        new THREE.MeshBasicMaterial({ color: colour, transparent: true, opacity: 0.6, side: THREE.DoubleSide, depthWrite: false, forceSinglePass: true }),
      );
      ring.position.z = 0.08;
      const beam = new THREE.Mesh(
        new THREE.CylinderGeometry(0.5, 0.5, GARAGE_BEAM_M, 12, 1, true),
        new THREE.MeshBasicMaterial({ color: colour, transparent: true, opacity: 0.35, depthWrite: false }),
      );
      // CylinderGeometry runs along +Y; the world group is +Z up.
      beam.rotation.x = Math.PI / 2;
      beam.position.z = GARAGE_BEAM_M / 2;
      holder.add(ring, beam);
      holder.name = g.name;
      this.garages.add(holder);
    }
  }

  private newContactMarker(): ContactMarker {
    const g = new THREE.Group();
    const dot = new THREE.Mesh(CONTACT_DOT, new THREE.MeshBasicMaterial({ color: 0xff00ff, transparent: true, depthTest: false }));
    const arrow = new THREE.ArrowHelper(new THREE.Vector3(0, 0, 1), new THREE.Vector3(), 1, 0xff00ff);
    for (const mat of [arrow.line.material, arrow.cone.material] as THREE.Material[]) mat.transparent = true;
    g.add(dot, arrow);
    g.renderOrder = 6;
    this.world.add(g);
    return { obj: g, dot, arrow, age: 0, active: true };
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
        m = this.newContactMarker();
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

  /**
   * Compile shader programs for every visible material in the scene (and `prewarmRoots`, hidden
   * or not: effect pools) for both states of the shared siren light. The light toggling changes the
   * point-light count, so each lit material needs two programs; compiling them on demand
   * recompiled ~15 programs in the frame the first siren came on (a 0.5–1 s hitch under
   * SwiftShader, browser-perf.md round 12b).
   *
   * Incremental: only (material, draw kind) pairs not compiled before are compiled, through a
   * small scene of stand-in objects sharing their geometry and material (renderer.compile
   * calls getProgram per object, and the city alone has thousands). Returns how many were new.
   */
  prewarm(camera: THREE.Camera): number {
    const fresh: THREE.Object3D[] = [];
    const visit = (o: THREE.Object3D) => {
      const kind = prewarmKind(o);
      if (!kind) return;
      const mats = (o as THREE.Mesh).material as THREE.Material | THREE.Material[];
      for (const m of Array.isArray(mats) ? mats : [mats]) {
        if (!m) continue;
        const done = this.prewarmed.get(m) ?? 0;
        if (done & kind) continue;
        this.prewarmed.set(m, done | kind);
        const stand = prewarmStandIn(o, m, kind);
        if (stand) fresh.push(stand);
      }
    };
    // Visible objects (hidden debug layers compile when switched on), plus roots that start
    // hidden but appear mid-game (effect pools).
    this.scene.traverseVisible(visit);
    for (const r of this.prewarmRoots) r.traverse(visit);
    if (fresh.length === 0) return 0;
    const holder = this.prewarmScene;
    holder.fog = this.scene.fog;
    holder.add(...fresh);
    const on = this.sirenLight.visible;
    for (const vis of [!on, on]) {
      this.sirenLight.visible = vis;
      if (this.compileOverride) this.compileOverride(camera, holder);
      else this.renderer.compile(holder, camera, this.scene);
    }
    this.sirenLight.visible = on;
    holder.clear();
    // renderer.compile only creates the GL programs; the driver (SwiftShader especially)
    // compiles and links when the program is first queried, which Three does on its first
    // draw. Query them now so that cost lands here, not in the frame that first draws them.
    for (const p of this.renderer.info.programs ?? []) (p as unknown as { getUniforms(): unknown }).getUniforms();
    return fresh.length;
  }

  /** prewarm() at most every `everyMs` (call each frame). */
  prewarmEvery(camera: THREE.Camera, nowMs: number, everyMs = 1000): void {
    if (nowMs - this.lastPrewarm < everyMs) return;
    this.lastPrewarm = nowMs;
    this.prewarmCount += this.prewarm(camera);
  }

  /** How far city geometry is drawn this frame, m: the draw distance, the camera's far plane
   * and the fully fogged distance (FogExp2 factor > 0.999 looks like the fog-coloured sky).
   * Other world layers (parked cars) cull at the same distance. */
  cullDistanceM(camera: THREE.PerspectiveCamera): number {
    let far = Math.min(this.drawDistanceM, camera.far);
    const fog = this.scene.fog;
    if (fog instanceof THREE.FogExp2 && fog.density > 0) far = Math.min(far, 2.7 / fog.density);
    else if (fog instanceof THREE.Fog) far = Math.min(far, fog.far);
    return far;
  }

  render(camera: THREE.PerspectiveCamera): void {
    const tiles = this.cityShown ? this.cityTiles.get(this.cityShown) : undefined;
    if (tiles) {
      camera.getWorldPosition(this.camPos);
      tiles.update(this.camPos, this.cullDistanceM(camera), this.detailDistanceM);
    }
    if (this.markings.tileCount > 0) {
      camera.getWorldPosition(this.camPos);
      this.markings.update(this.camPos, this.cullDistanceM(camera));
    }
    // Three bakes the number of visible point lights into every lit shader, so the shared siren
    // light turning on (first police lights) used to recompile every lit material mid-chase
    // (~13 programs, seconds under a software rasterizer). A few frames after a session or city
    // starts, one hidden render with the siren in the other state builds those variants up
    // front (a real render: only what is visible, exactly the programs the frame will use).
    if (this.sirenWarmIn > 0 && --this.sirenWarmIn === 0) {
      const siren = this.sirenLight;
      const visible = siren.visible;
      const intensity = siren.intensity;
      siren.visible = !visible;
      if (!visible) siren.intensity = 0;
      this.draw(camera);
      siren.visible = visible;
      siren.intensity = intensity;
      this.lightWarmups++;
    }
    this.draw(camera);
  }

  /** Frames until the siren-variant warm-up render (0 = none pending). */
  private sirenWarmIn = 0;
  /** Warm-up renders done (automation). */
  lightWarmups = 0;

  /** Schedule the siren-variant warm-up render a few frames from now (new session or city:
   * new materials and meshes). */
  warmLightVariants(): void {
    this.sirenWarmIn = 4;
  }

  private draw(camera: THREE.PerspectiveCamera): void {
    if (this.renderOverride) this.renderOverride(camera);
    else this.renderer.render(this.scene, camera);
  }

  /**
   * Build the shader programs of `root` (a subtree of this scene) now, hidden meshes included,
   * for both siren-light states: e.g. the crash effects, whose meshes stay hidden until the
   * first crash and otherwise compiled ~11 programs mid-chase (a long hitch under a software
   * rasterizer). Uses the real draw path (post target, fog, lights, shader hooks) with every
   * other object hidden, so the programs match the ones the frame loop will ask for; empty
   * instanced meshes build their program without drawing anything.
   */
  precompile(root: THREE.Object3D, camera: THREE.PerspectiveCamera): void {
    const changed: [THREE.Object3D, boolean][] = [];
    const set = (o: THREE.Object3D, v: boolean) => {
      if (o.visible !== v) {
        changed.push([o, o.visible]);
        o.visible = v;
      }
    };
    // Hide everything that is not a light, an ancestor of `root`, or inside `root`.
    const keep = new Set<THREE.Object3D>();
    for (let o: THREE.Object3D | null = root; o; o = o.parent) keep.add(o);
    this.scene.traverse((o) => {
      if (keep.has(o) || (o as THREE.Light).isLight) return;
      let inside = false;
      for (let p: THREE.Object3D | null = o; p; p = p.parent) if (p === root) inside = true;
      if (inside) set(o, true);
      else if (!keep.has(o.parent!)) return;
      else set(o, false);
    });
    const siren = this.sirenLight;
    const visible = siren.visible;
    const intensity = siren.intensity;
    for (const on of [!visible, visible]) {
      siren.visible = on;
      if (on && !visible) siren.intensity = 0;
      this.draw(camera);
    }
    siren.visible = visible;
    siren.intensity = intensity;
    for (let i = changed.length - 1; i >= 0; i--) changed[i][0].visible = changed[i][1];
  }


}

/** Draw kinds that need their own shader program variant (prewarm bookkeeping). */
const PREWARM_MESH = 1;
const PREWARM_INSTANCED = 2;
const PREWARM_INSTANCED_COLOUR = 4;
const PREWARM_LINE = 8;
const PREWARM_POINTS = 16;

function prewarmKind(o: THREE.Object3D): number {
  if ((o as THREE.InstancedMesh).isInstancedMesh) return (o as THREE.InstancedMesh).instanceColor ? PREWARM_INSTANCED_COLOUR : PREWARM_INSTANCED;
  if ((o as THREE.SkinnedMesh).isSkinnedMesh || (o as THREE.Sprite).isSprite) return 0;
  if ((o as THREE.Mesh).isMesh) return PREWARM_MESH;
  if ((o as THREE.Line).isLine) return PREWARM_LINE;
  if ((o as THREE.Points).isPoints) return PREWARM_POINTS;
  return 0;
}

/** An object that compiles to the same program as `o` with material `m`. */
function prewarmStandIn(o: THREE.Object3D, m: THREE.Material, kind: number): THREE.Object3D | null {
  const geo = (o as THREE.Mesh).geometry as THREE.BufferGeometry;
  switch (kind) {
    case PREWARM_INSTANCED:
    case PREWARM_INSTANCED_COLOUR: {
      const im = new THREE.InstancedMesh(geo, m, 1);
      im.instanceColor = (o as THREE.InstancedMesh).instanceColor;
      return im;
    }
    case PREWARM_MESH:
      return new THREE.Mesh(geo, m);
    case PREWARM_LINE:
      return (o as THREE.LineSegments).isLineSegments ? new THREE.LineSegments(geo, m) : new THREE.Line(geo, m);
    case PREWARM_POINTS:
      return new THREE.Points(geo, m);
  }
  return null;
}

/** One greybox file (`data/city/<stem>.greybox.glb`, else its base64 text: the published
 * build cannot serve binary files). */
async function loadGreyboxFile(stem: string): Promise<THREE.Object3D> {
  try {
    return (await new GLTFLoader().loadAsync(`data/city/${stem}.greybox.glb`)).scene;
  } catch {
    const b64 = await fetchGreyboxB64(stem);
    const bytes = Uint8Array.from(atob(b64.trim()), (c) => c.charCodeAt(0));
    return (await new GLTFLoader().parseAsync(bytes.buffer, "")).scene;
  }
}

/** The greybox of `cityId`: its parts in manifest order (`<id>.greybox.parts.json`,
 * `{"parts": [...]}`), or the single `<id>.greybox` file. */
async function loadGreybox(cityId: string): Promise<THREE.Object3D> {
  let parts: string[] | null = null;
  try {
    const r = await fetch(`data/city/${cityId}.greybox.parts.json`);
    if (r.ok) {
      const text = await r.text();
      // A dev server answers unknown paths with index.html.
      if (!text.trimStart().startsWith("<")) parts = (JSON.parse(text) as { parts: string[] }).parts;
    }
  } catch {
    parts = null;
  }
  if (!parts || parts.length === 0) return loadGreyboxFile(cityId);
  const scenes = await Promise.all(parts.map((p) => loadGreyboxFile(p)));
  const group = new THREE.Group();
  group.name = `${cityId}.greybox`;
  for (const sc of scenes) group.add(sc);
  return group;
}

/**
 * Fetches a city greybox as base64 text. The published build caps each file at 15 MB, so
 * `tools/preview_build.sh` splits larger greyboxes into `<id>.greybox.b64.partN.txt` files
 * listed in `<id>.greybox.b64.parts.txt` (one line: the part count). Those are joined in order.
 */
async function fetchGreyboxB64(cityId: string): Promise<string> {
  const base = `data/city/${cityId}.greybox.b64`;
  const whole = await fetch(`${base}.txt`);
  if (whole.ok) {
    const text = await whole.text();
    if (!text.trimStart().startsWith("<")) return text;
  }
  const manifest = await fetch(`${base}.parts.txt`);
  if (!manifest.ok) throw new Error(`greybox ${cityId}: no .b64.txt and no parts manifest`);
  const count = parseInt((await manifest.text()).trim(), 10);
  if (!(count > 0)) throw new Error(`greybox ${cityId}: bad parts manifest`);
  const parts: string[] = [];
  for (let i = 0; i < count; i++) {
    const r = await fetch(`${base}.part${i}.txt`);
    if (!r.ok) throw new Error(`greybox ${cityId}: missing part ${i}`);
    parts.push((await r.text()).trim());
  }
  return parts.join("");
}
