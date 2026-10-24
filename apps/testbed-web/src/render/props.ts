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
//     use the car model's mid-LOD panel mesh (render/panels.ts loadCarMidMesh, as the fleet).
//     Re-picked after the camera moves REPICK_M; only the cars that change drawer are rewritten;
//   * proxy / box: the city is cut into CHUNK_M chunks, each with its own instanced meshes and
//     a bounding sphere, so Three frustum-culls whole chunks. A chunk within PROXY_M of the
//     camera shows the fleet's low-poly proxy; farther chunks show a two-box car (20 triangles).
//     Switching a chunk's tier only toggles visibility: both instance sets are always current.
// Cones are one InstancedMesh of small orange cones, touge K-rails one InstancedMesh of
// concrete boxes the size of their colliders. A moved prop rewrites only its own
// instances. Coordinates: sim space (+Z up), parented under SceneView.world.
//
// Dented parked cars (owner playtest 2026-10-21, `parked_wrecks()`): a parked car hit hard
// enough gets a damage lattice in the sim. From then on it is drawn like a driven car — its own
// per-panel mesh (render/panels.ts `PanelBody`) skinned from its lattice nodes, broken glass
// hidden — at its prop pose, and its instance is hidden. Only the few hit cars pay for this;
// their nodes are re-read only while the lattice is awake.
import * as THREE from "three";
import { P, PROP_KIND, PROP_MOVED_STRIDE, PROP_STRIDE, WK, WRECK_STRIDE } from "../wasm/layout";
import type { CarModelInfo, SessionModels } from "../wasm/sim";
import { sim } from "../wasm/sim";
import { PanelBody, carMidMeshNow, loadCarMidMesh } from "./panels";
import { FULL_LOD_M, InstancedCar, TAIL, TRAFFIC_PALETTE, fullGeoms, proxyGeoms, styledPaint, type FleetMaterials } from "./fleet";
import { PALETTE } from "./palette";
import { artStyle } from "./style";

/** Full-mesh parked cars at most (nearest first; the rest keep their proxy / box). */
const MAX_FULL = 24;
/** Re-pick the full-mesh set after the camera moved this far, m. */
const REPICK_M = 12;
/** Chunk edge, m. 192 m (was 96): each chunk costs 4 draw calls in the proxy tier and 1 in the
 * box tier, and 96 m chunks were ~190 draw calls a frame in the chase (browser-perf.md 12b). */
const CHUNK_M = 192;
/** Chunks nearer than this (camera to chunk sphere) draw proxies, farther ones boxes, m
 * (default; `PropView.proxyM`, lower in the low graphics profile). */
const PROXY_M = 300;
/** Bounding-sphere slack for knocked props and the car's own extent, m. */
const SPHERE_PAD_M = 6;
/** Cone colour (traffic orange) and its faint glow so cones read at night (greybox; low-poly
 * dark uses PALETTE.propCone / propConeGlow). */
const CONE_RGB = 0xff6a14;
const CONE_GLOW = 0x2a0e00;
/** K-rail concrete (touge passes) and its faint glow so the barrier line reads at night. */
const KRAIL_RGB = 0xb9b4a8;
const KRAIL_GLOW = 0x16140f;

const ZERO = new THREE.Matrix4().makeScale(0, 0, 0);

/** A dented parked car drawn by its own panel mesh. */
interface Wreck {
  prop: number;
  entity: number;
  /** Prop body frame (box centre, prop pose). */
  root: THREE.Group;
  /** Car chassis frame inside it (body = chassis ⊙ scale + origin). */
  chassis: THREE.Group;
  body: PanelBody;
  /** Region names by index (vehicle_info().regions), for shattered glass. */
  regions: string[];
  /** Lattice nodes of the last re-skin (null: not skinned yet). */
  nodes: Float32Array | null;
  /** Shattered glass regions already hidden. */
  shattered: number;
  /** Panel mesh loaded: the instance is hidden and this draws the car. */
  ready: boolean;
}

function sameFloats(a: Float32Array, b: Float32Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

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
    // Without the -Z face (BoxGeometry's last 6 vertices once non-indexed): it sits on the road
    // or on the lower box, never seen. 10 triangles a box instead of 12.
    const full = new THREE.BoxGeometry(sx, sy, sz).toNonIndexed();
    full.translate(0, 0, z);
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute((full.getAttribute("position").array as Float32Array).slice(0, 30 * 3), 3));
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
/** One parked-car model's instanced tiers (proxy and full mid-LOD mesh). */
interface ParkedModel {
  /** Car id (null: a box-sized stand-in for older wasm builds without models). */
  id: string | null;
  proxyGeo: ReturnType<typeof proxyGeoms>;
  proxyFit: Fit;
  /** Full mid-LOD instances, once the mesh has loaded. */
  full: { car: InstancedCar; fit: Fit } | null;
  /** Full-mesh slot → prop, for this model. */
  fullProps: number[];
  /** Parked cars of this model. */
  count: number;
}

interface Chunk {
  /** Model index (`PropView.models`) every prop of the chunk is drawn as. */
  model: number;
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
  /** Most full-mesh parked cars at once (graphics quality). */
  maxFull: number;
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
  /** Dented parked cars, and how many of them are drawn by their own panel mesh. */
  wrecks: number;
  wrecksDrawn: number;
  /** Lattice re-skins of dented parked cars since the session started. */
  wreckReskins: number;
}

/**
 * A new geometry object over `src`'s attributes and index (no copy: same arrays, same GPU
 * buffers), so a mesh gets vertex-array state of its own. Disposing it releases its state and
 * the shared buffers, so views are disposed only together with their source.
 */
function geometryView(src: THREE.BufferGeometry): THREE.BufferGeometry {
  const g = new THREE.BufferGeometry();
  for (const name of Object.keys(src.attributes)) g.setAttribute(name, src.attributes[name]);
  g.setIndex(src.index);
  for (const gr of src.groups) g.addGroup(gr.start, gr.count, gr.materialIndex);
  g.setDrawRange(src.drawRange.start, src.drawRange.count);
  if (src.boundingSphere) g.boundingSphere = src.boundingSphere.clone();
  if (src.boundingBox) g.boundingBox = src.boundingBox.clone();
  return g;
}

/**
 * Parked cars (not cones, not dented ones) within `maxD` of the camera (sim space): their distance into `dist`,
 * their index appended to `near` (`PropView.repick`). A standalone loop over typed arrays:
 * inside the method, each per-prop distance came back from a call as a boxed number
 * (~12 KB a frame at 3–4 fps, allocation pass 2026-10-20).
 */
function nearCars(pose: Float32Array, kind: Uint8Array, wrecked: Uint8Array, n: number, cx: number, cy: number, cz: number, maxD: number, dist: Float32Array, near: number[]): void {
  for (let i = 0; i < n; i++) {
    // Cones, K-rails, and parked cars replaced by their dented vehicle (drawn by its own panel mesh).
    if (kind[i] !== PROP_KIND.parkedCar || wrecked[i]) continue;
    const o = i * 7;
    // Not Math.hypot: it allocates its argument list.
    const dx = pose[o] - cx;
    const dy = pose[o + 1] - cy;
    const dz = pose[o + 2] - cz;
    const d = Math.sqrt(dx * dx + dy * dy + dz * dz);
    if (d < maxD) {
      dist[i] = d;
      near.push(i);
    }
  }
}

export class PropView {
  /** Full panel-mesh distance, m (graphics quality; lower on phones). */
  fullLodM = FULL_LOD_M;
  /** Most parked cars drawn with the full mesh at once (graphics quality). */
  maxFull = MAX_FULL;
  /** Chunks farther than this are not drawn at all (phones only; Infinity = draw every prop,
   * which the desktop keeps so nothing that can be hit is ever invisible). */
  drawM = Infinity;
  /** Chunks nearer than this (m) draw the low-poly proxy, farther ones the two-box car. */
  proxyM = PROXY_M;
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
  /** Per parked car: full-mesh slot (in its model's full set), or -1. */
  private fullSlot = new Int32Array(0);
  /** Per prop: model index into `models` (parked cars only; cars audit F27, a mix of models). */
  private modelOf = new Int16Array(0);
  /** Parked-car models in `session_car_models().parked_models` order. */
  private models: ParkedModel[] = [];
  private chunks: Chunk[] = [];
  private parkedCount = 0;
  private coneCount = 0;
  private krailCount = 0;
  private cones: THREE.InstancedMesh | null = null;
  private krails: THREE.InstancedMesh | null = null;
  private farGeo: THREE.BufferGeometry | null = null;
  private mats: FleetMaterials | null = null;
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
  private readonly krailMat = new THREE.MeshStandardMaterial({ color: KRAIL_RGB, emissive: KRAIL_GLOW, flatShading: true, roughness: 0.95 });
  /** Art-style version the props were built for, and the session inputs to rebuild them. */
  private styleVersion = -1;
  private lastSet: { list: Float32Array; models: SessionModels; mats: FleetMaterials } | null = null;
  private repicks = 0;
  /** Re-pick scratch: candidate props, their distances (by prop) and the wanted flags. */
  private readonly pickIdx: number[] = [];
  private pickDist = new Float32Array(0);
  private pickWant = new Uint8Array(0);
  private readonly byPickDist = (a: number, b: number) => this.pickDist[a] - this.pickDist[b];

  /** Dented parked cars by prop index, and a per-prop flag (instance hidden once drawn). */
  private readonly wrecks = new Map<number, Wreck>();
  private wrecked = new Uint8Array(0);
  private entityBase = 0;
  private wreckReskins = 0;
  // Flat-shaded: some panel-mesh lamp parts are batched without normals (render/panels.ts).
  private readonly wreckHead = new THREE.MeshStandardMaterial({ color: 0x8a8778, flatShading: true, side: THREE.DoubleSide });
  private readonly wreckTail = new THREE.MeshStandardMaterial({ color: 0x4a0c0c, flatShading: true, side: THREE.DoubleSide });

  private movedWrites = 0;
  private repickWrites = 0;
  private updates = 0;
  private updateMsSum = 0;
  private updateMaxMs = 0;

  /** New session: `list` from `props_static()`, the session's models, the fleet's materials. */
  set(list: Float32Array, models: SessionModels, mats: FleetMaterials): void {
    this.clear();
    this.generation++;
    this.lastSet = { list, models, mats };
    this.applyConeStyle();
    this.mats = mats;
    const n = list.length / PROP_STRIDE;
    this.n = n;
    this.kind = new Uint8Array(n);
    this.pose = new Float32Array(n * 7);
    this.half = new Float32Array(n * 3);
    this.chunkOf = new Int32Array(n).fill(-1);
    this.slot = new Int32Array(n);
    this.fullSlot = new Int32Array(n).fill(-1);
    this.modelOf = new Int16Array(n);
    // Per-prop parked-car model (`prop_models()`, -1 for cones / K-rails); older wasm builds
    // have one model for every parked car.
    const ids: (string | null)[] = models.parked_models?.length ? models.parked_models : [models.parked ?? models.traffic ?? null];
    const propModels = sim.propModels();
    this.pickDist = new Float32Array(n);
    this.pickWant = new Uint8Array(n);
    this.wrecked = new Uint8Array(n);
    this.entityBase = n > 0 ? sim.parkedEntityBase() : 0;
    const byCell = new Map<string, number[]>();
    const firstOf = new Map<number, number>();
    for (let i = 0, o = 0; i < n; i++, o += PROP_STRIDE) {
      this.kind[i] = list[o + P.kind];
      this.pose.set(list.subarray(o + P.pos, o + P.pos + 7), i * 7);
      this.half.set(list.subarray(o + P.half, o + P.half + 3), i * 3);
      if (this.kind[i] === PROP_KIND.cone) {
        this.slot[i] = this.coneCount++;
        continue;
      }
      if (this.kind[i] === PROP_KIND.krail) {
        this.slot[i] = this.krailCount++;
        continue;
      }
      this.parkedCount++;
      const m = Math.min(Math.max(Math.round(propModels[i] ?? 0), 0), ids.length - 1);
      this.modelOf[i] = m;
      if (!firstOf.has(m)) firstOf.set(m, i);
      const key = `${m}:${Math.floor(this.pose[i * 7] / CHUNK_M)},${Math.floor(this.pose[i * 7 + 1] / CHUNK_M)}`;
      let cell = byCell.get(key);
      if (!cell) byCell.set(key, (cell = []));
      cell.push(i);
    }
    if (this.parkedCount > 0) {
      // Each parked car is drawn as the model it dents with (`parked_models`).
      this.farGeo = farCarGeometry();
      this.models = ids.map((id, m) => {
        const first = firstOf.get(m) ?? 0;
        const h = this.half.subarray(first * 3, first * 3 + 3);
        const info = id ? sim.carModelInfo(id) : boxCarInfo([h[0], h[1], h[2]]);
        const proxyGeo = proxyGeoms(info);
        return { id, proxyGeo, proxyFit: fitOf([proxyGeo.paint, proxyGeo.detail]), full: null, fullProps: [], count: 0 };
      });
      for (let i = 0; i < n; i++) if (this.kind[i] === PROP_KIND.parkedCar) this.models[this.modelOf[i]].count++;
      for (const props of byCell.values()) this.addChunk(props, mats, this.modelOf[props[0]]);
      this.models.forEach((md, m) => {
        if (!md.id || md.count === 0) return;
        const info = sim.carModelInfo(md.id);
        // The mid LOD (render/panels.ts loadCarMidMesh), as the fleet's near cars.
        const now = carMidMeshNow(md.id);
        if (now) this.setFullMesh(m, now, info);
        else if (now === undefined) {
          const gen = this.generation;
          void loadCarMidMesh(md.id).then((mesh) => mesh && gen === this.generation && this.setFullMesh(m, mesh, info));
        }
      });
    }
    if (this.coneCount > 0) {
      const geo = new THREE.ConeGeometry(1, 1, 8, 1);
      geo.rotateX(Math.PI / 2); // Three cones point +Y; sim up is +Z
      this.cones = new THREE.InstancedMesh(geo, this.coneMat, this.coneCount);
      this.cones.frustumCulled = false;
      this.group.add(this.cones);
    }
    if (this.krailCount > 0) {
      // Unit box with a slight taper towards the top, like a jersey barrier's profile.
      const geo = new THREE.CylinderGeometry(Math.SQRT1_2 * 0.55, Math.SQRT1_2, 1, 4, 1);
      geo.rotateY(Math.PI / 4);
      geo.rotateX(Math.PI / 2); // cylinder axis +Y → sim up +Z
      this.krails = new THREE.InstancedMesh(geo, this.krailMat, this.krailCount);
      this.krails.frustumCulled = false;
      this.group.add(this.krails);
    }
    for (let i = 0; i < n; i++) this.writeInstance(i);
    this.flush();
    this.lastPick.set(Infinity, Infinity, Infinity);
  }

  private addChunk(props: number[], mats: FleetMaterials, model: number): void {
    const c = this.chunks.length;
    // Instances share one geometry set per tier (the same vertex buffers); each chunk's meshes
    // only own their instances. Each mesh still gets its own geometry *object* (`geometryView`):
    // Three keys its vertex-array state by geometry + program, so meshes drawing one geometry
    // object with different instance matrices re-bound every attribute and allocated a new
    // attribute cache on each draw (~15 KB a frame, allocation pass 2026-10-20).
    const pg = this.models[model].proxyGeo;
    const views = { paint: geometryView(pg.paint), detail: geometryView(pg.detail), tail: geometryView(pg.tail), head: geometryView(pg.head), indL: geometryView(pg.indL), indR: geometryView(pg.indR) };
    const proxy = new InstancedCar(views, props.length, mats, null);
    const box = new THREE.InstancedMesh(geometryView(this.farGeo!), mats.paint, props.length);
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
    const chunk: Chunk = { model, props, proxy, box, sphere, near: false, dirty: true };
    this.chunks.push(chunk);
    this.showTier(chunk, false);
  }

  private showTier(ch: Chunk, near: boolean): void {
    ch.near = near;
    for (const mesh of ch.proxy.meshes) mesh.visible = near;
    ch.box.visible = !near;
  }

  private setFullMesh(model: number, mesh: Parameters<typeof fullGeoms>[0], info: CarModelInfo): void {
    const md = this.models[model];
    if (!md || md.full || !this.mats) return;
    const g = fullGeoms(mesh, info);
    md.full = { car: new InstancedCar(g, Math.min(this.maxFull, md.count), this.mats, null), fit: fitOf([g.paint, g.detail]) };
    md.full.car.count = 0;
    this.group.add(...md.full.car.meshes);
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
    if (this.kind[i] === PROP_KIND.krail) {
      if (!this.krails) return;
      const o = i * 7;
      this.p.set(this.pose[o], this.pose[o + 1], this.pose[o + 2]);
      this.q.set(this.pose[o + 3], this.pose[o + 4], this.pose[o + 5], this.pose[o + 6]);
      // Unit box → the K-rail's collider box.
      this.s.set(this.half[i * 3] * 2, this.half[i * 3 + 1] * 2, this.half[i * 3 + 2] * 2);
      this.krails.setMatrixAt(this.slot[i], this.m.compose(this.p, this.q, this.s));
      this.krails.instanceMatrix.needsUpdate = true;
      return;
    }
    const ch = this.chunks[this.chunkOf[i]];
    const md = this.models[this.modelOf[i]];
    if (!ch || !md) return;
    const full = md.full;
    const k = this.slot[i];
    const fs = this.fullSlot[i];
    this.paint.setHex(parkedPaint(i));
    if (this.wrecked[i]) {
      // Drawn by its own dented panel mesh.
      if (fs >= 0 && full) {
        full.car.put(fs, ZERO, this.paint, this.tail, null, false);
        full.car.end();
      }
      ch.proxy.put(k, ZERO, this.paint, this.tail, null, false);
      ch.box.setMatrixAt(k, ZERO);
    } else if (fs >= 0 && full) {
      full.car.put(fs, this.carMatrix(i, full.fit), this.paint, this.tail, null, false);
      full.car.end();
      ch.proxy.put(k, ZERO, this.paint, this.tail, null, false);
      ch.box.setMatrixAt(k, ZERO);
    } else {
      ch.proxy.put(k, this.carMatrix(i, md.proxyFit), this.paint, this.tail, null, false);
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
    if (this.styleVersion !== artStyle.version) this.restyle();
    // Low-poly dark: cones glow a little more as night falls so they read outside lamp light.
    this.coneMat.emissiveIntensity = artStyle.lowpoly ? 1 + PALETTE.propConeNightGlow * artStyle.night : 1;
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
    this.updateWrecks();
    for (const ch of this.chunks) {
      const d = ch.sphere.distanceToPoint(this.cam);
      const near = d < this.proxyM;
      if (near !== ch.near) this.showTier(ch, near);
      if (!near) ch.box.visible = d <= this.drawM;
    }
    if (this.models.some((md) => md.full) && this.cam.distanceTo(this.lastPick) > REPICK_M) this.repick();
    this.flush();
    const ms = performance.now() - t0;
    this.updates++;
    this.updateMsSum += ms;
    this.updateMaxMs = Math.max(this.updateMaxMs, ms);
  }

  /**
   * Dented parked cars: create the panel mesh of a newly dented one, follow each one's prop
   * pose, re-skin from its lattice while the lattice moves, hide shattered glass.
   */
  private updateWrecks(): void {
    const list = sim.parkedWrecks();
    for (let o = 0; o + WRECK_STRIDE <= list.length; o += WRECK_STRIDE) {
      const i = list[o + WK.prop];
      if (i >= this.n || this.kind[i] !== PROP_KIND.parkedCar) continue;
      const w = this.wrecks.get(i) ?? this.addWreck(i, list, o);
      const p = i * 7;
      w.root.position.set(this.pose[p], this.pose[p + 1], this.pose[p + 2]);
      w.root.quaternion.set(this.pose[p + 3], this.pose[p + 4], this.pose[p + 5], this.pose[p + 6]);
      if (!w.ready) continue;
      if (list[o + WK.awake] > 0 || w.nodes === null) {
        const fresh = sim.latticeNodes(w.entity);
        if (!w.nodes || !sameFloats(fresh, w.nodes)) {
          w.nodes = fresh;
          w.body.update(fresh, 0);
          this.wreckReskins++;
        }
      }
      if (list[o + WK.shattered] !== w.shattered) {
        w.shattered = list[o + WK.shattered];
        for (const r of sim.parkedWreckShattered(w.entity)) w.body.shatter(w.regions[r]);
      }
    }
  }

  private addWreck(i: number, list: Float32Array, o: number): Wreck {
    const entity = this.entityBase + i;
    const info = sim.info(entity);
    const root = new THREE.Group();
    const chassis = new THREE.Group();
    chassis.scale.set(list[o + WK.scale], list[o + WK.scale + 1], list[o + WK.scale + 2]);
    chassis.position.set(list[o + WK.origin], list[o + WK.origin + 1], list[o + WK.origin + 2]);
    root.add(chassis);
    root.name = `parked${i}`;
    const paint = new THREE.Color(styledPaint(parkedPaint(i)));
    // keepPaint: the instanced parked car it replaces has this paint (no factory tint).
    const body = new PanelBody(entity, info.car_id, paint, this.wreckHead, this.wreckTail, { keepPaint: true });
    chassis.add(body.root);
    const w: Wreck = { prop: i, entity, root, chassis, body, regions: info.regions, nodes: null, shattered: 0, ready: false };
    this.wrecks.set(i, w);
    this.group.add(root);
    const gen = this.generation;
    body.whenReady(() => {
      if (gen !== this.generation || this.wrecks.get(i) !== w) return;
      body.applyStyle(artStyle.lowpoly, paint);
      // Wheels stand at their static ride height (a parked car does not roll its wheels).
      info.wheels.forEach((wi, k) => {
        const hub = new THREE.Group();
        hub.position.set(wi.position_m[0], wi.position_m[1], wi.position_m[2]);
        for (const m of body.wheelParts[k] ?? []) hub.add(m);
        for (const m of body.hubParts[k] ?? []) hub.add(m);
        chassis.add(hub);
      });
      w.ready = true;
      this.wrecked[i] = 1;
      this.writeInstance(i);
      this.flush();
    });
    return w;
  }

  /** Cone material for the current art style (flat-shaded either way). */
  private applyConeStyle(): void {
    this.styleVersion = artStyle.version;
    const low = artStyle.lowpoly;
    this.coneMat.color.setHex(low ? PALETTE.propCone : CONE_RGB);
    this.coneMat.emissive.setHex(low ? PALETTE.propConeGlow : CONE_GLOW);
    this.coneMat.roughness = low ? 1 : 0.7;
  }

  /**
   * The art style changed mid-session: parked-car colours are baked into instance colours and
   * the shared proxy / full geometry (detail vertex colours), so rebuild from the session's
   * list and put every prop back at its current (possibly knocked) pose. A debug switch, not a
   * per-frame cost.
   */
  private restyle(): void {
    const last = this.lastSet;
    if (!last) return this.applyConeStyle();
    const pose = this.pose.slice();
    this.set(last.list, last.models, last.mats);
    if (pose.length !== this.pose.length) return;
    this.pose.set(pose);
    for (let i = 0; i < this.n; i++) this.writeInstance(i);
    this.flush();
  }

  /** Re-pick the nearest parked cars for the full mesh; rewrite only the ones that change drawer. */
  private repick(): void {
    if (!this.models.some((md) => md.full)) return;
    this.lastPick.copy(this.cam);
    this.repicks++;
    // Candidates within the full-mesh distance, nearest first (scratch arrays, no garbage).
    const near = this.pickIdx;
    const dist = this.pickDist;
    near.length = 0;
    nearCars(this.pose, this.kind, this.wrecked, this.n, this.cam.x, this.cam.y, this.cam.z, this.fullLodM, dist, near);
    near.sort(this.byPickDist);
    if (near.length > this.maxFull) near.length = this.maxFull;
    const want = this.pickWant;
    for (const i of near) want[i] = 1;
    // Release full slots no longer wanted (swap-remove keeps each model's slots dense).
    for (const md of this.models) {
      const fp = md.fullProps;
      for (let s = fp.length - 1; s >= 0; s--) {
        const i = fp[s];
        if (want[i]) continue;
        const last = fp.pop()!;
        this.fullSlot[i] = -1;
        if (last !== i) {
          fp[s] = last;
          this.fullSlot[last] = s;
          this.writeInstance(last);
          this.repickWrites++;
        }
        this.writeInstance(i); // back to its chunk
        this.repickWrites++;
      }
    }
    for (const i of near) {
      want[i] = 0;
      if (this.fullSlot[i] >= 0) continue;
      const md = this.models[this.modelOf[i]];
      if (!md?.full) continue; // its mesh has not loaded yet: stays on the proxy
      this.fullSlot[i] = md.fullProps.length;
      md.fullProps.push(i);
      md.full.car.count = md.fullProps.length;
      this.writeInstance(i);
      this.repickWrites++;
    }
    for (const md of this.models) {
      if (!md.full) continue;
      md.full.car.count = md.fullProps.length;
      md.full.car.end();
    }
  }

  private distTo(i: number): number {
    const o = i * 7;
    // Not Math.hypot: it allocates its argument list (runs for every prop on each re-pick).
    const dx = this.pose[o] - this.cam.x;
    const dy = this.pose[o + 1] - this.cam.y;
    const dz = this.pose[o + 2] - this.cam.z;
    return Math.sqrt(dx * dx + dy * dy + dz * dz);
  }

  /** Number of props of the session (parked cars + cones). */
  get count(): number {
    return this.n;
  }

  /** The mesh, slot and model fit currently drawing prop `i` (null: none). */
  private drawer(i: number): { mesh: THREE.InstancedMesh; slot: number; fit: Fit | null } | null {
    if (i < 0 || i >= this.n) return null;
    if (this.kind[i] === PROP_KIND.cone) return this.cones ? { mesh: this.cones, slot: this.slot[i], fit: null } : null;
    if (this.kind[i] === PROP_KIND.krail) return this.krails ? { mesh: this.krails, slot: this.slot[i], fit: null } : null;
    const md = this.models[this.modelOf[i]];
    if (this.fullSlot[i] >= 0 && md?.full) return { mesh: md.full.car.paint, slot: this.fullSlot[i], fit: md.full.fit };
    const ch = this.chunks[this.chunkOf[i]];
    if (!ch || !md) return null;
    return ch.near ? { mesh: ch.proxy.paint, slot: this.slot[i], fit: md.proxyFit } : { mesh: ch.box, slot: this.slot[i], fit: UNIT_FIT };
  }

  /**
   * The pose prop `i` is drawn with, read back from its instance matrix: box centre (sim space,
   * m) and rotation `[x, y, z, w]`, or null when it has no drawn instance. Automation (e2e).
   */
  drawnPose(i: number): number[] | null {
    const w = this.wrecks.get(i);
    if (w?.ready) {
      const r = w.root;
      return [r.position.x, r.position.y, r.position.z, r.quaternion.x, r.quaternion.y, r.quaternion.z, r.quaternion.w];
    }
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
      const drawn = this.drawnPose(i) !== null;
      // Parked cars of the instanced tiers (a dented one draws itself; it still counts as drawn).
      const car = this.kind[i] === PROP_KIND.parkedCar && !this.wrecked[i];
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
      // A dented car keeps its full slot until the next re-pick, but its own mesh draws it.
      drawnFull: this.models.reduce((sum, md) => sum + md.fullProps.reduce((n, i) => n + (this.wrecked[i] ? 0 : 1), 0), 0),
      drawnProxy,
      drawnBox,
      drawnCones: this.cones?.count ?? 0,
      maxFull: this.maxFull,
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
      wrecks: this.wrecks.size,
      wrecksDrawn: [...this.wrecks.values()].filter((w) => w.ready).length,
      wreckReskins: this.wreckReskins,
    };
  }

  /** The first model parked cars are drawn as (null: a box-sized stand-in). */
  get model(): string | null {
    return this.models[0]?.id ?? null;
  }

  /** Every parked-car model of the session, in `prop_models()` order (automation). */
  get parkedModels(): (string | null)[] {
    return this.models.map((md) => md.id);
  }

  /** Model id prop `i` is drawn as (null: not a parked car or a box stand-in). */
  modelOfProp(i: number): string | null {
    if (i < 0 || i >= this.n || this.kind[i] !== PROP_KIND.parkedCar) return null;
    return this.models[this.modelOf[i]]?.id ?? null;
  }

  /** The dented parked car of prop `i` (automation): its entity, whether its panel mesh draws
   * it, and the largest drawn vertex offset from the undented mesh, m. */
  wreck(i: number): { entity: number; drawn: boolean; maxDisp: number } | null {
    const w = this.wrecks.get(i);
    if (!w) return null;
    return { entity: w.entity, drawn: w.ready, maxDisp: w.body.maxDisp() };
  }

  /** Drop every instance (new session). */
  clear(): void {
    for (const w of this.wrecks.values()) {
      w.root.removeFromParent();
      w.body.dispose();
    }
    this.wrecks.clear();
    this.wreckReskins = 0;
    // Chunks share vertex buffers: remove their meshes and dispose their geometry views (vertex
    // array state; the shared buffers go with the source geometries just after).
    for (const ch of this.chunks)
      for (const mesh of [...ch.proxy.meshes, ch.box]) {
        mesh.removeFromParent();
        mesh.geometry.dispose();
      }
    this.chunks = [];
    for (const md of this.models) {
      const pg = md.proxyGeo;
      for (const g of [pg.paint, pg.detail, pg.tail, pg.head, pg.indL, pg.indR]) g.dispose();
      md.full?.car.dispose();
    }
    this.models = [];
    this.farGeo?.dispose();
    this.farGeo = null;
    if (this.cones) {
      this.cones.removeFromParent();
      this.cones.geometry.dispose();
      this.cones = null;
    }
    if (this.krails) {
      this.krails.removeFromParent();
      this.krails.geometry.dispose();
      this.krails = null;
    }
    this.n = 0;
    this.parkedCount = 0;
    this.coneCount = 0;
    this.krailCount = 0;

    this.repicks = 0;
    this.movedWrites = 0;
    this.repickWrites = 0;
    this.updates = 0;
    this.updateMsSum = 0;
    this.updateMaxMs = 0;
  }
}
