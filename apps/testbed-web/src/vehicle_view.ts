// Three.js representation of one vehicle and its per-vehicle debug layers. Everything here is
// in sim/chassis coordinates (+Z up, +Y forward, +X right); the scene maps that to Three's
// Y-up by parenting under a rotated `world` group (see scene.ts).
import * as THREE from "three";
import { META_FLAG, S, SW, T, TW, wheelCentreZ } from "./wasm/layout";
import type { VehicleInfo } from "./wasm/sim";
import { sim } from "./wasm/sim";
import { layers } from "./settings";
import { autoHullSkin, SkinnedBody, skinFromBinding } from "./skin";
import { PanelBody } from "./render/panels";
import { artStyle, makeHeadlightPool, vehiclePaint } from "./render/style";
import { PALETTE, sirenLightIntensity } from "./render/palette";
import { proxyGeoms, styledPaint } from "./render/fleet";

/** Newtons per metre of tire-force arrow. */
const FORCE_SCALE = 1 / 4000;
/** Slip angle (rad) shown as fully red. */
const SLIP_RED = 0.15;
/** Strain shown as fully red in the heat-map. */
const STRAIN_RED = 0.05;

const tmpColor = new THREE.Color();
// Per-frame scratch (VehicleView.update runs for every car every frame: no allocation).
const tmpQa = new THREE.Quaternion();
const tmpQb = new THREE.Quaternion();
const tmpV = new THREE.Vector3();
const tmpDir = new THREE.Vector3();
const FX_DIR = new THREE.Vector3(0, 1, 0);
const FY_DIR = new THREE.Vector3(-1, 0, 0);

/**
 * Geometry and material shared by every view (browser perf, round 12b: traffic and police are
 * promoted to full views about once a second in a chase, and each one built ~25 geometries and
 * ~45 materials, ~1 MB of garbage and a shader-program lookup per material). Geometries are
 * keyed by their dimensions; views never dispose them.
 */
const sharedGeo = new Map<string, THREE.BufferGeometry>();
function geo(key: string, make: () => THREE.BufferGeometry): THREE.BufferGeometry {
  let g = sharedGeo.get(key);
  if (!g) sharedGeo.set(key, (g = make()));
  return g;
}
const box = (x: number, y: number, z: number) => geo(`box ${x} ${y} ${z}`, () => new THREE.BoxGeometry(x, y, z));
/** Materials no view changes per car (debug markers, nose, spokes, lattice, proxy detail). */
const SHARED = {
  nose: new THREE.MeshStandardMaterial({ color: 0xffee88 }),
  spoke: new THREE.MeshStandardMaterial({ color: 0xbbbbbb }),
  cg: new THREE.MeshBasicMaterial({ color: 0xffff00, depthTest: false }),
  lattice: new THREE.LineBasicMaterial({ vertexColors: true, depthTest: false, transparent: true }),
  stress: new THREE.PointsMaterial({ size: 0.12, vertexColors: true, depthTest: false }),
  proxyDetail: new THREE.MeshStandardMaterial({ vertexColors: true, flatShading: true, roughness: 0.6 }),
};
/** Proxy stand-in geometry per car model and size. */
const proxyCache = new Map<string, ReturnType<typeof proxyGeoms>>();

/** Blue→green→yellow→red ramp for t in 0..1. */
export function heat(t: number, out = tmpColor): THREE.Color {
  const x = Math.min(Math.max(t, 0), 1);
  return out.setHSL((1 - x) * 0.66, 0.9, 0.5);
}

/** Green (1 = healthy) → red (0 = destroyed). */
export function healthColour(h: number, out = tmpColor): THREE.Color {
  return out.setHSL(Math.min(Math.max(h, 0), 1) * 0.33, 0.85, 0.45);
}

interface WheelView {
  pivot: THREE.Group; // positioned at the hub, rotated by steer
  spin: THREE.Group; // rotated about X by the accumulated wheel angle
  tire: THREE.Mesh;
  material: THREE.MeshStandardMaterial;
  angle: number;
  /** Debug overlays, created the first time their layer shows on this car. */
  fx: THREE.ArrowHelper | null;
  fy: THREE.ArrowHelper | null;
  susp: THREE.Mesh | null;
}

/** Lattice wireframe + stress heat-map points of one car (debug layers). */
interface LatticeView {
  lattice: THREE.LineSegments;
  pos: Float32Array;
  col: Float32Array;
  beams: Uint32Array;
  points: THREE.Points;
  stressCol: Float32Array;
}

/** Element-wise equality of two node arrays (cheaper than rebuilding geometry). */
function sameValues(a: Float32Array, b: Float32Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

export class VehicleView {
  readonly root = new THREE.Group();
  readonly info: VehicleInfo;
  private readonly box: THREE.Mesh;
  private readonly nose: THREE.Mesh;
  /** Skinned debug body (shown when the panel mesh is off); built on first use. */
  private skinned: SkinnedBody | null = null;
  private skinTried = false;
  private readonly rest: Float32Array;
  private readonly wheels: WheelView[] = [];
  /** Lattice wireframe and stress points: built the first time those layers are on. */
  private latticeView: LatticeView | null = null;
  private readonly cg: THREE.Mesh;
  readonly velocity: THREE.ArrowHelper;
  /** Latest lattice nodes (chassis space). */
  nodes: Float32Array;
  /** Lattice wireframe / skin need rebuilding (nodes changed or a layer was switched on). */
  private shapeDirty = true;
  /** The wireframe currently carries strain colours (repaint grey when stress turns off). */
  private strainShown = false;
  private readonly lightBar: THREE.Group | null = null;
  private readonly barRed: THREE.MeshStandardMaterial | null = null;
  private readonly barBlue: THREE.MeshStandardMaterial | null = null;
  /** Siren state from the last `setMeta` (police only). The scene owns the one siren point
   * light (SceneView.setSiren) and puts it on the nearest car with `sirenOn`. */
  sirenOn = false;
  sirenColour = 0x2060ff;
  sirenIntensity = 0;
  private readonly headMat: THREE.MeshStandardMaterial;
  private readonly tailMat: THREE.MeshStandardMaterial;
  /** Per-panel car mesh (workstream R, render/panels.ts); replaces box, lamps and tires once loaded. */
  readonly panels: PanelBody;
  /** Sim-spawned traffic / police car: per-car debug overlays (tire forces, slip colours,
   * suspension bars, CG + velocity) stay off unless it is the focused car, so a promotion does
   * not sprout debug arrows (playtest row 25). */
  readonly background: boolean;
  /** The camera's focus car (set by main.ts every frame). */
  focused = false;
  /** Far enough that the instanced fleet draws this car instead (render/lod.ts; the root is
   * hidden and the lattice / panel re-skin is skipped until it comes back). */
  lodFar = false;
  private readonly lamps: THREE.Mesh[] = [];
  /** Greybox paint (the colour the view was created with). */
  private readonly greyPaint: THREE.Color;
  /** Art-style version last applied (render/style.ts `artStyle`). */
  private styleVersion = -1;
  /** Headlight pool on the road ahead (low-poly dark at night). */
  private readonly headPool: THREE.Mesh;
  /** Wheel children currently shown for the panel mesh (true) or the debug wheel (false). */
  private wheelPartsShown: boolean | null = null;
  /** Low-poly stand-in (render/fleet.ts proxy) shown while the panel mesh is still loading. */
  private readonly proxy: THREE.Group;
  /** The proxy's paint material (recoloured by the art style). */
  private readonly proxyPaint: THREE.MeshStandardMaterial;

  constructor(readonly entity: number, colour: number) {
    this.info = sim.info(entity);
    const [w, l, h] = this.info.body_size_m;
    const police = this.info.kind === "Police";
    this.greyPaint = new THREE.Color(colour);
    this.background = police || this.info.kind === "Traffic";

    // forceSinglePass: transparent + DoubleSide would re-resolve the program twice per frame.
    const bodyMat = new THREE.MeshStandardMaterial({ color: colour, transparent: true, opacity: 1, side: THREE.DoubleSide, forceSinglePass: true });
    this.box = new THREE.Mesh(box(w, l, h * 0.9), bodyMat);
    this.box.position.z = h * 0.55;
    const nose = new THREE.Mesh(box(w * 0.6, 0.3, 0.05), SHARED.nose);
    nose.position.set(0, l / 2 - 0.25, h + 0.03);
    this.nose = nose;
    this.root.add(this.box, nose);

    // Head- and tail-lights (emissive, brighter with headlights on / when braking).
    this.headMat = new THREE.MeshStandardMaterial({ color: 0xfff6d0, emissive: 0x000000 });
    this.tailMat = new THREE.MeshStandardMaterial({ color: 0x550000, emissive: 0x000000 });
    for (const sx of [-1, 1]) {
      const head = new THREE.Mesh(box(0.3, 0.05, 0.14), this.headMat);
      head.position.set(sx * (w / 2 - 0.25), l / 2 + 0.01, h * 0.45);
      const tail = new THREE.Mesh(box(0.3, 0.05, 0.12), this.tailMat);
      tail.position.set(sx * (w / 2 - 0.25), -l / 2 - 0.01, h * 0.5);
      this.root.add(head, tail);
      this.lamps.push(head, tail);
    }

    // Police: roof light bar (red left, blue right) + one point light that flashes with the siren.
    if (police) {
      this.lightBar = new THREE.Group();
      this.barRed = new THREE.MeshStandardMaterial({ color: 0x661018, emissive: 0x000000 });
      this.barBlue = new THREE.MeshStandardMaterial({ color: 0x102266, emissive: 0x000000 });
      const red = new THREE.Mesh(box(w * 0.32, 0.25, 0.12), this.barRed);
      red.position.x = -w * 0.17;
      const blue = new THREE.Mesh(box(w * 0.32, 0.25, 0.12), this.barBlue);
      blue.position.x = w * 0.17;
      this.lightBar.add(red, blue);
      this.lightBar.position.set(0, -0.1, h + 0.08);
      this.root.add(this.lightBar);
    }

    this.rest = sim.latticeRest(entity);
    this.nodes = this.rest.slice();

    // Wheels (debug tire + spoke; the force arrows and suspension bar come with their layers).
    for (const wi of this.info.wheels) {
      const pivot = new THREE.Group();
      pivot.position.set(...wi.position_m);
      const spin = new THREE.Group();
      const material = new THREE.MeshStandardMaterial({ color: 0x222222 });
      // Axle along X.
      const cyl = geo(`tire ${wi.radius_m} ${wi.width_m}`, () => new THREE.CylinderGeometry(wi.radius_m, wi.radius_m, wi.width_m, 20).rotateZ(Math.PI / 2));
      const tire = new THREE.Mesh(cyl, material);
      // Spoke so rotation is visible.
      const spoke = new THREE.Mesh(box(wi.width_m * 1.05, wi.radius_m * 1.6, 0.06), SHARED.spoke);
      spin.add(tire, spoke);
      pivot.add(spin);
      this.root.add(pivot);
      this.wheels.push({ pivot, spin, tire, material, angle: 0, fx: null, fy: null, susp: null });
    }

    // Headlight pool: on the road just ahead of the front bumper.
    const wheel0 = this.info.wheels[0];
    this.headPool = makeHeadlightPool(w * 2.6, 18);
    this.headPool.position.set(0, l / 2 - 0.2, wheel0 ? wheel0.position_m[2] - wheel0.radius_m + 0.04 : 0.04);
    this.headPool.visible = false;
    this.root.add(this.headPool);

    // Stand-in until the panel mesh arrives (normally never seen: meshes are preloaded at
    // session start, render/fleet.ts): the far-LOD proxy in this car's livery.
    this.proxy = new THREE.Group();
    {
      const key = `${this.info.car_id} ${this.info.body_size_m.join(" ")}`;
      let g = proxyCache.get(key);
      if (!g) proxyCache.set(key, (g = proxyGeoms({ car_id: this.info.car_id, body_size_m: this.info.body_size_m, wheels: this.info.wheels })));
      this.proxyPaint = new THREE.MeshStandardMaterial({ color: colour, vertexColors: true, flatShading: true, metalness: 0.25, roughness: 0.55 });
      this.proxy.add(new THREE.Mesh(g.paint, this.proxyPaint), new THREE.Mesh(g.detail, SHARED.proxyDetail), new THREE.Mesh(g.tail, this.tailMat));
      this.proxy.visible = false;
      this.root.add(this.proxy);
    }

    // Per-panel mesh: its wheel parts spin with the wheel groups above.
    this.panels = new PanelBody(entity, this.info.car_id, colour, this.headMat, this.tailMat);
    this.root.add(this.panels.root);
    this.panels.whenReady(() => {
      this.wheels.forEach((w, i) => this.panels.wheelParts[i].forEach((m) => w.spin.add(m)));
      this.wheelPartsShown = null;
    });

    // CG marker (+ velocity arrow, which lives in world space; see update()).
    this.cg = new THREE.Mesh(
      geo("cg", () => new THREE.SphereGeometry(0.12, 12, 8)),
      SHARED.cg,
    );
    this.cg.renderOrder = 5;
    this.cg.position.set(...this.info.cg_m);
    this.root.add(this.cg);
    this.velocity = new THREE.ArrowHelper(new THREE.Vector3(0, 1, 0), new THREE.Vector3(), 1, 0xffff00);
  }

  /** Apply interpolated pose + per-frame debug data. `rec`/`prev` are the entity's state
   * records (offset `o` / `po`), `alpha` the interpolation factor, `tel` its telemetry. */
  update(cur: Float32Array, o: number, prev: Float32Array, po: number, alpha: number, tel: Float32Array, dt: number): void {
    const p = S.pos;
    this.root.position.set(
      prev[po + p] + (cur[o + p] - prev[po + p]) * alpha,
      prev[po + p + 1] + (cur[o + p + 1] - prev[po + p + 1]) * alpha,
      prev[po + p + 2] + (cur[o + p + 2] - prev[po + p + 2]) * alpha,
    );
    const qa = tmpQa.set(prev[po + S.rot], prev[po + S.rot + 1], prev[po + S.rot + 2], prev[po + S.rot + 3]);
    const qb = tmpQb.set(cur[o + S.rot], cur[o + S.rot + 1], cur[o + S.rot + 2], cur[o + S.rot + 3]);
    this.root.quaternion.copy(qa.slerp(qb, alpha));

    const bodyOn = layers.on("body");
    // "panel meshes / lattice view" (key 0): panel meshes, or the bare lattice wireframe.
    const panelsOn = bodyOn && layers.on("panels") && this.panels.ready;
    const latticeOn = layers.on("lattice") || (bodyOn && !layers.on("panels"));
    const stressOn = layers.on("stress");
    // Handed to the fleet (far): nothing of the body is drawn, so skip the node fetch and the
    // panel re-skin; they catch up on the frame it comes back.
    const needNodes = !this.lodFar && (latticeOn || stressOn || bodyOn);
    if (needNodes) {
      // Nodes only move when the car is dented: rebuild the wireframe / skin on change only.
      const fresh = sim.latticeNodes(this.entity);
      if (!sameValues(fresh, this.nodes)) {
        this.nodes = fresh;
        this.shapeDirty = true;
      }
    } else {
      // Rebuild once a layer that needs nodes comes back on.
      this.shapeDirty = true;
    }
    const rebuild = this.shapeDirty;
    if (needNodes) this.shapeDirty = false;
    this.panels.root.visible = panelsOn;
    // Panel meshes carry their own head/tail light lenses (shared head/tail materials, so the
    // glow below still works); the box lamps only belong to the debug box body.
    for (const lamp of this.lamps) lamp.visible = bodyOn && !panelsOn;
    if (panelsOn !== this.wheelPartsShown) {
      this.wheelPartsShown = panelsOn;
      this.wheels.forEach((w, i) => {
        const ours = this.panels.wheelParts[i];
        for (const c of w.spin.children) c.visible = ours.includes(c as THREE.Mesh) ? panelsOn : !panelsOn;
      });
    }
    if (panelsOn && !this.lodFar) {
      const v = tmpV.set(cur[o + S.linVel], cur[o + S.linVel + 1], cur[o + S.linVel + 2]);
      this.panels.update(this.nodes, dt, v);
      this.panels.paint.opacity = latticeOn || stressOn ? 0.35 : 1;
      this.panels.paint.transparent = this.panels.paint.opacity < 1;
      this.panels.paint.depthWrite = !this.panels.paint.transparent;
    }

    // Panel meshes wanted but not loaded yet: the proxy, not the debug box / lattice hull.
    const waiting = bodyOn && layers.on("panels") && !this.panels.ready && !latticeOn && !stressOn;
    this.proxy.visible = waiting;
    if (panelsOn || waiting) {
      if (this.skinned) this.skinned.object.visible = false;
      this.box.visible = false;
    } else if (bodyOn ? this.ensureSkin() : this.skinned) {
      const sk = this.skinned!;
      sk.object.visible = bodyOn;
      this.box.visible = false;
      if (bodyOn && rebuild) sk.update(this.nodes);
    } else {
      this.box.visible = bodyOn;
    }
    this.nose.visible = bodyOn && !panelsOn && !waiting && layers.on("panels");
    const bodyMat = this.box.material as THREE.MeshStandardMaterial;
    bodyMat.opacity = latticeOn || stressOn ? 0.35 : 1;
    bodyMat.depthWrite = bodyMat.opacity === 1;

    const lv = latticeOn || stressOn ? this.ensureLattice() : this.latticeView;
    if (lv) {
      lv.lattice.visible = latticeOn;
      lv.points.visible = stressOn;
    }
    if (lv && (latticeOn || stressOn)) {
      // Strain colours change every frame; positions only when the nodes did.
      const strain = stressOn ? sim.latticeStrain(this.entity) : null;
      if (rebuild || strain || this.strainShown) {
        const pos = lv.pos;
        const col = lv.col;
        for (let b = 0; b < lv.beams.length; b++) {
          const n = lv.beams[b] * 3;
          pos[b * 3] = this.nodes[n];
          pos[b * 3 + 1] = this.nodes[n + 1];
          pos[b * 3 + 2] = this.nodes[n + 2];
          if (strain) heat(strain[n / 3] / STRAIN_RED).toArray(col, b * 3);
          else col[b * 3] = col[b * 3 + 1] = col[b * 3 + 2] = 0.75;
        }
        lv.lattice.geometry.attributes.position.needsUpdate = true;
        lv.lattice.geometry.attributes.color.needsUpdate = true;
        this.strainShown = strain !== null;
      }
      if (strain) {
        const pos = lv.points.geometry.attributes.position as THREE.BufferAttribute;
        (pos.array as Float32Array).set(this.nodes);
        pos.needsUpdate = true;
        for (let i = 0; i < strain.length; i++) heat(strain[i] / STRAIN_RED).toArray(lv.stressCol, i * 3);
        lv.points.geometry.attributes.color.needsUpdate = true;
      }
    }

    // Wheels.
    const debug = !this.background || this.focused;
    const forces = debug && layers.on("tireForces");
    const susp = debug && layers.on("suspension");
    const slip = debug && layers.on("slipColour");
    this.wheels.forEach((w, i) => {
      const wb = o + S.wheelBase + i * S.wheelStride;
      const tb = T.wheelBase + i * T.wheelStride;
      const wi = this.info.wheels[i];
      const detached = cur[wb + SW.detached] > 0.5;
      w.pivot.visible = !detached;
      const compression = cur[wb + SW.compression];
      w.pivot.position.z = wheelCentreZ(wi, compression);
      w.pivot.rotation.z = cur[wb + SW.steer];
      w.angle += cur[wb + SW.omega] * dt;
      w.spin.rotation.x = w.angle;
      if (slip) heat(Math.abs(cur[wb + SW.slipAngle]) / SLIP_RED, w.material.color);
      else w.material.color.setHex(0x222222);

      // Applied tire forces in the steered wheel frame: Fx along +Y, Fy toward the wheel's left (-X).
      // Telemetry is only fetched for other cars while the force arrows are on (`tel` empty).
      if (forces && !w.fx) this.addForceArrows(w, wi.radius_m);
      for (let k = 0; k < 2; k++) {
        const arrow = k === 0 ? w.fx : w.fy;
        if (!arrow) continue;
        const f = tel.length > 0 ? tel[tb + (k === 0 ? TW.fx : TW.fyLeft)] : 0;
        const len = Math.abs(f) * FORCE_SCALE;
        arrow.visible = forces && len > 0.02;
        if (arrow.visible) {
          arrow.setDirection(tmpDir.copy(k === 0 ? FX_DIR : FY_DIR).multiplyScalar(Math.sign(f) || 1));
          arrow.setLength(len, Math.min(0.3, len * 0.3), Math.min(0.15, len * 0.2));
        }
      }

      if (susp && !w.susp) {
        w.susp = new THREE.Mesh(box(0.06, 0.06, 1), new THREE.MeshBasicMaterial({ color: 0x66ccff, depthTest: false }));
        w.susp.renderOrder = 4;
        this.root.add(w.susp);
      }
      if (w.susp) w.susp.visible = susp;
      if (susp && w.susp) {
        const frac = Math.min(Math.max(compression / Math.max(wi.travel_m, 1e-3), 0), 1);
        const hgt = 0.05 + frac * 0.6;
        w.susp.scale.set(1, 1, hgt);
        w.susp.position.set(wi.position_m[0] * 1.25, wi.position_m[1], this.info.body_size_m[2] + 0.2 + hgt / 2);
        heat(frac, (w.susp.material as THREE.MeshBasicMaterial).color);
      }
    });

    // CG + velocity.
    const cgOn = debug && layers.on("cgVelocity");
    this.cg.visible = cgOn;
    this.velocity.visible = cgOn;
    if (cgOn) {
      const cgWorld = this.cg.getWorldPosition(new THREE.Vector3());
      const parent = this.velocity.parent;
      if (parent) parent.worldToLocal(cgWorld);
      this.velocity.position.copy(cgWorld);
      const v = new THREE.Vector3(cur[o + S.linVel], cur[o + S.linVel + 1], cur[o + S.linVel + 2]);
      const speed = v.length();
      this.velocity.visible = speed > 0.1;
      if (speed > 0.1) {
        this.velocity.setDirection(v.normalize());
        this.velocity.setLength(Math.min(speed * 0.25, 12), 0.4, 0.25);
      }
    }
  }

  /** Skinned debug body on first use (null when the car has no skin). */
  private ensureSkin(): SkinnedBody | null {
    if (!this.skinTried) {
      this.skinTried = true;
      const skin = skinFromBinding(sim.skinIndices(this.entity), sim.skinWeights(this.entity)) ?? autoHullSkin(this.rest);
      if (skin) {
        this.skinned = new SkinnedBody(skin, this.box.material as THREE.Material);
        this.root.add(this.skinned.object);
        this.skinned.update(this.nodes);
      }
    }
    return this.skinned;
  }

  /** Lattice wireframe + stress points on first use. */
  private ensureLattice(): LatticeView {
    if (this.latticeView) return this.latticeView;
    const [w, l, h] = this.info.body_size_m;
    const beams = sim.latticeBeams(this.entity);
    const pos = new Float32Array(beams.length * 3);
    const col = new Float32Array(beams.length * 3).fill(0.6);
    const lg = new THREE.BufferGeometry();
    lg.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    lg.setAttribute("color", new THREE.BufferAttribute(col, 3));
    const lattice = new THREE.LineSegments(lg, SHARED.lattice);
    lattice.renderOrder = 2;
    // Nodes stay near the body box (chassis space) even when crushed: a fixed bounding sphere
    // instead of recomputing one for every car every frame.
    const bound = new THREE.Sphere(new THREE.Vector3(0, 0, h / 2), 0.5 * Math.hypot(w, l, h) + 1);
    lg.boundingSphere = bound;
    // Stress heat-map points.
    const sg = new THREE.BufferGeometry();
    sg.setAttribute("position", new THREE.BufferAttribute(this.nodes.slice(), 3));
    const stressCol = new Float32Array(this.nodes.length);
    sg.setAttribute("color", new THREE.BufferAttribute(stressCol, 3));
    sg.boundingSphere = bound;
    const points = new THREE.Points(sg, SHARED.stress);
    points.renderOrder = 3;
    this.root.add(lattice, points);
    // The wireframe takes the current nodes on its first update.
    this.shapeDirty = true;
    this.latticeView = { lattice, pos, col, beams, points, stressCol };
    return this.latticeView;
  }

  /** Tire force arrows of one wheel on first use. */
  private addForceArrows(w: WheelView, radius: number): void {
    w.fx = new THREE.ArrowHelper(new THREE.Vector3(0, 1, 0), new THREE.Vector3(), 1, 0x33ff66);
    w.fy = new THREE.ArrowHelper(new THREE.Vector3(1, 0, 0), new THREE.Vector3(), 1, 0xff3366);
    for (const a of [w.fx, w.fy]) {
      a.position.z = -radius + 0.02;
      a.visible = false;
      w.pivot.add(a);
    }
  }

  /**
   * The car can be shown as intended: its panel mesh is loaded (or the panel layer is off).
   * Sim-spawned traffic and police views stay hidden until then and the instanced fleet
   * (render/fleet.ts) draws them instead, so they never pop in as a placeholder.
   */
  get drawable(): boolean {
    return this.panels.ready || !(layers.on("body") && layers.on("panels"));
  }

  /**
   * Flat tyres from tire_punctures() (`p[base + i]`, FL FR RL RR, 0 inflated … 1 flat): the
   * wheel shrinks radially by 30 % × deflation, matching the physics' smaller rolling radius.
   * Scales the spin group, so the panel mesh's wheel parts (parented there) shrink too.
   */
  setPunctures(p: ArrayLike<number>, base: number): void {
    this.wheels.forEach((w, i) => {
      const r = 1 - 0.3 * Math.min(Math.max(p[base + i] ?? 0, 0), 1);
      // Axle along X: the radius lies in the Y/Z plane, so this commutes with the spin about X.
      w.spin.scale.set(1, r, r);
    });
  }

  /** Lights from `vehicle_meta()` flags; `timeS` drives the siren flash pattern. */
  setMeta(flags: number, timeS: number): void {
    if (this.styleVersion !== artStyle.version) this.applyStyle();
    const lowpoly = artStyle.lowpoly;
    const headlights = (flags & META_FLAG.headlights) !== 0;
    const brake = (flags & META_FLAG.brake) !== 0;
    this.headMat.emissive.setHex(headlights ? 0xfff2c0 : 0x000000);
    this.tailMat.emissive.setHex(brake ? 0xff1010 : headlights ? 0x400000 : 0x000000);
    // Low-poly dark: HDR lamp glow so lit lenses reach the bloom pass (running tail lights at
    // a quarter of the brake glow, which is already ×4 in the hex above).
    this.headMat.emissiveIntensity = lowpoly ? PALETTE.headlightGlow : 1;
    this.tailMat.emissiveIntensity = lowpoly ? PALETTE.brakeGlow : 1;
    const pool = lowpoly && headlights && artStyle.night > 0.2;
    this.headPool.visible = pool;
    if (pool) (this.headPool.material as THREE.ShaderMaterial).uniforms.uStrength.value = 0.35 * Math.min(1, (artStyle.night - 0.2) * 2.5);
    if (!this.lightBar) return;
    const siren = (flags & META_FLAG.siren) !== 0;
    // Alternating red/blue at 2.5 Hz with a double strobe on each side.
    const phase = (timeS * 2.5) % 1;
    const strobe = (phase % 0.5) < 0.12 || ((phase % 0.5) > 0.2 && (phase % 0.5) < 0.32);
    const redOn = siren && phase < 0.5 && strobe;
    const blueOn = siren && phase >= 0.5 && strobe;
    this.barRed!.emissive.setHex(redOn ? 0xff2040 : 0x000000);
    this.barBlue!.emissive.setHex(blueOn ? 0x2060ff : 0x000000);
    // The brightest thing in the frame at night (palette.ts policeRedGlow / policeBlueGlow).
    const redGlow = lowpoly ? PALETTE.policeRedGlow : 1;
    const blueGlow = lowpoly ? PALETTE.policeBlueGlow : 1;
    this.barRed!.emissiveIntensity = redGlow;
    this.barBlue!.emissiveIntensity = blueGlow;
    // The scene's one shared siren light (SceneView.setSiren) takes colour and strength from
    // the side that is lit; low-poly dark scales it with the palette glow.
    this.sirenOn = siren;
    this.sirenColour = redOn ? (lowpoly ? PALETTE.policeRed : 0xff2040) : lowpoly ? PALETTE.policeBlue : 0x2060ff;
    this.sirenIntensity = redOn || blueOn ? (lowpoly ? sirenLightIntensity(redOn ? redGlow : blueGlow) : 25) : 0;
  }

  /** Where the siren light sits (sim space, the scene's `world` group): above the roof. */
  sirenPosition(out: THREE.Vector3): THREE.Vector3 {
    return out.set(0, 0, this.info.body_size_m[2] + 0.6).applyQuaternion(this.root.quaternion).add(this.root.position);
  }

  /** Re-colour for the current art style (paint, lamp lenses, light bar, flat shading). */
  private applyStyle(): void {
    this.styleVersion = artStyle.version;
    const lowpoly = artStyle.lowpoly;
    const kind = this.info.kind === "Police" ? "police" : this.info.kind === "Traffic" ? "traffic" : "player";
    // Traffic and police keep the livery the instanced fleet gave them (render/fleet.ts
    // styledPaint), so a promotion swaps the drawer without a recolour.
    const paint = !lowpoly
      ? this.greyPaint
      : new THREE.Color(kind === "player" ? vehiclePaint(kind, this.entity) : styledPaint(this.greyPaint.getHex(), kind === "police"));
    const bodyMat = this.box.material as THREE.MeshStandardMaterial;
    bodyMat.color.copy(paint);
    bodyMat.flatShading = lowpoly;
    bodyMat.needsUpdate = true;
    this.panels.applyStyle(lowpoly, paint);
    this.proxyPaint.color.copy(paint);
    this.proxyPaint.roughness = lowpoly ? 1 : 0.55;
    this.proxyPaint.metalness = lowpoly ? 0 : 0.25;
    this.headMat.color.setHex(lowpoly ? PALETTE.headlight : 0xfff6d0);
    this.tailMat.color.setHex(lowpoly ? 0x5a1414 : 0x550000);
    if (this.barRed && this.barBlue) {
      this.barRed.color.setHex(lowpoly ? 0x5a1418 : 0x661018);
      this.barBlue.color.setHex(lowpoly ? 0x141e5a : 0x102266);
    }
  }

  dispose(): void {
    this.panels.dispose();
    this.root.removeFromParent();
    this.velocity.removeFromParent();
  }
}
