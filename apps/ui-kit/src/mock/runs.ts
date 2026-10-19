/**
 * Parsing simcli run output (`apps/simcli/SCENARIOS.md` → Outputs):
 * - `events.jsonl`: one `{"tick", "t_s", "event": <SimEvent>}` per line
 * - `metrics.csv`:  `t_s,tick,entity,x,y,z,speed_mps,lat_g,long_g,yaw_rate_rps,engine_rpm,gear,health,crush_depth_m`
 * - `summary.json`: maps scenario entity ids (`"car"`) to `EntityId`s and car ids
 *
 * Fixtures live in `fixtures/runs/<name>/` (see `scripts/gen-fixtures.mjs`).
 */
import type { SimEvent } from "../generated/contracts";
import type { CaptainView } from "../model/police";
import type { CityMap } from "../model/types";

/** One line of `events.jsonl`. */
export interface EventLine {
  tick: number;
  t_s: number;
  event: SimEvent;
}

/** One line of a recorded run's `captain.jsonl` (`wp_police::CaptainView` samples). */
export interface CaptainSample {
  tick: number;
  view: CaptainView;
}

/** One row of `metrics.csv`. */
export interface MetricsRow {
  tS: number;
  tick: number;
  /** Scenario entity id string (e.g. `"car"`), see `RunEntity`. */
  entity: string;
  x: number;
  y: number;
  z: number;
  speedMps: number;
  latG: number;
  longG: number;
  yawRateRps: number;
  engineRpm: number;
  gear: number;
  health: number;
  crushDepthM: number;
}

/** Scenario entity ↔ sim entity mapping from `summary.json`. */
export interface RunEntity {
  /** Scenario id (`"car"`, `"striker"`). */
  id: string;
  /** `EntityId` number used in events. */
  entity: number;
  /** `VehicleDef` id. */
  car: string;
}

/** A parsed run, ready for the replayer. */
export interface Run {
  id: string;
  label: string;
  /** True for hand-authored scripts (not produced by simcli). */
  synthetic: boolean;
  entities: RunEntity[];
  events: EventLine[];
  /** Metrics grouped by scenario entity id, in tick order. */
  metrics: Record<string, MetricsRow[]>;
  totalTicks: number;
  ticksPerSecond: number;
  /** `CaptainView` samples in tick order (recorded wp_police runs only). */
  captain: CaptainSample[];
  /** The run's own map (recorded wp_police runs drive E's police city), else null. */
  map: CityMap | null;
}

/** Parse `events.jsonl` (blank lines ignored; a bad line throws with its line number). */
export function parseEventsJsonl(text: string): EventLine[] {
  const out: EventLine[] = [];
  text.split("\n").forEach((line, i) => {
    const t = line.trim();
    if (!t) return;
    let v: unknown;
    try {
      v = JSON.parse(t);
    } catch (e) {
      throw new Error(`events.jsonl:${i + 1}: ${(e as Error).message}`);
    }
    const o = v as EventLine;
    if (typeof o.tick !== "number" || typeof o.event !== "object" || o.event === null) {
      throw new Error(`events.jsonl:${i + 1}: expected {tick, t_s, event}`);
    }
    out.push({ tick: o.tick, t_s: o.t_s ?? 0, event: o.event });
  });
  return out;
}

const METRIC_COLUMNS = [
  "t_s",
  "tick",
  "entity",
  "x",
  "y",
  "z",
  "speed_mps",
  "lat_g",
  "long_g",
  "yaw_rate_rps",
  "engine_rpm",
  "gear",
  "health",
  "crush_depth_m",
];

/** Parse `metrics.csv`, grouped by entity. */
export function parseMetricsCsv(text: string): Record<string, MetricsRow[]> {
  const lines = text.split("\n").filter((l) => l.trim());
  if (lines.length === 0) return {};
  const header = lines[0].split(",");
  if (header.join(",") !== METRIC_COLUMNS.join(",")) {
    throw new Error(`metrics.csv: unexpected header ${lines[0]}`);
  }
  const out: Record<string, MetricsRow[]> = {};
  for (const line of lines.slice(1)) {
    const c = line.split(",");
    const n = (i: number) => Number(c[i]);
    const row: MetricsRow = {
      tS: n(0),
      tick: n(1),
      entity: c[2],
      x: n(3),
      y: n(4),
      z: n(5),
      speedMps: n(6),
      latG: n(7),
      longG: n(8),
      yawRateRps: n(9),
      engineRpm: n(10),
      gear: n(11),
      health: n(12),
      crushDepthM: n(13),
    };
    (out[row.entity] ??= []).push(row);
  }
  return out;
}

/** Build a [`Run`] from the three simcli files. */
export function parseRun(
  id: string,
  label: string,
  files: { events: string; metrics: string; summary: string; captain?: string; map?: string },
  synthetic = false,
): Run {
  const summary = JSON.parse(files.summary) as { ticks: number; entities: RunEntity[] };
  const metrics = parseMetricsCsv(files.metrics);
  const events = parseEventsJsonl(files.events);
  const lastMetric = Math.max(0, ...Object.values(metrics).map((rows) => rows[rows.length - 1]?.tick ?? 0));
  return {
    id,
    label,
    synthetic,
    entities: summary.entities.map((e) => ({ id: e.id, entity: e.entity, car: e.car })),
    events,
    metrics,
    totalTicks: Math.max(summary.ticks ?? 0, lastMetric),
    ticksPerSecond: 240,
    captain: files.captain ? parseCaptainJsonl(files.captain) : [],
    map: files.map ? parseMapJson(files.map) : null,
  };
}

/** Parse `captain.jsonl` (`{"tick","view"}` per line). */
export function parseCaptainJsonl(text: string): CaptainSample[] {
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as CaptainSample);
}

/** Parse a map fixture (`scripts/gen-fixtures.mjs` / `tools/record-police` format). */
export function parseMapJson(text: string): CityMap {
  const m = JSON.parse(text) as {
    id: string;
    bounds_min: [number, number];
    bounds_max: [number, number];
    roads: { id: number; points: [number, number][]; speed_limit_mps: number; district?: CityMap["roads"][number]["district"] }[];
    zones: { id: number; kind: string; polygon: [number, number][] }[];
    pois: { kind: string; name: string; pos: [number, number] }[];
  };
  return {
    id: m.id,
    boundsMin: m.bounds_min,
    boundsMax: m.bounds_max,
    roads: m.roads.map((r) => ({ id: r.id, points: r.points, speedLimitMps: r.speed_limit_mps, district: r.district })),
    zones: m.zones,
    pois: m.pois,
  };
}

/** Index of the last captain sample with `tick <= t`, or -1. */
export function captainAt(samples: CaptainSample[], tick: number): number {
  let lo = 0;
  let hi = samples.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (samples[mid].tick <= tick) {
      ans = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return ans;
}

/** Index of the last row with `tick <= t` (binary search), or -1. */
export function rowAt(rows: MetricsRow[], tick: number): number {
  let lo = 0;
  let hi = rows.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid].tick <= tick) {
      ans = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return ans;
}

/** Heading (rad, 0 = +Y north, positive = clockwise toward +X) from motion around `i`. */
export function headingAt(rows: MetricsRow[], i: number): number {
  if (rows.length < 2 || i < 0) return 0;
  // Look back for a sample far enough away to give a stable direction.
  for (let j = i - 1; j >= 0; j--) {
    const dx = rows[i].x - rows[j].x;
    const dy = rows[i].y - rows[j].y;
    if (dx * dx + dy * dy > 0.04) return Math.atan2(dx, dy);
  }
  for (let j = i + 1; j < rows.length; j++) {
    const dx = rows[j].x - rows[i].x;
    const dy = rows[j].y - rows[i].y;
    if (dx * dx + dy * dy > 0.04) return Math.atan2(dx, dy);
  }
  return 0;
}
