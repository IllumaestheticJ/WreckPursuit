/**
 * Map data from `wp_wasm` (workstream 16 glue in `crates/wp_wasm/src/map.rs`):
 * `map_static_json()` once per city and `map_dynamic(player)` every frame. Keep the layout
 * constants in step with that file (`HEADER`, `UNIT_STRIDE`, `TOOL_STRIDE`, `h::*`, `flag::*`).
 */

/** Road class index in `MapRoad.class` (contract `RoadClass` order). */
export const ROAD_CLASSES = ["Highway", "Arterial", "Collector", "Local", "Alley"] as const;

/** One drawn road piece. */
export interface MapRoad {
  /** 0 highway, 1 arterial, 2 collector, 3 local, 4 alley. */
  class: number;
  /** 0 at grade, 1 bridge deck / overpass. */
  layer: number;
  /** Flat `[x0, y0, x1, y1, …]`, world m. */
  pts: number[];
}

/** District polygon (`RoadNetwork::zones` order; the first containing zone wins). */
export interface MapZone {
  id: number;
  kind: "Core" | "Commercial" | "Residential" | "Industrial" | "Port" | "Suburb";
  poly: number[];
}

/** Point of interest. */
export interface MapPoi {
  kind: "Safehouse" | "Garage" | "PoliceStation" | "Helipad" | "RaceStartGrid";
  name: string;
  pos: [number, number];
}

/** `map_static_json()`. */
export interface MapStatic {
  city: string;
  /** `[min_x, min_y, max_x, max_y]`, m. */
  bounds: [number, number, number, number];
  roads: MapRoad[];
  zones: MapZone[];
  pois: MapPoi[];
  /** Signalised junction centres. */
  signals: [number, number][];
}

/** Layout of `map_dynamic()`. */
export const DYN = {
  VERSION: 1,
  HEADER: 20,
  UNIT_STRIDE: 8,
  TOOL_STRIDE: 6,
} as const;

/** Unit state (flags bits 0–3). */
export const UNIT_STATES = ["idle", "patrol", "respond", "pursue", "intercept", "roadblock", "spikes", "track", "out"] as const;
export type UnitState = (typeof UNIT_STATES)[number];

/** Unit flag bits above the state code. */
export const UNIT_FLAG = { HELI: 1 << 4, LIGHTS: 1 << 5, ON_PLAYER: 1 << 6, WRECKED: 1 << 7, HUMAN: 1 << 8, HAS_TARGET: 1 << 9 } as const;

/** Player pursuit status from the header. */
export const MAP_STATUS = ["clear", "wanted", "pursuit", "searching"] as const;
export type MapStatus = (typeof MAP_STATUS)[number];

/** One police unit this frame. */
export interface MapUnit {
  id: number;
  x: number;
  y: number;
  /** Yaw about +Z, 0 = +Y (north), counter-clockwise, rad. */
  yaw: number;
  speed: number;
  state: UnitState;
  heli: boolean;
  /** Lights and siren (responding / pursuing / intercepting / roadblock / spikes): blink. */
  lights: boolean;
  /** Assignment is about the player. */
  onPlayer: boolean;
  wrecked: boolean;
  human: boolean;
  /** Intercept point / tracked spot. */
  target: [number, number] | null;
}

/** A roadblock or spike strip, drawn from `c - dir·half` to `c + dir·half`. */
export interface MapTool {
  kind: "roadblock" | "spikes";
  x: number;
  y: number;
  dir: [number, number];
  halfLenM: number;
}

/** One decoded `map_dynamic()` frame. */
export interface MapFrame {
  status: MapStatus;
  level: number;
  /** Search zone while searching (centre = last known position). */
  search: { x: number; y: number; r: number } | null;
  /** Escape progress 0..1 while searching. */
  escape: number;
  carSightM: number;
  heliSightM: number;
  heliSpotM: number;
  lastKnown: [number, number] | null;
  player: { x: number; y: number; yaw: number; speed: number } | null;
  units: MapUnit[];
  tools: MapTool[];
}

/** An empty frame (no player, no police). */
export const EMPTY_FRAME: MapFrame = {
  status: "clear",
  level: 0,
  search: null,
  escape: 0,
  carSightM: 0,
  heliSightM: 0,
  heliSpotM: 0,
  lastKnown: null,
  player: null,
  units: [],
  tools: [],
};

/** Decode `map_dynamic()`. Throws on a layout version it does not know. */
export function decodeMapDynamic(a: ArrayLike<number>): MapFrame {
  if (a.length < DYN.HEADER) return EMPTY_FRAME;
  if (a[0] !== DYN.VERSION) throw new Error(`map_dynamic layout v${a[0]}, expected v${DYN.VERSION}`);
  const nUnits = a[1] | 0;
  const nTools = a[2] | 0;
  const units: MapUnit[] = [];
  let o = DYN.HEADER;
  for (let i = 0; i < nUnits; i++, o += DYN.UNIT_STRIDE) {
    const f = a[o + 5] | 0;
    units.push({
      id: a[o],
      x: a[o + 1],
      y: a[o + 2],
      yaw: a[o + 3],
      speed: a[o + 4],
      state: UNIT_STATES[f & 0xf] ?? "idle",
      heli: (f & UNIT_FLAG.HELI) !== 0,
      lights: (f & UNIT_FLAG.LIGHTS) !== 0,
      onPlayer: (f & UNIT_FLAG.ON_PLAYER) !== 0,
      wrecked: (f & UNIT_FLAG.WRECKED) !== 0,
      human: (f & UNIT_FLAG.HUMAN) !== 0,
      target: f & UNIT_FLAG.HAS_TARGET ? [a[o + 6], a[o + 7]] : null,
    });
  }
  const tools: MapTool[] = [];
  for (let i = 0; i < nTools; i++, o += DYN.TOOL_STRIDE) {
    tools.push({ kind: a[o] === 1 ? "spikes" : "roadblock", x: a[o + 1], y: a[o + 2], dir: [a[o + 3], a[o + 4]], halfLenM: a[o + 5] });
  }
  return {
    status: MAP_STATUS[a[3] | 0] ?? "clear",
    level: a[4] | 0,
    search: a[7] > 0 ? { x: a[5], y: a[6], r: a[7] } : null,
    escape: a[8],
    carSightM: a[9],
    heliSightM: a[10],
    heliSpotM: a[11],
    lastKnown: a[14] > 0.5 ? [a[12], a[13]] : null,
    player: a[19] > 0.5 ? { x: a[15], y: a[16], yaw: a[17], speed: a[18] } : null,
    units,
    tools,
  };
}

/** Whether a unit's dot blinks (lights on) rather than shows steady (patrol, idle). */
export function unitBlinks(u: MapUnit): boolean {
  return u.lights && !u.wrecked;
}

/** The player is wanted (sight areas and search zone are drawn). */
export function isWanted(f: MapFrame): boolean {
  return f.status !== "clear";
}
