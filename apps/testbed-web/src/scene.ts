// Scene: renderer, lights, sky colour from TimeOfDay, ground + 10 m grid, the city greybox
// (data/city/<id>.greybox.glb, prompt 05) and its traffic agents, barriers, fading contact
// markers and intent arrows.
// Sim space is +Z up / +Y forward; Three.js is +Y up, so `world` is rotated -90° about X and
// every sim-space object is parented under it.
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import type { Barrier, SimEvent } from "./wasm/sim";
import { AGENT_FLAG, AGENT_STRIDE, INTENT_STRIDE } from "./wasm/layout";
import { layers } from "./settings";

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
  private readonly agentMesh = new THREE.InstancedMesh(
    new THREE.BoxGeometry(1, 1, 1),
    new THREE.MeshStandardMaterial({ color: 0xffffff }),
    MAX_AGENTS,
  );

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
    this.world.add(ground);
    // 10 m grid (GridHelper lives in Three's XZ plane, i.e. the ground).
    const grid = new THREE.GridHelper(2000, 200, 0xb0b0b0, 0x5c6268);
    grid.position.y = 0.01;
    this.scene.add(grid);
    // Axis hint at the origin: +X red, +Y (forward) green.
    const axes = new THREE.AxesHelper(3);
    axes.position.z = 0.02;
    this.agentMesh.count = 0;
    this.agentMesh.frustumCulled = false;
    this.world.add(axes, this.barriers, this.intentGroup, this.agentMesh);
  }

  /**
   * Show the greybox mesh for `cityId` (`data/city/<id>.greybox.glb`, prompt 05), loading it on
   * first use; `null` hides every city. The greybox is only shown when the session loaded that
   * city, so flat-world sessions are not drawn inside buildings.
   */
  async setCity(cityId: string | null): Promise<void> {
    for (const [id, obj] of this.cities) obj.visible = id === cityId;
    if (!cityId || this.cities.has(cityId)) return;
    const holder = new THREE.Group();
    this.cities.set(cityId, holder);
    this.scene.add(holder);
    try {
      const gltf = await new GLTFLoader().loadAsync(`data/city/${cityId}.greybox.glb`);
      // glTF is +Y up, same as Three: add to the scene, not the Z-up world group.
      holder.add(gltf.scene);
    } catch (e) {
      console.warn("city greybox not available", cityId, e);
    }
  }

  /** Draw civilian traffic from traffic_agents() ([x, y, z, yaw, flags] per agent). */
  updateAgents(agents: Float32Array): void {
    const n = Math.min(agents.length / AGENT_STRIDE, MAX_AGENTS);
    const show = layers.on("traffic");
    this.agentMesh.visible = show && n > 0;
    if (!this.agentMesh.visible) return;
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const zAxis = new THREE.Vector3(0, 0, 1);
    for (let i = 0; i < n; i++) {
      const o = i * AGENT_STRIDE;
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
