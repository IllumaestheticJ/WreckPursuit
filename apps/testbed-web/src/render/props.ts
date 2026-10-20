// City props: parked cars and cones (lead helper, playtest row 50 "hitting invisible objects").
//
// Every prop is a Rapier body in the sim (fixed until a car comes near, then dynamic), so every
// prop must be drawn wherever the player can reach it. wp_wasm hands us the full list once per
// session (`props_static()`) and, per frame, only the props whose pose changed
// (`props_moved()`, usually empty). Nothing is rebuilt per frame:
//   * parked cars are instanced traffic cars (the session's traffic model, palette and fleet
//     materials, so they look like the cars driving past), scaled so the model's bounding box is
//     exactly the collider box. Every parked car always has a low-poly proxy instance; the ones
//     within FULL_LOD_M of the camera (the traffic fleet's full-mesh distance, nearest
//     MAX_FULL first) swap to the full panel mesh. The near set is re-picked only after the
//     camera moved REPICK_M, and only the instances that change drawer are rewritten;
//   * cones are one InstancedMesh of small orange cones;
//   * a moved prop rewrites only its own instance matrix.
// So a prop is never undrawn at any distance: no invisible collisions.
// Coordinates: sim space (+Z up), parented under SceneView.world.
import * as THREE from "three";
import { P, PROP_KIND, PROP_MOVED_STRIDE, PROP_STRIDE } from "../wasm/layout";
import type { CarModelInfo, SessionModels } from "../wasm/sim";
import { sim } from "../wasm/sim";
import { carMeshNow, loadCarMesh } from "./panels";
import { FULL_LOD_M, InstancedCar, TAIL, TRAFFIC_PALETTE, fullGeoms, proxyGeoms, type FleetMaterials } from "./fleet";

/** Full-mesh parked cars at most (nearest first; the rest keep their proxy). */
const MAX_FULL = 256;
/** Re-pick the full-mesh set after the camera moved this far, m. */
const REPICK_M = 12;
/** Cone colour (traffic orange) and its faint glow so cones read at night. */
const CONE_RGB = 0xff6a14;
const CONE_GLOW = 0x2a0e00;

const ZERO = new THREE.Matrix4().makeScale(0, 0, 0);

/** Livery of parked car `i` (stable per prop, spread over the traffic palette). */
export const parkedPaint = (i: number) => TRAFFIC_PALETTE[Math.imul(i + 1, 2654435761) >>> 29];

/** A model's bounding box (chassis space) as centre + size, for fitting it to a collider box. */
interface Fit {
  centre: THREE.Vector3;
  size: THREE.Vector3;
}

function fitOf(geoms: { paint: THREE.BufferGeometry; detail: THREE.BufferGeometry }): Fit {
  const box = new THREE.Box3();
  for (const g of [geoms.paint, geoms.detail]) {
    g.computeBoundingBox();
    if (g.boundingBox) box.union(g.boundingBox);
  }
  return { centre: box.getCenter(new THREE.Vector3()), size: box.getSize(new THREE.Vector3()) };
}

/** Proxy stand-in when the session has no traffic model: a car shape the size of the box. */
function boxCarInfo(half: [number, number, number]): CarModelInfo {
  const [w, l, h] = [half[0] * 2, half[1] * 2, half[2] * 2];
  const r = Math.min(0.33, h * 0.24);
  const wheel = (x: number, y: number) => ({ position_m: [x, y, r] as [number, number, number], radius_m: r, width_m: 0.22, steerable: false, travel_m: 0, tire_id: "" });
  const wx = w / 2 - 0.13;
  const wy = l * 0.33;
  return { car_id: "parked", body_size_m: [w, l, h], wheels: [wheel(-wx, wy), wheel(wx, wy), wheel(-wx, -wy), wheel(wx, -wy)] };
}

/** One set of instanced cars plus how its model fits a collider box. */
interface Bucket {
  car: InstancedCar;
  fit: Fit;
}

export interface PropStats {
  parked: number;
  cones: number;
  /** Instances currently drawn (non-hidden): full + proxy + cones. */
  drawnFull: number;
  drawnProxy: number;
  drawnCones: number;
  /** Props within `rangeM` of the camera, and how many of them have exactly one drawn instance. */
  inRange: number;
  drawnInRange: number;
  /** Parked cars within `rangeM`, and how many of those use the full mesh. */
  parkedInRange: number;
  fullInRange: number;
  /** Parked cars within FULL_LOD_M of the camera, and how many of those use the full mesh. */
  inFullRange: number;
  fullInFullRange: number;
  /** Times the full-mesh set was re-picked, and instance matrices written by moves / re-picks. */
  repicks: number;
  movedWrites: number;
  repickWrites: number;
}

export class PropView {
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = new THREE.Group();
  private n = 0;
  private kind = new Uint8Array(0);
  /** Current pose per prop: x y z qx qy qz qw. */
  private pose = new Float32Array(0);
  private half = new Float32Array(0);
  /** Per prop: proxy instance (parked cars) or cone instance. */
  private slot = new Int32Array(0);
  /** Per parked car: full-mesh slot, or -1. */
  private fullSlot = new Int32Array(0);
  /** Full-mesh slot → prop. */
  private fullProps: number[] = [];
  private parkedCount = 0;
  private coneCount = 0;
  private proxy: Bucket | null = null;
  private full: Bucket | null = null;
  private cones: THREE.InstancedMesh | null = null;
  private mats: FleetMaterials | null = null;
  private trafficModel: string | null = null;
  private generation = 0;
  private readonly cam = new THREE.Vector3(Infinity, Infinity, Infinity);
  private readonly lastPick = new THREE.Vector3(Infinity, Infinity, Infinity);
  private readonly m = new THREE.Matrix4();
  private readonly local = new THREE.Matrix4();
  private readonly p = new THREE.Vector3();
  private readonly q = new THREE.Quaternion();
  private readonly s = new THREE.Vector3();
  private readonly one = new THREE.Vector3(1, 1, 1);
  private readonly paint = new THREE.Color();
  private readonly tail = new THREE.Color(TAIL.off);
  private readonly coneMat = new THREE.MeshStandardMaterial({ color: CONE_RGB, emissive: CONE_GLOW, flatShading: true, roughness: 0.7 });
  private repicks = 0;
  private movedWrites = 0;
  private repickWrites = 0;

  /** New session: `list` from `props_static()`, the session's models, the fleet's materials. */
  set(list: Float32Array, models: SessionModels, mats: FleetMaterials): void {
    this.clear();
    this.generation++;
    this.mats = mats;
    const n = list.length / PROP_STRIDE;
    this.n = n;
    this.kind = new Uint8Array(n);
    this.pose = new Float32Array(n * 7);
    this.half = new Float32Array(n * 3);
    this.slot = new Int32Array(n);
    this.fullSlot = new Int32Array(n).fill(-1);
    let firstCar = -1;
    for (let i = 0, o = 0; i < n; i++, o += PROP_STRIDE) {
      this.kind[i] = list[o + P.kind];
      this.pose.set(list.subarray(o + P.pos, o + P.pos + 7), i * 7);
      this.half.set(list.subarray(o + P.half, o + P.half + 3), i * 3);
      if (this.kind[i] === PROP_KIND.cone) this.slot[i] = this.coneCount++;
      else {
        this.slot[i] = this.parkedCount++;
        if (firstCar < 0) firstCar = i;
      }
    }
    if (this.parkedCount > 0) {
      const h = this.half.subarray(firstCar * 3, firstCar * 3 + 3);
      this.trafficModel = models.traffic;
      const info = models.traffic ? sim.carModelInfo(models.traffic) : boxCarInfo([h[0], h[1], h[2]]);
      const g = proxyGeoms(info);
      this.proxy = { car: new InstancedCar(g, this.parkedCount, mats, null), fit: fitOf(g) };
      this.group.add(...this.proxy.car.meshes);
      if (models.traffic) {
        const carId = models.traffic;
        const now = carMeshNow(carId);
        if (now) this.setFullMesh(now, info);
        else if (now === undefined) {
          const gen = this.generation;
          void loadCarMesh(carId).then((mesh) => mesh && gen === this.generation && this.setFullMesh(mesh, info));
        }
      }
    }
    if (this.coneCount > 0) {
      const geo = new THREE.ConeGeometry(1, 1, 8, 1);
      geo.rotateX(Math.PI / 2); // Three cones point +Y; sim up is +Z
      this.cones = new THREE.InstancedMesh(geo, this.coneMat, this.coneCount);
      this.cones.frustumCulled = false;
      this.group.add(this.cones);
    }
    // Everything starts as a proxy / cone at its static pose.
    for (let i = 0; i < n; i++) this.writeInstance(i);
    this.flush();
    this.lastPick.set(Infinity, Infinity, Infinity);
  }

  private setFullMesh(mesh: Parameters<typeof fullGeoms>[0], info: CarModelInfo): void {
    if (this.full || !this.mats) return;
    const g = fullGeoms(mesh, info);
    this.full = { car: new InstancedCar(g, Math.min(MAX_FULL, this.parkedCount), this.mats, null), fit: fitOf(g) };
    this.full.car.count = 0;
    this.group.add(...this.full.car.meshes);
    this.lastPick.set(Infinity, Infinity, Infinity); // pick on the next frame
  }

  /** Matrix of prop `i` drawn by `b` (model bounding box → collider box). */
  private carMatrix(i: number, b: Bucket): THREE.Matrix4 {
    const o = i * 7;
    const hx = this.half[i * 3];
    const hy = this.half[i * 3 + 1];
    const hz = this.half[i * 3 + 2];
    this.s.set((2 * hx) / b.fit.size.x, (2 * hy) / b.fit.size.y, (2 * hz) / b.fit.size.z);
    // local = T(−centre ∘ scale) · S(scale): the model's box centre lands on the prop's centre.
    this.p.copy(b.fit.centre).multiply(this.s).negate();
    this.local.compose(this.p, this.q.identity(), this.s);
    this.p.set(this.pose[o], this.pose[o + 1], this.pose[o + 2]);
    this.q.set(this.pose[o + 3], this.pose[o + 4], this.pose[o + 5], this.pose[o + 6]);
    return this.m.compose(this.p, this.q, this.one).multiply(this.local);
  }

  /** Write prop `i`'s instance(s) from its current pose and drawer. */
  private writeInstance(i: number): void {
    if (this.kind[i] === PROP_KIND.cone) {
      if (!this.cones) return;
      const o = i * 7;
      this.p.set(this.pose[o], this.pose[o + 1], this.pose[o + 2]);
      this.q.set(this.pose[o + 3], this.pose[o + 4], this.pose[o + 5], this.pose[o + 6]);
      // Unit cone (radius 1, height 1) → the cone's collider box.
      this.s.set(this.half[i * 3], this.half[i * 3 + 1], this.half[i * 3 + 2] * 2);
      this.cones.setMatrixAt(this.slot[i], this.m.compose(this.p, this.q, this.s));
      this.cones.instanceMatrix.needsUpdate = true;
      return;
    }
    const proxy = this.proxy;
    if (!proxy) return;
    const fs = this.fullSlot[i];
    this.paint.setHex(parkedPaint(i));
    if (fs >= 0 && this.full) {
      this.full.car.put(fs, this.carMatrix(i, this.full), this.paint, this.tail, null);
      this.full.car.end();
      proxy.car.put(this.slot[i], ZERO, this.paint, this.tail, null);
    } else {
      proxy.car.put(this.slot[i], this.carMatrix(i, proxy), this.paint, this.tail, null);
    }
    this.dirtyProxy = true;
  }

  private dirtyProxy = false;

  private flush(): void {
    if (this.proxy && this.dirtyProxy) {
      this.proxy.car.count = this.parkedCount;
      this.proxy.car.end();
    }
    this.dirtyProxy = false;
  }

  /** Per frame: apply `props_moved()` records, then re-pick the full-mesh set if the camera moved. */
  update(moved: Float32Array, camera: THREE.Camera): void {
    if (this.n === 0) return;
    // Three (x, y, z) → sim (x, -z, y): `world` is rotated -90° about X.
    const c = camera.getWorldPosition(this.p);
    this.cam.set(c.x, -c.z, c.y);
    for (let o = 0; o < moved.length; o += PROP_MOVED_STRIDE) {
      const i = moved[o];
      if (i >= this.n) continue;
      this.pose.set(moved.subarray(o + 1, o + 8), i * 7);
      this.writeInstance(i);
      this.movedWrites++;
    }
    if (this.full && this.cam.distanceTo(this.lastPick) > REPICK_M) this.repick();
    this.flush();
  }

  /** Re-pick the nearest parked cars for the full mesh; rewrite only the ones that change drawer. */
  private repick(): void {
    const full = this.full;
    if (!full) return;
    this.lastPick.copy(this.cam);
    this.repicks++;
    const near: { i: number; d: number }[] = [];
    for (let i = 0; i < this.n; i++) {
      if (this.kind[i] === PROP_KIND.cone) continue;
      const d = this.distTo(i);
      if (d < FULL_LOD_M) near.push({ i, d });
    }
    near.sort((a, b) => a.d - b.d);
    const want = new Set(near.slice(0, MAX_FULL).map((x) => x.i));
    // Release full slots no longer wanted (swap-remove keeps the slots dense).
    for (let s = this.fullProps.length - 1; s >= 0; s--) {
      const i = this.fullProps[s];
      if (want.has(i)) continue;
      const last = this.fullProps.pop()!;
      this.fullSlot[i] = -1;
      if (last !== i) {
        this.fullProps[s] = last;
        this.fullSlot[last] = s;
        this.writeInstance(last);
        this.repickWrites++;
      }
      this.writeInstance(i); // back to its proxy
      this.repickWrites++;
    }
    for (const i of want) {
      if (this.fullSlot[i] >= 0) continue;
      this.fullSlot[i] = this.fullProps.length;
      this.fullProps.push(i);
      full.car.count = this.fullProps.length;
      this.writeInstance(i);
      this.repickWrites++;
    }
    full.car.count = this.fullProps.length;
    full.car.end();
  }

  private distTo(i: number): number {
    const o = i * 7;
    return Math.hypot(this.pose[o] - this.cam.x, this.pose[o + 1] - this.cam.y, this.pose[o + 2] - this.cam.z);
  }

  /** Number of props of the session (parked cars + cones). */
  get count(): number {
    return this.n;
  }

  /**
   * The pose prop `i` is drawn with, read back from its instance matrix: box centre (sim space,
   * m) and rotation `[x, y, z, w]`, or null when it has no drawn instance. Automation (e2e).
   */
  drawnPose(i: number): number[] | null {
    if (i < 0 || i >= this.n) return null;
    let mesh: THREE.InstancedMesh | null;
    let slot: number;
    let fit: Fit | null = null;
    if (this.kind[i] === PROP_KIND.cone) {
      mesh = this.cones;
      slot = this.slot[i];
    } else if (this.fullSlot[i] >= 0 && this.full) {
      mesh = this.full.car.paint;
      slot = this.fullSlot[i];
      fit = this.full.fit;
    } else {
      mesh = this.proxy?.car.paint ?? null;
      slot = this.slot[i];
      fit = this.proxy?.fit ?? null;
    }
    if (!mesh || slot >= mesh.count) return null;
    const m = new THREE.Matrix4();
    mesh.getMatrixAt(slot, m);
    if (m.determinant() === 0) return null;
    const centre = (fit?.centre.clone() ?? new THREE.Vector3()).applyMatrix4(m);
    const q = new THREE.Quaternion();
    m.decompose(new THREE.Vector3(), q, new THREE.Vector3());
    return [centre.x, centre.y, centre.z, q.x, q.y, q.z, q.w];
  }

  /** Draw-state summary for automation; `rangeM` = the "never invisible" radius. */
  stats(camera: THREE.Camera, rangeM = 150): PropStats {
    const c = camera.getWorldPosition(new THREE.Vector3());
    this.cam.set(c.x, -c.z, c.y);
    let inRange = 0;
    let drawnInRange = 0;
    let inFullRange = 0;
    let fullInFullRange = 0;
    let parkedInRange = 0;
    let fullInRange = 0;
    for (let i = 0; i < this.n; i++) {
      const d = this.distTo(i);
      const car = this.kind[i] !== PROP_KIND.cone;
      if (car && d < FULL_LOD_M) {
        inFullRange++;
        if (this.fullSlot[i] >= 0) fullInFullRange++;
      }
      if (d >= rangeM) continue;
      inRange++;
      if (car) {
        parkedInRange++;
        if (this.fullSlot[i] >= 0) fullInRange++;
      }
      if (this.drawnPose(i) !== null) drawnInRange++;
    }
    let hidden = 0;
    if (this.proxy) {
      const m = new THREE.Matrix4();
      for (let s = 0; s < this.proxy.car.paint.count; s++) {
        this.proxy.car.paint.getMatrixAt(s, m);
        if (m.determinant() === 0) hidden++;
      }
    }
    return {
      parked: this.parkedCount,
      cones: this.coneCount,
      drawnFull: this.full?.car.paint.count ?? 0,
      drawnProxy: (this.proxy?.car.paint.count ?? 0) - hidden,
      drawnCones: this.cones?.count ?? 0,
      inRange,
      drawnInRange,
      parkedInRange,
      fullInRange,
      inFullRange,
      fullInFullRange,
      repicks: this.repicks,
      movedWrites: this.movedWrites,
      repickWrites: this.repickWrites,
    };
  }

  /** The traffic model parked cars are drawn as (null: a box-sized stand-in). */
  get model(): string | null {
    return this.trafficModel;
  }

  /** Drop every instance (new session). */
  clear(): void {
    for (const b of [this.proxy, this.full]) b?.car.dispose();
    this.proxy = null;
    this.full = null;
    if (this.cones) {
      this.cones.removeFromParent();
      this.cones.geometry.dispose();
      this.cones = null;
    }
    this.n = 0;
    this.parkedCount = 0;
    this.coneCount = 0;
    this.fullProps = [];
    this.trafficModel = null;
    this.repicks = 0;
    this.movedWrites = 0;
    this.repickWrites = 0;
  }
}
