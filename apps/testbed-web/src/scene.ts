// Scene: renderer, lights, sky colour from TimeOfDay, ground + 10 m grid, the city greybox
// (data/city/<id>.greybox.glb, prompt 05) and its traffic agents, barriers, fading contact
// markers and intent arrows.
// Sim space is +Z up / +Y forward; Three.js is +Y up, so `world` is rotated -90° about X and
// every sim-space object is parented under it.
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import type { Barrier, SimEvent } from "./wasm/sim";
import { AGENT_FLAG, AGENT_STRIDE, INTENT_STRIDE, TL, TOOL_KIND, TOOL_STRIDE } from "./wasm/layout";
import { layers } from "./settings";
import { SignalView } from "./signals_view";

const DAY_SKY = new THREE.Color(0x8fb8de);
const NIGHT_SKY = new THREE.Color(0x070a14);

interface ContactMarker {
  obj: THREE.Group;
  age: number;
}

const CONTACT_LIFETIME_S = 1.5;
/** Instanced traffic capacity. */
const MAX_AGENTS = 1024;
/** Traffic car box size [w, l, h], m. */
const AGENT_SCALE = new THREE.Vector3(1.8, 4.4, 1.4);
const tmpAgentColour = new THREE.Color();
/** Floats per unit in police_units(). */
const POLICE_UNIT_STRIDE = 6;

export class SceneView {
  readonly renderer: THREE.WebGLRenderer;
  readonly scene = new THREE.Scene();
  readonly world = new THREE.Group();
  private readonly hemi: THREE.HemisphereLight;
  private readonly sun: THREE.DirectionalLight;
  private readonly barriers = new THREE.Group();
  private readonly contacts: ContactMarker[] = [];
  /** Contact markers created since the page loaded (for automation). */
  contactsSeen = 0;
  private readonly intentGroup = new THREE.Group();
  private readonly cities = new Map<string, THREE.Object3D>();
  private readonly policeUnits: { obj: THREE.Group; red: THREE.MeshStandardMaterial; blue: THREE.MeshStandardMaterial }[] = [];
  /** Roadblock barriers and spike strips (police_tools()), pooled; see updatePoliceTools(). */
  private readonly tools = new THREE.Group();
  private readonly toolPool: { barrier: THREE.Group; lamp: THREE.MeshStandardMaterial; spikes: THREE.Group }[] = [];
  private readonly agentMesh = new THREE.InstancedMesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshStandardMaterial({ color: 0xffffff }),
    MAX_AGENTS,
  );

  private debugGrid!: THREE.GridHelper;
  /** Traffic signal heads and stop lines (signals_view.ts). */
  readonly signals = new SignalView();

  constructor(parent: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
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
    const ground = new THREE.Mesh(
      new THREE.PlaneGeometry(4000, 4000),
      new THREE.MeshStandardMaterial({ color: 0x3a3f44 }),
    );
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
    this.agentMesh.count = 0;
    this.agentMesh.frustumCulled = false;
    this.world.add(axes, this.barriers, this.tools, this.intentGroup, this.agentMesh, this.signals.group);
  }

  /**
   * Show the greybox mesh for `cityId` (`data/city/<id>.greybox.glb`, prompt 05), loading it on
   * first use; `null` hides every city. The greybox is only shown when the session loaded that
   * city, so flat-world sessions are not drawn inside buildings.
   */
  async setCity(cityId: string | null): Promise<void> {
    for (const [id, obj] of this.cities) obj.visible = id === cityId;
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
    } catch (e) {
      console.warn("city greybox not available", cityId, e);
    }
  }

  /**
   * Draw police units that are not sim vehicles (police_units(): [x, y, z, yaw, speed, flags],
   * flags 1 helicopter, 2 pursuing). Cars get a flashing light bar while pursuing.
   */
  updatePoliceUnits(units: Float32Array, timeS: number): void {
    const n = units.length / POLICE_UNIT_STRIDE;
    while (this.policeUnits.length < n) {
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
      this.world.add(g);
      this.policeUnits.push({ obj: g, red, blue });
    }
    this.policeUnits.forEach((u, i) => {
      u.obj.visible = i < n;
      if (i >= n) return;
      const o = i * POLICE_UNIT_STRIDE;
      u.obj.position.set(units[o], units[o + 1], units[o + 2]);
      u.obj.rotation.z = units[o + 3];
      const flags = units[o + 5];
      u.obj.scale.setScalar(flags & 1 ? 1.4 : 1); // helicopters: bigger box at altitude
      const chasing = (flags & 2) !== 0;
      const phase = (timeS * 2.5 + i * 0.17) % 1;
      u.red.emissive.setHex(chasing && phase < 0.5 ? 0xff2040 : 0x000000);
      u.blue.emissive.setHex(chasing && phase >= 0.5 ? 0x2060ff : 0x000000);
    });
  }

  /**
   * Draw police tools from police_tools(): roadblock barriers as red/white striped boxes with a
   * flashing amber lamp (their pose follows the physics body, so they visibly move when hit) and
   * spike strips as a dark strip with rows of steel studs, lying on the road.
   */
  updatePoliceTools(tools: Float32Array, timeS: number): void {
    const n = tools.length / TOOL_STRIDE;
    while (this.toolPool.length < n) {
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
      barrier.add(body);
      const lamp = new THREE.MeshStandardMaterial({ color: 0x664400, emissive: 0x000000 });
      const bulb = new THREE.Mesh(new THREE.SphereGeometry(0.09, 10, 8), lamp);
      barrier.add(bulb);

      const spikes = new THREE.Group();
      const strip = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshStandardMaterial({ color: 0x1c1c1e }));
      spikes.add(strip);
      const studs = new THREE.InstancedMesh(
        new THREE.ConeGeometry(0.03, 0.09, 6).rotateX(Math.PI / 2),
        new THREE.MeshStandardMaterial({ color: 0xc8ccd2, metalness: 0.8, roughness: 0.3 }),
        64,
      );
      spikes.add(studs);
      this.tools.add(barrier, spikes);
      this.toolPool.push({ barrier, lamp, spikes });
    }
    const flash = timeS % 1 < 0.5;
    this.toolPool.forEach((t, i) => {
      const on = i < n;
      const o = i * TOOL_STRIDE;
      const kind = on ? tools[o + TL.kind] : -1;
      t.barrier.visible = kind === TOOL_KIND.barrier;
      t.spikes.visible = kind === TOOL_KIND.spikes;
      if (!on) return;
      const obj = kind === TOOL_KIND.barrier ? t.barrier : t.spikes;
      obj.position.set(tools[o + TL.x], tools[o + TL.y], tools[o + TL.z]);
      obj.quaternion.set(tools[o + TL.qx], tools[o + TL.qy], tools[o + TL.qz], tools[o + TL.qw]);
      const hx = tools[o + TL.hx], hy = tools[o + TL.hy], hz = tools[o + TL.hz];
      if (kind === TOOL_KIND.barrier) {
        // The body (with its bands) scales to the barrier; the lamp sits on top at fixed size.
        const [body, bulb] = t.barrier.children;
        body.scale.set(2 * hx, 2 * hy, 2 * hz);
        bulb.position.set(0, 0, hz + 0.09);
        t.lamp.emissive.setHex(flash ? 0xffaa00 : 0x000000);
      } else {
        const [strip, studs] = t.spikes.children as [THREE.Mesh, THREE.InstancedMesh];
        strip.scale.set(2 * hx, 2 * hy, 2 * hz);
        // Two rows of studs along the strip.
        const per = Math.min(32, Math.max(2, Math.floor((2 * hx) / 0.25)));
        const m = new THREE.Matrix4();
        let k = 0;
        for (let row = 0; row < 2; row++) {
          for (let j = 0; j < per; j++) {
            const x = -hx + ((j + 0.5) / per) * 2 * hx;
            m.makeTranslation(x, (row - 0.5) * hy, hz + 0.045);
            studs.setMatrixAt(k++, m);
          }
        }
        studs.count = k;
        studs.instanceMatrix.needsUpdate = true;
      }
    });
  }

  /** Draw civilian traffic from traffic_agents() ([x, y, z, yaw, flags] per agent). */
  updateAgents(agents: Float32Array, skipPhysical = false): void {
    const total = agents.length / AGENT_STRIDE;
    const show = layers.on("traffic");
    this.agentMesh.visible = show && total > 0;
    if (!this.agentMesh.visible) return;
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const zAxis = new THREE.Vector3(0, 0, 1);
    let n = 0;
    for (let a = 0; a < total && n < MAX_AGENTS; a++) {
      const o = a * AGENT_STRIDE;
      if (skipPhysical && agents[o + 4] & AGENT_FLAG.physical) continue;
      const i = n++;
      q.setFromAxisAngle(zAxis, agents[o + 3]);
      m.compose(new THREE.Vector3(agents[o], agents[o + 1], agents[o + 2] + 0.7), q, AGENT_SCALE);
      this.agentMesh.setMatrixAt(i, m);
      const flags = agents[o + 4];
      const c =
        flags & AGENT_FLAG.yielding
          ? 0x3399ff
          : flags & AGENT_FLAG.brake
            ? 0xff5040
            : flags & AGENT_FLAG.physical
              ? 0xffd040
              : 0xb8bcc4;
      this.agentMesh.setColorAt(i, tmpAgentColour.setHex(c));
    }
    this.agentMesh.count = n;
    this.agentMesh.instanceMatrix.needsUpdate = true;
    if (this.agentMesh.instanceColor) this.agentMesh.instanceColor.needsUpdate = true;
  }

  /** Sky + light colour from the night factor (0 noon … 1 midnight). */
  setTimeOfDay(night: number): void {
    (this.scene.background as THREE.Color).copy(DAY_SKY).lerp(NIGHT_SKY, night);
    this.hemi.intensity = 2.0 * (1 - 0.8 * night);
    this.sun.intensity = 1.5 * (1 - night);
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
      const g = new THREE.Group();
      g.position.set(...c.point);
      const dot = new THREE.Mesh(
        new THREE.SphereGeometry(0.08, 8, 6),
        new THREE.MeshBasicMaterial({ color: 0xff00ff, transparent: true, depthTest: false }),
      );
      const len = Math.min(Math.max(c.impulse_ns / 2000, 0.2), 5);
      const arrow = new THREE.ArrowHelper(new THREE.Vector3(...c.normal).normalize(), new THREE.Vector3(), len, 0xff00ff);
      g.add(dot, arrow);
      g.renderOrder = 6;
      this.world.add(g);
      this.contacts.push({ obj: g, age: 0 });
      this.contactsSeen++;
    }
  }

  update(dt: number, intents: Float32Array): void {
    const show = layers.on("contacts");
    for (let i = this.contacts.length - 1; i >= 0; i--) {
      const c = this.contacts[i];
      c.age += dt;
      c.obj.visible = show;
      const fade = 1 - c.age / CONTACT_LIFETIME_S;
      c.obj.traverse((o) => {
        const m = (o as THREE.Mesh).material as THREE.Material | undefined;
        if (m) {
          m.transparent = true;
          m.opacity = Math.max(fade, 0);
        }
      });
      if (fade <= 0) {
        c.obj.removeFromParent();
        this.contacts.splice(i, 1);
      }
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
    for (const c of this.contacts) c.obj.removeFromParent();
    this.contacts.length = 0;
  }

  render(camera: THREE.Camera): void {
    this.renderer.render(this.scene, camera);
  }
}
