// Typed wrappers over the wp_wasm `net_*` surface (crates/wp_wasm/src/net.rs).
import * as wasm from "../wasm/pkg/wp_wasm.js";

/** Mirrors wp_contracts::VehicleInput (missing fields = 0/false). */
export interface NetInput {
  steer?: number;
  throttle?: number;
  brake?: number;
  handbrake?: number;
  headlights?: boolean;
  siren?: boolean;
  horn?: boolean;
}

export type EntityKind = "racer" | "police" | "ai_police" | "traffic";

export interface RenderVehicle {
  entity: number;
  car_id: string;
  kind: EntityKind;
  own: boolean;
  pos: [number, number, number];
  rot: [number, number, number, number];
  speed_mps: number;
  gear: number;
  engine_rpm: number;
  health: number;
  body_size_m: [number, number, number];
}

export type RoleName = "racer" | "police_chase" | "police_captain" | "spectator";

export interface RaceInfo {
  race: number;
  host: number;
  state: "lobby" | "countdown" | "running" | "finished";
  start_tick: number;
  stake: number;
  laps: number;
  checkpoints: [number, number][];
  members: number[];
}

export interface NetStatus {
  phase: string;
  player: number | null;
  token: string;
  role: RoleName;
  vehicle: number | null;
  mode: "arcade" | "sim";
  captain_offer: number | null;
  unit_order: { kind: number; x: number; y: number; target: number } | null;
  races: RaceInfo[];
  race_progress: { race: number; player: number; next: number }[];
  race_results: {
    race: number;
    entries: { player: number; place: number; time_s: number; payout: number }[];
  } | null;
  invites: [number, number][];
  account: { cash: number; rep: number; cop_rank: number; bounty: number };
  notices: string[];
}

export interface NetStats {
  rtt_ms: number;
  jitter_ms: number;
  reconciles: number;
  mispredictions: number;
  last_correction_m: number;
  max_correction_m: number;
  correction_rms_m: number;
  prediction_resets: number;
  bps_in: number;
  bps_out: number;
  bytes_in: number;
  bytes_out: number;
  snapshots: number;
  hash_checks: number;
  hash_mismatches: number;
  resyncs: number;
  remotes: number;
  predicted_tick: number | null;
}

export interface MapUnit {
  entity: number;
  kind: EntityKind;
  x: number;
  y: number;
  heading: number;
  wanted: number;
  order: number;
}

export interface CaptainMapData {
  tick: number;
  units: MapUnit[];
  incidents: { x: number; y: number; kind: number; age_s: number }[];
  heat: { x: number; y: number; size: number; heat: number }[];
}

export type OrderKind = "move" | "block" | "pursue" | "hold";

export const ROLE_CODE: Record<RoleName, number> = {
  racer: 0,
  police_chase: 1,
  police_captain: 2,
  spectator: 3,
};

export const create = (name: string, inputSource = 0): void => wasm.net_create(name, inputSource);
export const connect = (nowMs: number, token = ""): void => wasm.net_connect(nowMs, token);
export const receive = (bytes: Uint8Array, nowMs: number): void => wasm.net_receive(bytes, nowMs);
export const update = (nowMs: number, input: NetInput): void =>
  wasm.net_update(nowMs, JSON.stringify(input));
export const render = (nowMs: number): RenderVehicle[] => JSON.parse(wasm.net_render_json(nowMs));
export const status = (): NetStatus => JSON.parse(wasm.net_status_json());
export const stats = (): NetStats => JSON.parse(wasm.net_stats_json());
export const captainMap = (): CaptainMapData => JSON.parse(wasm.net_captain_json());
export const requestRole = (role: RoleName, nowMs: number): void =>
  wasm.net_request_role(ROLE_CODE[role], nowMs);
export const captainOrder = (
  order: { kind: OrderKind; units: number[]; x?: number; y?: number; target?: number },
  nowMs: number,
): void => wasm.net_captain_order_json(JSON.stringify(order), nowMs);
export const raceCreate = (laps: number, checkpoints: [number, number][], nowMs: number): void =>
  wasm.net_race_create_json(JSON.stringify({ laps, checkpoints }), nowMs);
export const raceJoin = (race: number, nowMs: number): void => wasm.net_race_join(race, nowMs);
export const raceStart = (race: number, nowMs: number): void => wasm.net_race_start(race, nowMs);
export const raceLeave = (race: number, nowMs: number): void => wasm.net_race_leave(race, nowMs);
export const bye = (nowMs: number): void => wasm.net_bye(nowMs);

/** Split `net_take_outgoing()` (u32 LE length + bytes records) into frames. */
export function takeOutgoing(): Uint8Array[] {
  const buf = wasm.net_take_outgoing();
  const frames: Uint8Array[] = [];
  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  let off = 0;
  while (off + 4 <= buf.byteLength) {
    const len = view.getUint32(off, true);
    off += 4;
    frames.push(buf.subarray(off, off + len));
    off += len;
  }
  return frames;
}
