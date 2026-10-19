// Three.js representation of one vehicle and its per-vehicle debug layers. Everything here is
// in sim/chassis coordinates (+Z up, +Y forward, +X right); the scene maps that to Three's
// Y-up by parenting under a rotated `world` group (see scene.ts).
import * as THREE from "three";
import { S, SW, T, TW } from "./wasm/layout";
import type { VehicleInfo } from "./wasm/sim";
import { sim } from "./wasm/sim";
import { layers } from "./settings";
import { autoHullSkin, SkinnedBody, skinFromBinding } from "./skin";

/** Newtons per metre of tire-force arrow. */
const FORCE_SCALE = 1 / 4000;
/** Slip angle (rad) shown as fully red. */
const SLIP_RED = 0.15;
/** Strain shown as fully red in the heat-map. */
const STRAIN_RED = 0.05;

const tmpColor = new THREE.Color();

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

  constructor(readonly entity: number, colour: number) {
    this.info = sim.info(entity);
    const [w, l, h] = this.info.body_size_m;

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

    // Stress heat-map points.
    const sg = new THREE.BufferGeometry();
    sg.setAttribute("position", new THREE.BufferAttribute(this.nodes, 3));
    this.stressCol = new Float32Array(this.nodes.length);
    sg.setAttribute("color", new THREE.BufferAttribute(this.stressCol, 3));
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
    const lerp = (i: number) => prev[po + i] + (cur[o + i] - prev[po + i]) * alpha;
    this.root.position.set(lerp(S.pos), lerp(S.pos + 1), lerp(S.pos + 2));
    const qa = new THREE.Quaternion(prev[po + S.rot], prev[po + S.rot + 1], prev[po + S.rot + 2], prev[po + S.rot + 3]);
    const qb = new THREE.Quaternion(cur[o + S.rot], cur[o + S.rot + 1], cur[o + S.rot + 2], cur[o + S.rot + 3]);
    this.root.quaternion.copy(qa.slerp(qb, alpha));

    const bodyOn = layers.on("body");
    const latticeOn = layers.on("lattice");
    const stressOn = layers.on("stress");
    const needNodes = latticeOn || stressOn || (bodyOn && this.skinned);
    if (needNodes) this.nodes = sim.latticeNodes(this.entity);

    if (this.skinned) {
      this.skinned.object.visible = bodyOn;
      this.box.visible = false;
      if (bodyOn) this.skinned.update(this.nodes);
    } else {
      this.box.visible = bodyOn;
    }
    this.nose.visible = bodyOn;
    const bodyMat = this.box.material as THREE.MeshStandardMaterial;
    bodyMat.opacity = latticeOn || stressOn ? 0.35 : 1;
    bodyMat.depthWrite = bodyMat.opacity === 1;

    this.lattice.visible = latticeOn;
    this.stressPoints.visible = stressOn;
    if (latticeOn || stressOn) {
      const strain = stressOn ? sim.latticeStrain(this.entity) : null;
      for (let b = 0; b < this.beams.length; b++) {
        const n = this.beams[b];
        this.latticePos.set(this.nodes.subarray(n * 3, n * 3 + 3), b * 3);
        if (strain) heat(strain[n] / STRAIN_RED).toArray(this.latticeCol, b * 3);
        else this.latticeCol.fill(0.75, b * 3, b * 3 + 3);
      }
      this.lattice.geometry.attributes.position.needsUpdate = true;
      this.lattice.geometry.attributes.color.needsUpdate = true;
      this.lattice.geometry.computeBoundingSphere();
      if (strain) {
        const pos = this.stressPoints.geometry.attributes.position as THREE.BufferAttribute;
        (pos.array as Float32Array).set(this.nodes);
        pos.needsUpdate = true;
        for (let i = 0; i < strain.length; i++) heat(strain[i] / STRAIN_RED).toArray(this.stressCol, i * 3);
        this.stressPoints.geometry.attributes.color.needsUpdate = true;
        this.stressPoints.geometry.computeBoundingSphere();
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
      const fx = tel[tb + TW.fx];
      const fy = tel[tb + TW.fyLeft];
      for (const [arrow, f, dir] of [
        [w.fx, fx, new THREE.Vector3(0, 1, 0)],
        [w.fy, fy, new THREE.Vector3(-1, 0, 0)],
      ] as const) {
        const len = Math.abs(f) * FORCE_SCALE;
        arrow.visible = forces && len > 0.02;
        if (arrow.visible) {
          arrow.setDirection(dir.clone().multiplyScalar(Math.sign(f) || 1));
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

  dispose(): void {
    this.root.removeFromParent();
    this.velocity.removeFromParent();
  }
}
