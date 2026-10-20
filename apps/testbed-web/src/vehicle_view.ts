// Three.js representation of one vehicle and its per-vehicle debug layers. Everything here is
// in sim/chassis coordinates (+Z up, +Y forward, +X right); the scene maps that to Three's
// Y-up by parenting under a rotated `world` group (see scene.ts).
import * as THREE from "three";
import { META_FLAG, S, SW, T, TW } from "./wasm/layout";
import type { VehicleInfo } from "./wasm/sim";
import { sim } from "./wasm/sim";
import { layers } from "./settings";
import { autoHullSkin, SkinnedBody, skinFromBinding } from "./skin";
import { PanelBody } from "./render/panels";

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
  fx: THREE.ArrowHelper;
  fy: THREE.ArrowHelper;
  susp: THREE.Mesh;
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
  private readonly skinned: SkinnedBody | null;
  private readonly wheels: WheelView[] = [];
  private readonly lattice: THREE.LineSegments;
  private readonly latticePos: Float32Array;
  private readonly latticeCol: Float32Array;
  private readonly beams: Uint32Array;
  private readonly stressPoints: THREE.Points;
  private readonly stressCol: Float32Array;
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
  private readonly lamps: THREE.Mesh[] = [];
  /** Wheel children currently shown for the panel mesh (true) or the debug wheel (false). */
  private wheelPartsShown: boolean | null = null;

  constructor(readonly entity: number, colour: number) {
    this.info = sim.info(entity);
    const [w, l, h] = this.info.body_size_m;
    const police = this.info.kind === "Police";

    const bodyMat = new THREE.MeshStandardMaterial({ color: colour, transparent: true, opacity: 1, side: THREE.DoubleSide });
    this.box = new THREE.Mesh(new THREE.BoxGeometry(w, l, h * 0.9), bodyMat);
    this.box.position.z = h * 0.55;
    const nose = new THREE.Mesh(
      new THREE.BoxGeometry(w * 0.6, 0.3, 0.05),
      new THREE.MeshStandardMaterial({ color: 0xffee88 }),
    );
    nose.position.set(0, l / 2 - 0.25, h + 0.03);
    this.nose = nose;
    this.root.add(this.box, nose);

    // Head- and tail-lights (emissive, brighter with headlights on / when braking).
    this.headMat = new THREE.MeshStandardMaterial({ color: 0xfff6d0, emissive: 0x000000 });
    this.tailMat = new THREE.MeshStandardMaterial({ color: 0x550000, emissive: 0x000000 });
    for (const sx of [-1, 1]) {
      const head = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.05, 0.14), this.headMat);
      head.position.set(sx * (w / 2 - 0.25), l / 2 + 0.01, h * 0.45);
      const tail = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.05, 0.12), this.tailMat);
      tail.position.set(sx * (w / 2 - 0.25), -l / 2 - 0.01, h * 0.5);
      this.root.add(head, tail);
      this.lamps.push(head, tail);
    }

    // Police: roof light bar (red left, blue right) + one point light that flashes with the siren.
    if (police) {
      this.lightBar = new THREE.Group();
      this.barRed = new THREE.MeshStandardMaterial({ color: 0x661018, emissive: 0x000000 });
      this.barBlue = new THREE.MeshStandardMaterial({ color: 0x102266, emissive: 0x000000 });
      const red = new THREE.Mesh(new THREE.BoxGeometry(w * 0.32, 0.25, 0.12), this.barRed);
      red.position.x = -w * 0.17;
      const blue = new THREE.Mesh(new THREE.BoxGeometry(w * 0.32, 0.25, 0.12), this.barBlue);
      blue.position.x = w * 0.17;
      this.lightBar.add(red, blue);
      this.lightBar.position.set(0, -0.1, h + 0.08);
      this.root.add(this.lightBar);
    }

    const rest = sim.latticeRest(entity);
    this.nodes = rest.slice();
    const skin =
      skinFromBinding(sim.skinIndices(entity), sim.skinWeights(entity)) ?? autoHullSkin(rest);
    this.skinned = skin ? new SkinnedBody(skin, bodyMat) : null;
    if (this.skinned) {
      this.root.add(this.skinned.object);
      this.skinned.update(rest);
    }

    // Lattice wireframe.
    this.beams = sim.latticeBeams(entity);
    this.latticePos = new Float32Array(this.beams.length * 3);
    this.latticeCol = new Float32Array(this.beams.length * 3).fill(0.6);
    const lg = new THREE.BufferGeometry();
    lg.setAttribute("position", new THREE.BufferAttribute(this.latticePos, 3));
    lg.setAttribute("color", new THREE.BufferAttribute(this.latticeCol, 3));
    this.lattice = new THREE.LineSegments(lg, new THREE.LineBasicMaterial({ vertexColors: true, depthTest: false, transparent: true }));
    this.lattice.renderOrder = 2;
    this.root.add(this.lattice);
    // Nodes stay near the body box (chassis space) even when crushed: a fixed bounding sphere
    // instead of recomputing one for every car every frame.
    const bound = new THREE.Sphere(new THREE.Vector3(0, 0, h / 2), 0.5 * Math.hypot(w, l, h) + 1);
    lg.boundingSphere = bound;

    // Stress heat-map points.
    const sg = new THREE.BufferGeometry();
    sg.setAttribute("position", new THREE.BufferAttribute(this.nodes, 3));
    this.stressCol = new Float32Array(this.nodes.length);
    sg.setAttribute("color", new THREE.BufferAttribute(this.stressCol, 3));
    sg.boundingSphere = bound;
    this.stressPoints = new THREE.Points(sg, new THREE.PointsMaterial({ size: 0.12, vertexColors: true, depthTest: false }));
    this.stressPoints.renderOrder = 3;
    this.root.add(this.stressPoints);

    // Wheels.
    for (const wi of this.info.wheels) {
      const pivot = new THREE.Group();
      pivot.position.set(...wi.position_m);
      const spin = new THREE.Group();
      const material = new THREE.MeshStandardMaterial({ color: 0x222222 });
      const geo = new THREE.CylinderGeometry(wi.radius_m, wi.radius_m, wi.width_m, 20);
      geo.rotateZ(Math.PI / 2); // axle along X
      const tire = new THREE.Mesh(geo, material);
      // Spoke so rotation is visible.
      const spoke = new THREE.Mesh(
        new THREE.BoxGeometry(wi.width_m * 1.05, wi.radius_m * 1.6, 0.06),
        new THREE.MeshStandardMaterial({ color: 0xbbbbbb }),
      );
      spin.add(tire, spoke);
      pivot.add(spin);
      const fx = new THREE.ArrowHelper(new THREE.Vector3(0, 1, 0), new THREE.Vector3(), 1, 0x33ff66);
      const fy = new THREE.ArrowHelper(new THREE.Vector3(1, 0, 0), new THREE.Vector3(), 1, 0xff3366);
      for (const a of [fx, fy]) {
        a.position.z = -wi.radius_m + 0.02;
        pivot.add(a);
      }
      const susp = new THREE.Mesh(
        new THREE.BoxGeometry(0.06, 0.06, 1),
        new THREE.MeshBasicMaterial({ color: 0x66ccff, depthTest: false }),
      );
      susp.renderOrder = 4;
      this.root.add(susp);
      this.root.add(pivot);
      this.wheels.push({ pivot, spin, tire, material, angle: 0, fx, fy, susp });
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
      new THREE.SphereGeometry(0.12, 12, 8),
      new THREE.MeshBasicMaterial({ color: 0xffff00, depthTest: false }),
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
    const needNodes = latticeOn || stressOn || (bodyOn && this.skinned) || panelsOn;
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
    if (panelsOn) {
      const v = tmpV.set(cur[o + S.linVel], cur[o + S.linVel + 1], cur[o + S.linVel + 2]);
      this.panels.update(this.nodes, dt, v);
      this.panels.paint.opacity = latticeOn || stressOn ? 0.35 : 1;
      this.panels.paint.transparent = this.panels.paint.opacity < 1;
      this.panels.paint.depthWrite = !this.panels.paint.transparent;
    }

    if (panelsOn) {
      if (this.skinned) this.skinned.object.visible = false;
      this.box.visible = false;
    } else if (this.skinned) {
      this.skinned.object.visible = bodyOn;
      this.box.visible = false;
      if (bodyOn && rebuild) this.skinned.update(this.nodes);
    } else {
      this.box.visible = bodyOn;
    }
    this.nose.visible = bodyOn && !panelsOn && layers.on("panels");
    const bodyMat = this.box.material as THREE.MeshStandardMaterial;
    bodyMat.opacity = latticeOn || stressOn ? 0.35 : 1;
    bodyMat.depthWrite = bodyMat.opacity === 1;

    this.lattice.visible = latticeOn;
    this.stressPoints.visible = stressOn;
    if (latticeOn || stressOn) {
      // Strain colours change every frame; positions only when the nodes did.
      const strain = stressOn ? sim.latticeStrain(this.entity) : null;
      if (rebuild || strain || this.strainShown) {
        const pos = this.latticePos;
        const col = this.latticeCol;
        for (let b = 0; b < this.beams.length; b++) {
          const n = this.beams[b] * 3;
          pos[b * 3] = this.nodes[n];
          pos[b * 3 + 1] = this.nodes[n + 1];
          pos[b * 3 + 2] = this.nodes[n + 2];
          if (strain) heat(strain[n / 3] / STRAIN_RED).toArray(col, b * 3);
          else col[b * 3] = col[b * 3 + 1] = col[b * 3 + 2] = 0.75;
        }
        this.lattice.geometry.attributes.position.needsUpdate = true;
        this.lattice.geometry.attributes.color.needsUpdate = true;
        this.strainShown = strain !== null;
      }
      if (strain) {
        const pos = this.stressPoints.geometry.attributes.position as THREE.BufferAttribute;
        (pos.array as Float32Array).set(this.nodes);
        pos.needsUpdate = true;
        for (let i = 0; i < strain.length; i++) heat(strain[i] / STRAIN_RED).toArray(this.stressCol, i * 3);
        this.stressPoints.geometry.attributes.color.needsUpdate = true;
      }
    }

    // Wheels.
    const forces = layers.on("tireForces");
    const susp = layers.on("suspension");
    const slip = layers.on("slipColour");
    this.wheels.forEach((w, i) => {
      const wb = o + S.wheelBase + i * S.wheelStride;
      const tb = T.wheelBase + i * T.wheelStride;
      const wi = this.info.wheels[i];
      const detached = cur[wb + SW.detached] > 0.5;
      w.pivot.visible = !detached;
      const compression = cur[wb + SW.compression];
      w.pivot.position.z = wi.position_m[2] + (compression - wi.travel_m * 0.5);
      w.pivot.rotation.z = cur[wb + SW.steer];
      w.angle += cur[wb + SW.omega] * dt;
      w.spin.rotation.x = w.angle;
      if (slip) heat(Math.abs(cur[wb + SW.slipAngle]) / SLIP_RED, w.material.color);
      else w.material.color.setHex(0x222222);

      // Applied tire forces in the steered wheel frame: Fx along +Y, Fy toward the wheel's left (-X).
      // Telemetry is only fetched for other cars while the force arrows are on (`tel` empty).
      for (let k = 0; k < 2; k++) {
        const arrow = k === 0 ? w.fx : w.fy;
        const f = tel.length > 0 ? tel[tb + (k === 0 ? TW.fx : TW.fyLeft)] : 0;
        const len = Math.abs(f) * FORCE_SCALE;
        arrow.visible = forces && len > 0.02;
        if (arrow.visible) {
          arrow.setDirection(tmpDir.copy(k === 0 ? FX_DIR : FY_DIR).multiplyScalar(Math.sign(f) || 1));
          arrow.setLength(len, Math.min(0.3, len * 0.3), Math.min(0.15, len * 0.2));
        }
      }

      w.susp.visible = susp;
      if (susp) {
        const frac = Math.min(Math.max(compression / Math.max(wi.travel_m, 1e-3), 0), 1);
        const hgt = 0.05 + frac * 0.6;
        w.susp.scale.set(1, 1, hgt);
        w.susp.position.set(wi.position_m[0] * 1.25, wi.position_m[1], this.info.body_size_m[2] + 0.2 + hgt / 2);
        heat(frac, (w.susp.material as THREE.MeshBasicMaterial).color);
      }
    });

    // CG + velocity.
    const cgOn = layers.on("cgVelocity");
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

  /** Lights from `vehicle_meta()` flags; `timeS` drives the siren flash pattern. */
  setMeta(flags: number, timeS: number): void {
    const headlights = (flags & META_FLAG.headlights) !== 0;
    this.headMat.emissive.setHex(headlights ? 0xfff2c0 : 0x000000);
    this.tailMat.emissive.setHex(flags & META_FLAG.brake ? 0xff1010 : headlights ? 0x400000 : 0x000000);
    if (!this.lightBar) return;
    const siren = (flags & META_FLAG.siren) !== 0;
    // Alternating red/blue at 2.5 Hz with a double strobe on each side.
    const phase = (timeS * 2.5) % 1;
    const strobe = (phase % 0.5) < 0.12 || ((phase % 0.5) > 0.2 && (phase % 0.5) < 0.32);
    const redOn = siren && phase < 0.5 && strobe;
    const blueOn = siren && phase >= 0.5 && strobe;
    this.barRed!.emissive.setHex(redOn ? 0xff2040 : 0x000000);
    this.barBlue!.emissive.setHex(blueOn ? 0x2060ff : 0x000000);
    this.sirenOn = siren;
    this.sirenColour = redOn ? 0xff2040 : 0x2060ff;
    this.sirenIntensity = redOn || blueOn ? 25 : 0;
  }

  /** Where the siren light sits (sim space, the scene's `world` group): above the roof. */
  sirenPosition(out: THREE.Vector3): THREE.Vector3 {
    return out.set(0, 0, this.info.body_size_m[2] + 0.6).applyQuaternion(this.root.quaternion).add(this.root.position);
  }

  dispose(): void {
    this.panels.dispose();
    this.root.removeFromParent();
    this.velocity.removeFromParent();
  }
}
