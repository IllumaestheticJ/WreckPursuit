// Who draws each car this frame (lead helper, playtest row 25 "LOD pop-in").
//
// Every traffic agent and police unit has a draw record from wp_wasm (`traffic_draw()` /
// `police_draw()`, crates/wp_wasm/src/lod.rs): a stable id, its sim entity once it is a
// physical vehicle, and a pose seated on the road exactly as the sim places a promoted car.
// Each frame a car is drawn by exactly one of:
//   * its VehicleView (full per-panel mesh), once the sim spawned it and its mesh is ready;
//   * the instanced fleet (render/fleet.ts) otherwise: same model, same livery, same pose.
// So a promotion or demotion swaps the drawer on one frame with no gap, jump or recolour.
// Multi-model sessions (night run: 7 traffic, 5 police models): each record's `model` picks
// its car model from `sim.sessionCarModels()` (the fleet instances per model); a promoted car's
// view shows the same model (its vehicle_info().car_id).
//
// `LodLog` records each car's draw state per frame for automation (e2e/run.mjs,
// e2e/lod_shots.mjs): frames where a car had no proper mesh, the pose jump on every swap with
// a few frames either side, and how far from the player cars appear and vanish.
import * as THREE from "three";
import { D, DRAW_STRIDE, AGENT_FLAG, POLICE_DRAW_FLAG } from "../wasm/layout";
import type { SessionModels } from "../wasm/sim";
import type { VehicleView } from "../vehicle_view";
import { layers } from "../settings";
import { BAR, Fleet, POLICE_PAINT, TAIL, TRAFFIC_PALETTE, type DrawMode } from "./fleet";

/** Hysteresis around the full-mesh distance for handing a far view to the fleet, m. */
const VIEW_LOD_BAND_M = 15;

/** How a car was drawn: by its view, by the fleet (full / proxy mesh), or not at all. */
export type LodMode = "view" | DrawMode | "none";

/** One car's draw state in one frame. */
export interface DrawState {
  /** `t<agent id>` traffic, `p<unit id>` police, `v<entity>` other sim vehicles. */
  key: string;
  mode: LodMode;
  /** Drawn without the car's real mesh where it should have it (loading placeholder). */
  fallback: boolean;
  entity: number;
  /** Drawn position, sim space (m). */
  pos: [number, number, number];
  /** Where the car is in the sim this tick (its draw record; the view itself for other sim
   * vehicles), m. `pos - truth` is the drawer's own offset (a view lags by its interpolation). */
  truth: [number, number, number];
  /** Distance from the camera, m. */
  dist: number;
}

/** Civilian livery of a traffic draw record's flags. */
export const trafficPaint = (flags: number) => TRAFFIC_PALETTE[(flags >> 8) & 7];

/** Tail lamp colour from traffic flags at time `t` (hazards blink at 1.5 Hz). */
function tailOf(flags: number, t: number): number {
  if (flags & AGENT_FLAG.hazard && (t * 1.5) % 1 < 0.5) return TAIL.hazard;
  return flags & AGENT_FLAG.brake ? TAIL.brake : TAIL.off;
}

/** Police light bar colour (alternating red / blue while pursuing). */
function barOf(flags: number, t: number, i: number): number {
  if (!(flags & POLICE_DRAW_FLAG.chasing)) return BAR.off;
  return (t * 2.5 + i * 0.17) % 1 < 0.5 ? BAR.red : BAR.blue;
}

export class LodDirector {
  readonly fleet = new Fleet();
  readonly log = new LodLog();
  models: SessionModels = { cars: [], traffic: [], police: [] };
  /** Draw states of the last frame (pooled: valid until the next update). */
  readonly states: DrawState[] = [];
  private readonly pool: DrawState[] = [];
  private used = 0;
  private readonly claimed = new Set<number>();
  private readonly paints = new Map<number, number>();
  private readonly tKeys = new Map<number, string>();
  private readonly pKeys = new Map<number, string>();
  private readonly vKeys = new Map<number, string>();
  private cx = 0;
  private cy = 0;
  private cz = 0;
  /** Instanced traffic past this distance from the camera (m) is not drawn (phones). Police and
   * sim vehicles are always drawn. */
  trafficDrawM = Infinity;
  /** Promoted traffic / police sim vehicles farther than this from the camera (m) are drawn by
   * the fleet (one instanced draw for all of them) instead of their own view (~40 draw calls
   * each, per panel). Low / touch graphics profiles only; the player and AI racers always use
   * their view. 10 m of hysteresis so a car on the boundary does not flip drawers every frame. */
  viewDrawM = Infinity;
  /** Entities currently handed to the fleet by `viewDrawM`. */
  private readonly farViews = new Set<number>();
  private readonly cam = new THREE.Vector3();

  /** Traffic livery per sim entity (for colouring a promoted car's VehicleView). The map is
   * reused: valid until the next call. */
  paintOfEntity(records: Float32Array): Map<number, number> {
    const out = this.paints;
    out.clear();
    for (let o = 0; o < records.length; o += DRAW_STRIDE) {
      const e = records[o + D.entity];
      if (e > 0) out.set(e, trafficPaint(records[o + D.flags]));
    }
    return out;
  }

  /**
   * Draw this frame's cars. `camera` is the Three camera (Y-up world; sim space is Z-up),
   * `focus` the player's drawn position (sim space) for the log's distances.
   */
  update(
    traffic: Float32Array,
    police: Float32Array,
    views: Map<number, VehicleView>,
    camera: THREE.Camera,
    focus: THREE.Vector3 | null,
    timeS: number,
  ): void {
    // Three (x, y, z) → sim (x, -z, y): `world` is rotated -90° about X.
    const c = camera.getWorldPosition(this.cam);
    this.cx = c.x;
    this.cy = -c.z;
    this.cz = c.y;
    // Per frame for every car: pooled states, cached keys, no closures (browser perf, 12b).
    this.used = 0;
    const states = this.states;
    states.length = 0;
    const claimed = this.claimed;
    claimed.clear();
    this.fleet.begin();

    const showTraffic = layers.on("traffic");
    const tModels = this.models.traffic;
    for (let o = 0; o < traffic.length; o += DRAW_STRIDE) {
      const tModel = this.fleetModel(tModels, traffic[o + D.model], false);
      const entity = traffic[o + D.entity];
      const flags = traffic[o + D.flags];
      const key = keyOf(this.tKeys, "t", traffic[o + D.id]);
      const view = entity > 0 ? views.get(entity) : undefined;
      if (view) claimed.add(entity);
      const far = !view && this.dist(traffic[o + D.pos], traffic[o + D.pos + 1], traffic[o + D.pos + 2]) > this.trafficDrawM;
      states.push(this.draw(key, entity, view, traffic, o, showTraffic && !far ? tModel : null, trafficPaint(flags), tailOf(flags, timeS), null));
    }

    const pModels = this.models.police;
    let unit = 0;
    for (let o = 0; o < police.length; o += DRAW_STRIDE, unit++) {
      const flags = police[o + D.flags];
      if (flags & POLICE_DRAW_FLAG.helicopter) continue; // SceneView draws helicopters
      const pModel = this.fleetModel(pModels, police[o + D.model], true);
      const entity = police[o + D.entity];
      const view = entity > 0 ? views.get(entity) : undefined;
      if (view) claimed.add(entity);
      states.push(this.draw(keyOf(this.pKeys, "p", police[o + D.id]), entity, view, police, o, pModel, POLICE_PAINT, TAIL.off, barOf(flags, timeS, unit)));
    }

    // Player, AI racers and scenario cars: always their own view.
    for (const [id, v] of views) {
      if (claimed.has(id)) continue;
      const p = v.root.position;
      const s = this.next(keyOf(this.vKeys, "v", id), "view", !v.panels.ready && layers.on("panels") && layers.on("body"), id, this.dist(p.x, p.y, p.z));
      setXYZ(s.pos, p.x, p.y, p.z);
      setXYZ(s.truth, p.x, p.y, p.z);
      states.push(s);
    }
    this.fleet.end();
    this.log.push(timeS, states, focus);
  }

  /**
   * Fleet model of a draw record's `model` index into `ids` (a session model list); an index out
   * of range falls back to the first model, null when the session has none.
   */
  private fleetModel(ids: string[], index: number, police: boolean): ReturnType<Fleet["model"]> | null {
    if (ids.length === 0) return null;
    return this.fleet.model(ids[index] ?? ids[0], police);
  }

  /** Distance from the camera (sim space), m. */
  private dist(x: number, y: number, z: number): number {
    const dx = x - this.cx;
    const dy = y - this.cy;
    const dz = z - this.cz;
    return Math.sqrt(dx * dx + dy * dy + dz * dz);
  }

  /** The next pooled state of this frame, filled in. */
  private next(key: string, mode: LodMode, fallback: boolean, entity: number, dist: number): DrawState {
    let s = this.pool[this.used];
    if (!s) this.pool.push((s = { key, mode, fallback, entity, pos: [0, 0, 0], truth: [0, 0, 0], dist }));
    this.used++;
    s.key = key;
    s.mode = mode;
    s.fallback = fallback;
    s.entity = entity;
    s.dist = dist;
    return s;
  }

  private draw(
    key: string,
    entity: number,
    view: VehicleView | undefined,
    rec: Float32Array,
    o: number,
    model: ReturnType<Fleet["model"]> | null,
    paint: number,
    tail: number,
    bar: number | null,
  ): DrawState {
    // A sim vehicle's view draws it once its mesh is ready; until then it stays hidden and the
    // fleet draws the car (no placeholder box, no empty frame).
    let ready = view?.drawable ?? false;
    const x = rec[o + D.pos];
    const y = rec[o + D.pos + 1];
    const z = rec[o + D.pos + 2];
    // Low / touch profiles (`viewDrawM`): a promoted traffic / police car farther than that is
    // drawn by the fleet (one instanced draw for all) instead of its own view; 10 m of
    // hysteresis. `lodFar` also lets the view skip its lattice fetch and re-skin meanwhile.
    if (view && ready && model && Number.isFinite(this.viewDrawM)) {
      const p = view.root.position;
      const dv = this.dist(p.x, p.y, p.z);
      view.lodFar = view.lodFar ? dv > this.viewDrawM : dv > this.viewDrawM + 10;
      if (view.lodFar) ready = false;
    } else if (view) view.lodFar = false;
    if (view) view.root.visible = ready;
    if (view && ready) {
      const p = view.root.position;
      const s = this.next(key, "view", false, entity, this.dist(p.x, p.y, p.z));
      setXYZ(s.pos, p.x, p.y, p.z);
      setXYZ(s.truth, x, y, z);
      return s;
    }
    const d = this.dist(x, y, z);
    let mode: LodMode = "none";
    let fallback = false;
    if (model) {
      mode = this.fleet.add(model, rec, o + D.pos, d, paint, tail, bar);
      fallback = mode === "proxy" && d < this.fleet.fullLodM && !model.full;
    }
    const s = this.next(key, mode, fallback, entity, d);
    setXYZ(s.pos, x, y, z);
    setXYZ(s.truth, x, y, z);
    return s;
  }
}

function setXYZ(a: [number, number, number], x: number, y: number, z: number): void {
  a[0] = x;
  a[1] = y;
  a[2] = z;
}

/** `prefix + id`, cached (ids are small integers that repeat every frame). */
function keyOf(cache: Map<number, string>, prefix: string, id: number): string {
  let k = cache.get(id);
  if (k === undefined) cache.set(id, (k = prefix + id));
  return k;
}

/** One frame of one car in the swap windows of the log. */
export interface LogRow {
  frame: number;
  t: number;
  mode: LodMode;
  fallback: boolean;
  entity: number;
  pos: [number, number, number];
  truth: [number, number, number];
}

/** A drawer change (view ↔ fleet) of one car. */
export interface Swap {
  key: string;
  frame: number;
  t: number;
  from: LodMode;
  to: LodMode;
  /**
   * How far the drawn car moves relative to the car itself at the swap: the change of
   * (drawn − sim pose) from the frame before to the swap frame, m. Frame-rate independent (at
   * 4 fps a car moves metres per frame). The sim pose's own continuity through promotions is
   * checked tick by tick in wp_wasm (`lod::tests`).
   */
  jump: number;
  /** Rows from 3 frames before to 3 frames after. */
  rows: LogRow[];
}

interface Track {
  /** Last few frames (oldest first). */
  rows: LogRow[];
  /** Seconds drawn as a placeholder so far. */
  placeholderS: number;
}

/** Per-frame draw-state recorder (enabled by automation; costs nothing when off). */
export class LodLog {
  enabled = false;
  frame = 0;
  t0 = 0;
  /** Frames (after the first second) where a car had no proper mesh. */
  missing = 0;
  missingSamples: { key: string; frame: number; t: number; mode: LodMode; dist: number }[] = [];
  swaps: Swap[] = [];
  /** Distance from the player where each car first appeared / last was seen, m. */
  appear: { key: string; t: number; dist: number }[] = [];
  vanish: { key: string; t: number; dist: number }[] = [];
  /** Seconds each car spent drawn as a placeholder (mesh still loading), when non-zero. */
  placeholder: { key: string; s: number }[] = [];
  private tracks = new Map<string, Track>();
  private open: { swap: Swap; left: number }[] = [];
  private lastT = 0;

  /** Start (or restart) recording; `t` is the sim time. */
  start(t = 0): void {
    this.enabled = true;
    this.reset(t);
  }

  reset(t = 0): void {
    this.frame = 0;
    this.t0 = t;
    this.missing = 0;
    this.missingSamples = [];
    this.swaps = [];
    this.appear = [];
    this.vanish = [];
    this.placeholder = [];
    this.tracks.clear();
    this.open = [];
    this.lastT = t;
  }

  push(t: number, states: DrawState[], focus: THREE.Vector3 | null): void {
    if (!this.enabled) return;
    this.frame++;
    const dt = Math.max(t - this.lastT, 0);
    this.lastT = t;
    const fd = (p: [number, number, number]) => (focus ? Math.hypot(p[0] - focus.x, p[1] - focus.y, p[2] - focus.z) : 0);
    const seen = new Set<string>();
    const settled = t - this.t0 > 1;
    for (const s of states) {
      seen.add(s.key);
      // Copies: the director's states are pooled and rewritten every frame.
      const row: LogRow = { frame: this.frame, t, mode: s.mode, fallback: s.fallback, entity: s.entity, pos: [s.pos[0], s.pos[1], s.pos[2]], truth: [s.truth[0], s.truth[1], s.truth[2]] };
      const tr = this.tracks.get(s.key);
      if (settled && (s.mode === "none" || s.fallback)) {
        this.missing++;
        if (this.missingSamples.length < 50) this.missingSamples.push({ key: s.key, frame: this.frame, t, mode: s.mode, dist: s.dist });
      }
      if (!tr) {
        this.tracks.set(s.key, { rows: [row], placeholderS: 0 });
        if (settled) this.appear.push({ key: s.key, t, dist: fd(s.pos) });
        continue;
      }
      if (s.fallback) tr.placeholderS += dt;
      const prev = tr.rows[tr.rows.length - 1];
      const drawer = (m: LodMode) => (m === "view" ? "view" : m === "none" ? "none" : "fleet");
      if (drawer(prev.mode) !== drawer(s.mode)) {
        const off = (r: { pos: number[]; truth: number[] }) => r.pos.map((p, i) => p - r.truth[i]);
        const a = off(prev);
        const b = off(s);
        const jump = Math.hypot(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
        const swap: Swap = { key: s.key, frame: this.frame, t, from: prev.mode, to: s.mode, jump, rows: [...tr.rows.slice(-3), row] };
        this.swaps.push(swap);
        this.open.push({ swap, left: 3 });
      }
      tr.rows.push(row);
      if (tr.rows.length > 8) tr.rows.shift();
      for (const w of this.open) if (w.swap.key === s.key && w.swap.frame !== this.frame && w.left > 0) {
        w.swap.rows.push(row);
        w.left--;
      }
    }
    this.open = this.open.filter((w) => w.left > 0 && seen.has(w.swap.key));
    for (const [key, tr] of this.tracks) {
      if (seen.has(key)) continue;
      if (tr.placeholderS > 0) this.placeholder.push({ key, s: tr.placeholderS });
      this.vanish.push({ key, t, dist: fd(tr.rows[tr.rows.length - 1].pos) });
      this.tracks.delete(key);
    }
  }

  /** Summary for automation. */
  summary() {
    const minOf = (xs: { key: string; dist: number }[], prefix: string) =>
      xs.filter((x) => x.key.startsWith(prefix)).reduce((m, x) => Math.min(m, x.dist), Infinity);
    const waits = [...this.placeholder, ...[...this.tracks].filter(([, tr]) => tr.placeholderS > 0).map(([key, tr]) => ({ key, s: tr.placeholderS }))];
    return {
      frames: this.frame,
      missing: this.missing,
      missingSamples: this.missingSamples.slice(0, 10),
      swaps: this.swaps.length,
      maxSwapJump: this.swaps.reduce((m, s) => Math.max(m, s.jump), 0),
      worstSwaps: [...this.swaps].sort((a, b) => b.jump - a.jump).slice(0, 3),
      minAppearTraffic: minOf(this.appear, "t"),
      minVanishTraffic: minOf(this.vanish, "t"),
      minAppearPolice: minOf(this.appear, "p"),
      minVanishPolice: minOf(this.vanish, "p"),
      cars: this.tracks.size + this.vanish.length,
      /** Longest time any car was shown as a loading placeholder, s (0 = always its real mesh). */
      maxPlaceholderS: waits.reduce((m, x) => Math.max(m, x.s), 0),
    };
  }
}
