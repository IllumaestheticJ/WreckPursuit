// Traffic signals: one mast-arm head over each signalised approach (at the stop line) plus a
// far-side pedestal head across the junction (the stop line is part of the road markings,
// markings_view.ts). Static geometry comes from
// signal_heads() once per session; per-frame states from signal_states() (one byte per head)
// only recolour lamps whose state changed. One InstancedMesh per part keeps it to a handful of
// draw calls for the whole city. Only heads within SIGNAL_DRAW_M of the camera are packed into
// the instanced meshes (browser perf pass: the whole city's lamps were ~110k triangles a frame).
// Everything is placed from the exported road z, so hills work the same as the flat grid.
import * as THREE from "three";
import { SIGNAL, SIGNAL_HEAD_STRIDE, SH } from "./wasm/layout";
import { layers } from "./settings";

/** Near (mast arm) and far (pedestal) head per approach. */
const HEADS_PER_APPROACH = 2;
const NEAR_POLE_H = 6.2;
const FAR_POLE_H = 3.6;
const POLE_CURB_GAP = 0.8;
const LAMP_SPACING = 0.34;
/** Heads farther than this from the camera (m, horizontal) are not drawn (a lamp is < 2 px). */
export const SIGNAL_DRAW_M = 450;
/** Re-pick the near heads when the camera has moved this far (m). */
const RESELECT_M = 20;

const LAMP_COLOURS = [new THREE.Color(1.0, 0.08, 0.04), new THREE.Color(1.0, 0.62, 0.0), new THREE.Color(0.15, 1.0, 0.45)];
/** Lamp index (0 red, 1 amber, 2 green) lit for a state code, or -1. */
const litLamp = (code: number): number =>
  code === SIGNAL.red ? 0 : code === SIGNAL.yellow ? 1 : code === SIGNAL.green ? 2 : -1;

function instanced(geo: THREE.BufferGeometry, mat: THREE.Material, count: number): THREE.InstancedMesh {
  const m = new THREE.InstancedMesh(geo, mat, Math.max(count, 1));
  m.count = count;
  m.frustumCulled = false;
  return m;
}

export class SignalView {
  /** Heads within this distance of the camera are drawn, m (graphics quality: lower on the
   * software / touch profiles). */
  drawM = SIGNAL_DRAW_M;
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = new THREE.Group();
  private parts: THREE.InstancedMesh[] = [];
  private lamps: THREE.InstancedMesh[] = [];
  private glow: THREE.InstancedMesh | null = null;
  private readonly glowMat = new THREE.MeshBasicMaterial({
    transparent: true,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
    toneMapped: false,
  });
  /** Lamp centres, [instance][lamp][xyz] flattened (instance = head * 2 + near/far). */
  private lampPos = new Float32Array(0);
  private shown = new Uint8Array(0);
  private lastNight = -1;
  private lampBoost = 1;
  /** Head count of the current city (0 without one). */
  count = 0;
  /** Every instance matrix of every part as built (full city), for re-packing near heads. */
  private fullPerInstance: Float32Array[] = [];
  private fullPerHead: Float32Array[] = [];
  /** Head stop-line positions (sim x, y). */
  private headXY = new Float32Array(0);
  /** Packed slot of each head (-1 = not drawn) and the heads in slot order. */
  private slotOf = new Int32Array(0);
  private near: number[] = [];
  private readonly lastPick = new THREE.Vector2(Infinity, Infinity);
  private readonly tmpC = new THREE.Color();
  private readonly tmpM = new THREE.Matrix4();
  private readonly zero = new THREE.Matrix4().makeScale(0, 0, 0);
  /** Heads drawn at the last update (automation). */
  get drawnCount(): number {
    return this.near.length;
  }

  /** Rebuild from signal_heads(); an empty array clears the view. */
  setHeads(heads: Float32Array): void {
    for (const m of [...this.parts, ...this.lamps, ...(this.glow ? [this.glow] : [])]) {
      m.geometry.dispose();
      if (m.material !== this.glowMat) (m.material as THREE.Material).dispose();
      m.removeFromParent();
    }
    this.parts = [];
    this.lamps = [];
    this.glow = null;
    const n = heads.length / SIGNAL_HEAD_STRIDE;
    this.count = n;
    this.shown = new Uint8Array(n).fill(255);
    this.lastNight = -1;
    this.near = [];
    this.slotOf = new Int32Array(n).fill(-1);
    this.headXY = new Float32Array(n * 2);
    this.lastPick.set(Infinity, Infinity);
    this.fullPerInstance = [];
    this.fullPerHead = [];
    if (n === 0) return;
    const inst = n * HEADS_PER_APPROACH;

    const metal = new THREE.MeshStandardMaterial({ color: 0x4a4f55, metalness: 0.4, roughness: 0.6 });
    const poleGeo = new THREE.CylinderGeometry(0.11, 0.14, 1, 8).rotateX(Math.PI / 2).translate(0, 0, 0.5);
    const poles = instanced(poleGeo, metal, inst);
    const arms = instanced(new THREE.BoxGeometry(1, 0.16, 0.16), metal.clone(), n);
    const housings = instanced(
      new THREE.BoxGeometry(0.42, 0.3, 1.12),
      new THREE.MeshStandardMaterial({ color: 0x1b1d20, roughness: 0.8 }),
      inst,
    );
    const lampGeo = new THREE.SphereGeometry(0.13, 10, 8);
    this.lamps = [0, 1, 2].map(() => {
      const m = instanced(lampGeo.clone(), new THREE.MeshBasicMaterial({ toneMapped: false }), inst);
      (m.material as THREE.MeshBasicMaterial).color.setScalar(this.lampBoost);
      for (let i = 0; i < inst; i++) m.setColorAt(i, new THREE.Color(0, 0, 0));
      return m;
    });
    this.glow = instanced(new THREE.SphereGeometry(0.42, 12, 8), this.glowMat, inst);
    this.glow.setColorAt(0, new THREE.Color());

    const m4 = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const zAxis = new THREE.Vector3(0, 0, 1);
    const v = new THREE.Vector3();
    const one = new THREE.Vector3(1, 1, 1);
    const s = new THREE.Vector3();
    this.lampPos = new Float32Array(inst * 9);
    for (let h = 0; h < n; h++) {
      const o = h * SIGNAL_HEAD_STRIDE;
      const [x, y, z, yaw] = [heads[o + SH.x], heads[o + SH.y], heads[o + SH.z], heads[o + SH.yaw]];
      this.headXY[h * 2] = x;
      this.headXY[h * 2 + 1] = y;
      const left = heads[o + SH.left];
      const right = heads[o + SH.right];
      const radius = heads[o + SH.radius];
      const cz = heads[o + SH.centreZ];
      // Travel direction f and travel-right r (yaw 0 = +Y, so r = +X).
      const fx = -Math.sin(yaw);
      const fy = Math.cos(yaw);
      const rx = fy;
      const ry = -fx;
      q.setFromAxisAngle(zAxis, yaw);
      const poleR = right + POLE_CURB_GAP;

      // Near side: tall pole at the right curb, arm over the lanes, head at the lane centre.
      m4.compose(v.set(x + rx * poleR, y + ry * poleR, z), q, s.set(1, 1, NEAR_POLE_H));
      poles.setMatrixAt(h * 2, m4);
      m4.compose(v.set(x + (rx * poleR) / 2, y + (ry * poleR) / 2, z + NEAR_POLE_H - 0.2), q, s.set(poleR, 1, 1));
      arms.setMatrixAt(h, m4);
      this.placeHead(housings, h * 2, x, y, z + NEAR_POLE_H - 0.9, fx, fy, q, m4, one);

      // Far side: pedestal across the junction on the same side, facing the same drivers.
      const reach = 2 * radius + 1.5;
      const px = x + fx * reach + rx * poleR;
      const py = y + fy * reach + ry * poleR;
      // Mirror the stop-line height through the junction centre so hills tilt the right way.
      const pz = z + 2 * (cz - z);
      m4.compose(v.set(px, py, pz), q, s.set(1, 1, FAR_POLE_H));
      poles.setMatrixAt(h * 2 + 1, m4);
      this.placeHead(housings, h * 2 + 1, px, py, pz + FAR_POLE_H - 0.45, fx, fy, q, m4, one);
    }
    this.parts = [poles, arms, housings];
    this.group.add(...this.parts, ...this.lamps, this.glow);
    this.fullPerInstance = [poles, housings, ...this.lamps].map((m) => (m.instanceMatrix.array as Float32Array).slice());
    this.fullPerHead = [arms].map((m) => (m.instanceMatrix.array as Float32Array).slice());
    // Nothing is drawn until the first update() picks the heads near the camera.
    for (const m of [...this.parts, ...this.lamps, this.glow]) m.count = 0;
  }

  /** Pack the heads within `radius` m of (x, y) (sim space) into the instanced meshes. */
  private pick(x: number, y: number, radius: number): void {
    this.lastPick.set(x, y);
    const near: number[] = [];
    const r2 = radius * radius;
    for (let h = 0; h < this.count; h++) {
      const dx = this.headXY[h * 2] - x;
      const dy = this.headXY[h * 2 + 1] - y;
      if (dx * dx + dy * dy < r2) near.push(h);
    }
    if (near.length === this.near.length && near.every((h, i) => h === this.near[i])) return;
    this.slotOf.fill(-1);
    near.forEach((h, s) => (this.slotOf[h] = s));
    this.near = near;
    const [poles, arms, housings] = this.parts;
    [poles, housings, ...this.lamps].forEach((m, k) => {
      const src = this.fullPerInstance[k];
      const dst = m.instanceMatrix.array as Float32Array;
      near.forEach((h, s) => dst.set(src.subarray(h * HEADS_PER_APPROACH * 16, (h + 1) * HEADS_PER_APPROACH * 16), s * HEADS_PER_APPROACH * 16));
      m.count = near.length * HEADS_PER_APPROACH;
      m.instanceMatrix.needsUpdate = true;
    });
    [arms].forEach((m, k) => {
      const src = this.fullPerHead[k];
      const dst = m.instanceMatrix.array as Float32Array;
      near.forEach((h, s) => dst.set(src.subarray(h * 16, (h + 1) * 16), s * 16));
      m.count = near.length;
      m.instanceMatrix.needsUpdate = true;
    });
    this.glow!.count = near.length * HEADS_PER_APPROACH;
    // Re-colour every packed head on this update.
    this.shown.fill(255);
  }

  /** Housing matrix plus the three lamp centres, on the face toward approaching drivers (-f). */
  private placeHead(
    housings: THREE.InstancedMesh,
    i: number,
    x: number,
    y: number,
    z: number,
    fx: number,
    fy: number,
    q: THREE.Quaternion,
    m4: THREE.Matrix4,
    one: THREE.Vector3,
  ): void {
    m4.compose(new THREE.Vector3(x, y, z), q, one);
    housings.setMatrixAt(i, m4);
    const m = new THREE.Matrix4();
    for (let l = 0; l < 3; l++) {
      const p = [x - fx * 0.16, y - fy * 0.16, z + (1 - l) * LAMP_SPACING];
      this.lampPos.set(p, i * 9 + l * 3);
      m.makeTranslation(p[0], p[1], p[2]);
      this.lamps[l].setMatrixAt(i, m);
    }
  }

  /**
   * Apply signal_states() (one code byte per head). Only heads whose state changed are
   * recoloured; `night` (0 noon … 1 midnight) dims unlit lamps and strengthens the glow.
   */
  update(states: Uint8Array, night: number, camera?: THREE.Vector3): void {
    this.group.visible = layers.on("signals") && this.count > 0;
    if (!this.group.visible || !this.glow) return;
    // Camera in sim space: Three (x, y, z) = sim (x, z, -y). Without a camera: every head.
    if (!camera) {
      if (this.lastPick.x === Infinity) this.pick(0, 0, Infinity);
    } else if (this.lastPick.distanceTo({ x: camera.x, y: -camera.z }) > RESELECT_M) {
      this.pick(camera.x, -camera.z, this.drawM);
    }
    const nightChanged = Math.abs(night - this.lastNight) > 0.02;
    if (nightChanged) {
      this.lastNight = night;
      this.glowMat.opacity = 0.18 + 0.6 * night;
    }
    const dim = 0.16 * (1 - 0.7 * night);
    const c = this.tmpC;
    const m = this.tmpM;
    const zero = this.zero;
    let dirty = false;
    for (let s = 0; s < this.near.length; s++) {
      const h = this.near[s];
      if (h >= states.length) continue;
      const code = states[h];
      if (code === this.shown[h] && !nightChanged) continue;
      this.shown[h] = code;
      dirty = true;
      const lit = litLamp(code);
      for (let k = 0; k < HEADS_PER_APPROACH; k++) {
        const i = s * HEADS_PER_APPROACH + k;
        const src = h * HEADS_PER_APPROACH + k;
        for (let l = 0; l < 3; l++) {
          c.copy(LAMP_COLOURS[l]);
          if (l !== lit) c.multiplyScalar(dim);
          this.lamps[l].setColorAt(i, c);
        }
        if (lit < 0) {
          this.glow.setMatrixAt(i, zero);
        } else {
          const p = src * 9 + lit * 3;
          m.makeTranslation(this.lampPos[p], this.lampPos[p + 1], this.lampPos[p + 2]);
          this.glow.setMatrixAt(i, m);
          this.glow.setColorAt(i, LAMP_COLOURS[lit]);
        }
      }
    }
    if (!dirty) return;
    for (const l of this.lamps) if (l.instanceColor) l.instanceColor.needsUpdate = true;
    this.glow.instanceMatrix.needsUpdate = true;
    if (this.glow.instanceColor) this.glow.instanceColor.needsUpdate = true;
  }

  /**
   * Lamp brightness multiplier (art style: > 1 so lit lamps reach the bloom pass; 1 = as
   * before). Applies to the lamps of the current and later cities.
   */
  setLampBoost(k: number): void {
    this.lampBoost = k;
    for (const l of this.lamps) (l.material as THREE.MeshBasicMaterial).color.setScalar(k);
  }

  /** Current per-head state codes as last drawn (for automation; 255 = not drawn yet). */
  drawnStates(): number[] {
    return Array.from(this.shown.subarray(0, this.count), (c, h) => (this.slotOf[h] >= 0 ? c : 255));
  }
}
