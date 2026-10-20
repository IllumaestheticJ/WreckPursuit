// City props: parked cars and cones (lead helper, playtest row 50 "hitting invisible objects").
//
// Every prop is a Rapier body in the sim (fixed until a car comes near, then dynamic), so every
// prop must be drawn wherever the player can reach it. wp_wasm hands us the full list once per
// session (`props_static()`) and, per frame, only the props whose pose changed
// (`props_moved()`, usually empty). Nothing is rebuilt per frame.
//
// Parked cars look like the traffic driving past (the session's traffic model, palette and
// fleet materials), each model's bounding box scaled exactly onto the collider box. There are
// 3,000+ of them, so they are drawn in three tiers, and every car always has exactly one drawn
// instance (no distance hides a prop: no invisible collisions):
//   * full: the nearest MAX_FULL parked cars within FULL_LOD_M (the fleet's full-mesh distance)
//     use the full panel mesh. Re-picked after the camera moves REPICK_M; only the cars that
//     change drawer are rewritten;
//   * proxy / box: the city is cut into CHUNK_M chunks, each with its own instanced meshes and
//     a bounding sphere, so Three frustum-culls whole chunks. A chunk within PROXY_M of the
//     camera shows the fleet's low-poly proxy; farther chunks show a two-box car (24 triangles).
//     Switching a chunk's tier only toggles visibility: both instance sets are always current.
// Cones are one InstancedMesh of small orange cones. A moved prop rewrites only its own
// instances. Coordinates: sim space (+Z up), parented under SceneView.world.
import * as THREE from "three";
import { P, PROP_KIND, PROP_MOVED_STRIDE, PROP_STRIDE } from "../wasm/layout";
import type { CarModelInfo, SessionModels } from "../wasm/sim";
import { sim } from "../wasm/sim";
import { carMeshNow, loadCarMesh } from "./panels";
import { FULL_LOD_M, InstancedCar, TAIL, TRAFFIC_PALETTE, fullGeoms, proxyGeoms, styledPaint, type FleetMaterials } from "./fleet";

/** Full-mesh parked cars at most (nearest first; the rest keep their proxy / box). */
const MAX_FULL = 24;
/** Re-pick the full-mesh set after the camera moved this far, m. */
const REPICK_M = 12;
/** Chunk edge, m. */
const CHUNK_M = 96;
/** Chunks nearer than this (camera to chunk sphere) draw proxies, farther ones boxes, m. */
const PROXY_M = 300;
/** Bounding-sphere slack for knocked props and the car's own extent, m. */
const SPHERE_PAD_M = 6;
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

function fitOf(geos: THREE.BufferGeometry[]): Fit {
  const box = new THREE.Box3();
  for (const g of geos) {
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

/** Far tier: a unit-cube car (lower body + cabin, white vertex colour = instance livery). */
function farCarGeometry(): THREE.BufferGeometry {
  const box = (sx: number, sy: number, sz: number, z: number) => {
    const g = new THREE.BoxGeometry(sx, sy, sz).toNonIndexed();
    g.translate(0, 0, z);
    g.deleteAttribute("uv");
    return g;
  };
  const parts = [box(1, 1, 0.6, -0.2), box(0.86, 0.5, 0.4, 0.3)];
  const n = parts.reduce((s, g) => s + g.getAttribute("position").count * 3, 0);
  const pos = new Float32Array(n);
  let o = 0;
  for (const g of parts) {
    const a = g.getAttribute("position").array as Float32Array;
    pos.set(a, o);
    o += a.length;
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  g.setAttribute("color", new THREE.BufferAttribute(new Float32Array(n).fill(1), 3));
  g.computeVertexNormals();
  return g;
}

const UNIT_FIT: Fit = { centre: new THREE.Vector3(0, 0, 0), size: new THREE.Vector3(1, 1, 1) };

/** One chunk of parked cars: proxy and box instance sets over the same cars. */
interface Chunk {
  props: number[];
  proxy: InstancedCar;
  box: THREE.InstancedMesh;
  sphere: THREE.Sphere;
  near: boolean;
  dirty: boolean;
}

export interface PropStats {
  parked: number;
  cones: number;
  chunks: number;
  /** Parked cars drawn by each tier right now (sum = parked when every car is drawn once). */
  drawnFull: number;
  drawnProxy: number;
  drawnBox: number;
  drawnCones: number;
  /** Props within `rangeM` of the camera, and how many of them have a drawn instance. */
  inRange: number;
  drawnInRange: number;
  /** Parked cars within `rangeM`, and how many of those use the full mesh. */
  parkedInRange: number;
  fullInRange: number;
  /** Parked cars within FULL_LOD_M of the camera, and how many of those use the full mesh. */
  inFullRange: number;
  fullInFullRange: number;
  /** Times the full-mesh set was re-picked, and instance writes by moves / re-picks. */
  repicks: number;
  movedWrites: number;
  repickWrites: number;
  /** Cost of `update()`: mean over frames and the worst frame (a re-pick), ms. */
  updateMeanMs: number;
  updateMaxMs: number;
}

export class PropView {
  /** Full panel-mesh distance, m (graphics quality; lower on phones). */
  fullLodM = FULL_LOD_M;
  /** Most parked cars drawn with the full mesh at once (graphics quality). */
  maxFull = MAX_FULL;
  /** Chunks farther than this are not drawn at all (phones only; Infinity = draw every prop,
   * which the desktop keeps so nothing that can be hit is ever invisible). */
  drawM = Infinity;
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = new THREE.Group();
  private n = 0;
  private kind = new Uint8Array(0);
  /** Current pose per prop: x y z qx qy qz qw. */
  private pose = new Float32Array(0);
  private half = new Float32Array(0);
  /** Per prop: chunk index (parked cars) and slot inside it, or cone instance. */
  private chunkOf = new Int32Array(0);
  private slot = new Int32Array(0);
  /** Per parked car: full-mesh slot, or -1. */
  private fullSlot = new Int32Array(0);
  /** Full-mesh slot → prop. */
  private fullProps: number[] = [];
  private chunks: Chunk[] = [];
  private parkedCount = 0;
  private coneCount = 0;
  private proxyFit: Fit | null = null;
  private full: { car: InstancedCar; fit: Fit } | null = null;
  private cones: THREE.InstancedMesh | null = null;
  private farGeo: THREE.BufferGeometry | null = null;
  private proxyGeo: ReturnType<typeof proxyGeoms> | null = null;
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
  private updates = 0;
  private updateMsSum = 0;
  private updateMaxMs = 0;

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
    this.chunkOf = new Int32Array(n).fill(-1);
    this.slot = new Int32Array(n);
    this.fullSlot = new Int32Array(n).fill(-1);
    const byCell = new Map<string, number[]>();
    let firstCar = -1;
    for (let i = 0, o = 0; i < n; i++, o += PROP_STRIDE) {
      this.kind[i] = list[o + P.kind];
      this.pose.set(list.subarray(o + P.pos, o + P.pos + 7), i * 7);
      this.half.set(list.subarray(o + P.half, o + P.half + 3), i * 3);
      if (this.kind[i] === PROP_KIND.cone) {
        this.slot[i] = this.coneCount++;
        continue;
      }
      this.parkedCount++;
      if (firstCar < 0) firstCar = i;
      const key = `${Math.floor(this.pose[i * 7] / CHUNK_M)},${Math.floor(this.pose[i * 7 + 1] / CHUNK_M)}`;
      let cell = byCell.get(key);
      if (!cell) byCell.set(key, (cell = []));
      cell.push(i);
    }
    if (this.parkedCount > 0) {
      const h = this.half.subarray(firstCar * 3, firstCar * 3 + 3);
      this.trafficModel = models.traffic;
      const info = models.traffic ? sim.carModelInfo(models.traffic) : boxCarInfo([h[0], h[1], h[2]]);
      this.proxyGeo = proxyGeoms(info);
      this.proxyFit = fitOf([this.proxyGeo.paint, this.proxyGeo.detail]);
      this.farGeo = farCarGeometry();
      for (const props of byCell.values()) this.addChunk(props, mats);
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
    for (let i = 0; i < n; i++) this.writeInstance(i);
    this.flush();
    this.lastPick.set(Infinity, Infinity, Infinity);
  }

  private addChunk(props: number[], mats: FleetMaterials): void {
    const c = this.chunks.length;
    // Instances share one geometry set per tier; each chunk's meshes only own their instances.
    const proxy = new InstancedCar(this.proxyGeo!, props.length, mats, null);
    const box = new THREE.InstancedMesh(this.farGeo!, mats.paint, props.length);
    const pts = props.map((i) => new THREE.Vector3(this.pose[i * 7], this.pose[i * 7 + 1], this.pose[i * 7 + 2]));
    const sphere = new THREE.Sphere().setFromPoints(pts);
    sphere.radius += SPHERE_PAD_M;
    for (const mesh of [...proxy.meshes, box]) {
      mesh.frustumCulled = true; // the whole chunk is skipped when its sphere is off screen
      mesh.boundingSphere = sphere;
      mesh.count = props.length;
      this.group.add(mesh);
    }
    proxy.count = props.length;
    props.forEach((i, k) => {
      this.chunkOf[i] = c;
      this.slot[i] = k;
      box.setColorAt(k, this.paint.setHex(styledPaint(parkedPaint(i))));
    });
    const chunk: Chunk = { props, proxy, box, sphere, near: false, dirty: true };
    this.chunks.push(chunk);
    this.showTier(chunk, false);
  }

  private showTier(ch: Chunk, near: boolean): void {
    ch.near = near;
    for (const mesh of ch.proxy.meshes) mesh.visible = near;
    ch.box.visible = !near;
  }

  private setFullMesh(mesh: Parameters<typeof fullGeoms>[0], info: CarModelInfo): void {
    if (this.full || !this.mats) return;
    const g = fullGeoms(mesh, info);
    this.full = { car: new InstancedCar(g, Math.min(this.maxFull, this.parkedCount), this.mats, null), fit: fitOf([g.paint, g.detail]) };
    this.full.car.count = 0;
    this.group.add(...this.full.car.meshes);
    this.lastPick.set(Infinity, Infinity, Infinity); // pick on the next frame
  }

  /** Matrix of prop `i` for a model with bounding box `fit` (model box → collider box). */
  private carMatrix(i: number, fit: Fit): THREE.Matrix4 {
    const o = i * 7;
    this.s.set((2 * this.half[i * 3]) / fit.size.x, (2 * this.half[i * 3 + 1]) / fit.size.y, (2 * this.half[i * 3 + 2]) / fit.size.z);
    // local = T(−centre ∘ scale) · S(scale): the model's box centre lands on the prop's centre.
    this.p.copy(fit.centre).multiply(this.s).negate();
    this.local.compose(this.p, this.q.identity(), this.s);
    this.p.set(this.pose[o], this.pose[o + 1], this.pose[o + 2]);
    this.q.set(this.pose[o + 3], this.pose[o + 4], this.pose[o + 5], this.pose[o + 6]);
    return this.m.compose(this.p, this.q, this.one).multiply(this.local);
  }

  /** Write prop `i`'s instances from its current pose and drawer. */
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
    const ch = this.chunks[this.chunkOf[i]];
    if (!ch || !this.proxyFit) return;
    const k = this.slot[i];
    const fs = this.fullSlot[i];
    this.paint.setHex(parkedPaint(i));
    if (fs >= 0 && this.full) {
      this.full.car.put(fs, this.carMatrix(i, this.full.fit), this.paint, this.tail, null);
      this.full.car.end();
      ch.proxy.put(k, ZERO, this.paint, this.tail, null);
      ch.box.setMatrixAt(k, ZERO);
    } else {
      ch.proxy.put(k, this.carMatrix(i, this.proxyFit), this.paint, this.tail, null);
      ch.box.setMatrixAt(k, this.carMatrix(i, UNIT_FIT));
    }
    // A knocked car may leave its chunk's sphere: grow it (culling must never drop it).
    const o = i * 7;
    this.p.set(this.pose[o], this.pose[o + 1], this.pose[o + 2]);
    if (ch.sphere.distanceToPoint(this.p) > -SPHERE_PAD_M) ch.sphere.expandByPoint(this.p).radius += SPHERE_PAD_M;
    ch.dirty = true;
  }

  private flush(): void {
    for (const ch of this.chunks) {
      if (!ch.dirty) continue;
      ch.dirty = false;
      ch.proxy.end();
      ch.box.instanceMatrix.needsUpdate = true;
      if (ch.box.instanceColor) ch.box.instanceColor.needsUpdate = true;
    }
  }

  /** Per frame: apply `props_moved()` records, pick chunk tiers, re-pick the full-mesh set. */
  update(moved: Float32Array, camera: THREE.Camera): void {
    if (this.n === 0) return;
    const t0 = performance.now();
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
    for (const ch of this.chunks) {
      const d = ch.sphere.distanceToPoint(this.cam);
      const near = d < PROXY_M;
      if (near !== ch.near) this.showTier(ch, near);
      if (!near) ch.box.visible = d <= this.drawM;
    }
    if (this.full && this.cam.distanceTo(this.lastPick) > REPICK_M) this.repick();
    this.flush();
    const ms = performance.now() - t0;
    this.updates++;
    this.updateMsSum += ms;
    this.updateMaxMs = Math.max(this.updateMaxMs, ms);
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
      if (d < this.fullLodM) near.push({ i, d });
    }
    near.sort((a, b) => a.d - b.d);
    const want = new Set(near.slice(0, this.maxFull).map((x) => x.i));
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
      this.writeInstance(i); // back to its chunk
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

  /** The mesh, slot and model fit currently drawing prop `i` (null: none). */
  private drawer(i: number): { mesh: THREE.InstancedMesh; slot: number; fit: Fit | null } | null {
    if (i < 0 || i >= this.n) return null;
    if (this.kind[i] === PROP_KIND.cone) return this.cones ? { mesh: this.cones, slot: this.slot[i], fit: null } : null;
    if (this.fullSlot[i] >= 0 && this.full) return { mesh: this.full.car.paint, slot: this.fullSlot[i], fit: this.full.fit };
    const ch = this.chunks[this.chunkOf[i]];
    if (!ch) return null;
    return ch.near ? { mesh: ch.proxy.paint, slot: this.slot[i], fit: this.proxyFit } : { mesh: ch.box, slot: this.slot[i], fit: UNIT_FIT };
  }

  /**
   * The pose prop `i` is drawn with, read back from its instance matrix: box centre (sim space,
   * m) and rotation `[x, y, z, w]`, or null when it has no drawn instance. Automation (e2e).
   */
  drawnPose(i: number): number[] | null {
    const d = this.drawer(i);
    if (!d || d.slot >= d.mesh.count || !d.mesh.visible) return null;
    const m = new THREE.Matrix4();
    d.mesh.getMatrixAt(d.slot, m);
    if (m.determinant() === 0) return null;
    const centre = (d.fit?.centre.clone() ?? new THREE.Vector3()).applyMatrix4(m);
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
    let drawnProxy = 0;
    let drawnBox = 0;
    for (let i = 0; i < this.n; i++) {
      const d = this.distTo(i);
      const car = this.kind[i] !== PROP_KIND.cone;
      const drawn = this.drawnPose(i) !== null;
      if (car && drawn && this.fullSlot[i] < 0) {
        if (this.chunks[this.chunkOf[i]]?.near) drawnProxy++;
        else drawnBox++;
      }
      if (car && d < this.fullLodM) {
        inFullRange++;
        if (this.fullSlot[i] >= 0) fullInFullRange++;
      }
      if (d >= rangeM) continue;
      inRange++;
      if (drawn) drawnInRange++;
      if (car) {
        parkedInRange++;
        if (this.fullSlot[i] >= 0) fullInRange++;
      }
    }
    return {
      parked: this.parkedCount,
      cones: this.coneCount,
      chunks: this.chunks.length,
      drawnFull: this.full?.car.paint.count ?? 0,
      drawnProxy,
      drawnBox,
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
      updateMeanMs: this.updates ? this.updateMsSum / this.updates : 0,
      updateMaxMs: this.updateMaxMs,
    };
  }

  /** The traffic model parked cars are drawn as (null: a box-sized stand-in). */
  get model(): string | null {
    return this.trafficModel;
  }

  /** Drop every instance (new session). */
  clear(): void {
    // Chunks share geometry: remove their meshes, dispose the shared geometry once.
    for (const ch of this.chunks) for (const mesh of [...ch.proxy.meshes, ch.box]) mesh.removeFromParent();
    this.chunks = [];
    if (this.proxyGeo) for (const g of [this.proxyGeo.paint, this.proxyGeo.detail, this.proxyGeo.tail, this.proxyGeo.head]) g.dispose();
    this.proxyGeo = null;
    this.farGeo?.dispose();
    this.farGeo = null;
    this.proxyFit = null;
    this.full?.car.dispose();
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
    this.updates = 0;
    this.updateMsSum = 0;
    this.updateMaxMs = 0;
  }
}
