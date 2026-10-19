// Typed wrapper over the wp_wasm surface. Per-frame calls return typed arrays (see layout.ts);
// JSON only for rare structured data.
import initWasm, * as wasm from "./pkg/wp_wasm.js";
import type { PoliceStatus } from "../police";

/** Driver input in the testbed (packed to Float32Array for set_input). */
export interface DriverInput {
  steer: number;
  throttle: number;
  brake: number;
  handbrake: number;
  clutch: number;
  gearUp: boolean;
  gearDown: boolean;
  horn: boolean;
  headlights: boolean;
}

export interface WheelInfo {
  position_m: [number, number, number];
  radius_m: number;
  width_m: number;
  steerable: boolean;
  travel_m: number;
  tire_id: string;
}

/** vehicle_info(entity) JSON. */
export interface VehicleInfo {
  entity: number;
  /** Scenario-local entity name (sim-spawned vehicles: `police7`, `traffic12`, …). */
  name: string;
  /** `Player` | `AiRacer` | `Police` | `Traffic` (`wp_sim::VehicleKind`). */
  kind: string;
  car_id: string;
  display_name: string;
  body_size_m: [number, number, number];
  mass_kg: number;
  wheelbase_m: number;
  cg_m: [number, number, number];
  redline_rpm: number;
  rev_limit_rpm: number;
  gears: number;
  wheels: WheelInfo[];
  lattice_nodes: number;
  lattice_beams: number;
  skin_binds: number;
  regions: string[];
  components: string[];
  mounted_components: string[];
  surfaces: string[];
  spawn_pos: [number, number, number];
}

export interface AssistProfile {
  abs: boolean;
  tcs: boolean;
  esc: boolean;
  steer_speed_sensitivity: number;
  countersteer_help: number;
  throttle_filter: number;
  brake_filter: number;
  auto_gears: boolean;
  auto_clutch: boolean;
}

export interface Barrier {
  pos: [number, number, number];
  size: [number, number, number];
  yaw: number;
}

/** simcli scenario format v2 (apps/simcli/SCENARIOS.md); only the fields the UI reads. */
export interface Scenario {
  name: string;
  seed: number;
  mode: "Arcade" | "Simulation";
  duration_s: number;
  barriers?: Barrier[];
  entities: { id: string; car: string; pos: number[]; yaw: number }[];
  [k: string]: unknown;
}

/** One evaluated simcli expectation. */
export interface ExpectResult {
  expect: string;
  actual: string;
  passed: boolean;
}

/** scenario_result(): progress, and once done the headless simcli run summary. */
export interface ScenarioResult {
  name: string;
  done: boolean;
  tick: number;
  end_tick: number;
  pass: boolean;
  live_hash: string | null;
  matches_simcli: boolean | null;
  summary: {
    final_hash: string;
    expectations: ExpectResult[];
    pending: ExpectResult[];
    failures: string[];
    warnings: string[];
  } | null;
}

export interface ReplayStatus {
  tick: number;
  end_tick: number;
  speed: number;
  verified_ticks: number;
  drift_count: number;
  drift_ticks: number[];
  state_hash: string;
  recorded_final_hash: string | null;
  scenario: string;
  mode: string;
  contract_version: string;
  hash_spec: number;
}

/** SimEvent as decoded by events_decode (serde externally tagged). */
export type SimEvent = Record<string, Record<string, unknown>>;

export async function loadWasm(): Promise<void> {
  await initWasm();
}

export function packInput(i: DriverInput): Float32Array {
  const flags = (i.gearUp ? 1 : 0) | (i.gearDown ? 2 : 0) | (i.horn ? 4 : 0) | (i.headlights ? 8 : 0);
  return new Float32Array([i.steer, i.throttle, i.brake, i.handbrake, i.clutch, flags, 0]);
}

export const sim = {
  /** Start a live session from scenario JSON; `scripted` = drive entities by their drivers. */
  init: (seed: number, scenarioJson: string, scripted: boolean): number[] =>
    Array.from(wasm.init(seed, scenarioJson, scripted)),
  freeDriveJson: (car: string, mode: string, city: boolean): string => wasm.free_drive_json(car, mode, city),
  cityChaseJson: (car: string, mode: string): string => wasm.city_chase_json(car, mode),
  /** Free drive / city chase in a city by id ("vesper_v2" | "procedural_12x12"). */
  freeDriveCityJson: (car: string, mode: string, cityId: string): string => wasm.free_drive_city_json(car, mode, cityId),
  cityChaseCityJson: (car: string, mode: string, cityId: string): string => wasm.city_chase_city_json(car, mode, cityId),
  /** Id of the session's city ("" on the flat world): data/city/<id>.greybox.glb. */
  cityId: (): string => wasm.city_id(),
  scenarioFromRon: (text: string): string => wasm.scenario_from_ron(text),
  scenarioWithSpeed: (json: string, mps: number): string => wasm.scenario_with_speed(json, mps),
  step: (dt: number): number => wasm.step(dt),
  setDamageMode: (mode: string): void => wasm.set_damage_mode(mode),
  carIds: (): string[] => JSON.parse(wasm.car_ids()),
  spawn: (carId: string, pos: [number, number, number], yaw: number): number =>
    wasm.spawn_vehicle(carId, new Float32Array(pos), yaw),
  setInput: (entity: number, input: DriverInput): boolean => wasm.set_input(entity, packInput(input)),
  setAssists: (entity: number, p: AssistProfile): void => wasm.set_assist_profile(entity, JSON.stringify(p)),
  assistPreset: (source: "Keyboard" | "Gamepad" | "Wheel"): AssistProfile => JSON.parse(wasm.assist_preset(source)),
  assists: (entity: number): AssistProfile => JSON.parse(wasm.assist_profile(entity)),
  reset: (entity: number): void => wasm.reset_vehicle(entity),
  entities: (): number[] => Array.from(wasm.entities()),
  states: (): Float32Array => wasm.vehicle_states(),
  statesPrev: (): Float32Array => wasm.vehicle_states_prev(),
  info: (entity: number): VehicleInfo => JSON.parse(wasm.vehicle_info(entity)),
  worldInfo: (): Float32Array => wasm.world_info(),
  trafficAgents: (): Float32Array => wasm.traffic_agents(),
  /** Static signal heads of the city (layout.ts SH, SIGNAL_HEAD_STRIDE); fetch once per session. */
  signalHeads: (): Float32Array => wasm.signal_heads(),
  /** One state byte per signal head (layout.ts SIGNAL), same order as signalHeads(). */
  signalStates: (): Uint8Array => wasm.signal_states(),
  /** [entity, kind, flags] per sim vehicle (layout.ts META_*), same order as states(). */
  vehicleMeta: (): Float32Array => wasm.vehicle_meta(),
  /** Wanted level / heat for a racer, or null when the session has no police. */
  /** Police units that are not sim vehicles: [x, y, z, yaw, speed, flags] each. */
  policeUnits: (): Float32Array => wasm.police_units(),
  policeStatus: (entity: number): PoliceStatus | null => JSON.parse(wasm.police_status(entity)),
  stateHash: (): string => wasm.state_hash(),
  latticeNodes: (entity: number): Float32Array => wasm.lattice_nodes(entity),
  latticeRest: (entity: number): Float32Array => wasm.lattice_rest(entity),
  latticeBeams: (entity: number): Uint32Array => wasm.lattice_beams(entity),
  latticeRegions: (entity: number): Uint8Array => wasm.lattice_node_regions(entity),
  latticeStrain: (entity: number): Float32Array => wasm.lattice_strain(entity),
  regionEnergy: (entity: number): Float32Array => wasm.region_energy(entity),
  skinIndices: (entity: number): Uint32Array => wasm.skin_binding_indices(entity),
  skinWeights: (entity: number): Float32Array => wasm.skin_binding_weights(entity),
  eventsDrain: (): SimEvent[] => {
    const bytes = wasm.events_drain();
    // Empty Vec<SimEvent> encodes as a single 0 byte; skip the decode round trip.
    return bytes.length <= 1 ? [] : JSON.parse(wasm.events_decode(bytes));
  },
  telemetry: (entity: number): Float32Array => wasm.telemetry(entity),
  telemetryNames: (): string[] => JSON.parse(wasm.telemetry_names()),
  ffb: (entity: number): Float32Array => wasm.ffb(entity),
  intents: (): Float32Array => wasm.intents(),
  recordStart: (): void => wasm.record_start(),
  recordStop: (): Uint8Array => wasm.record_stop(),
  recording: (): boolean => wasm.recording(),
  replayLoad: (bytes: Uint8Array): number[] => Array.from(wasm.replay_load(bytes)),
  replaySeek: (tick: number): void => wasm.replay_seek(tick),
  replayPlay: (speed: number): void => wasm.replay_play(speed),
  replayStatus: (): ReplayStatus | null => JSON.parse(wasm.replay_status()),
  replayCsv: (bytes: Uint8Array, entity: number, everyNTicks: number): string =>
    wasm.replay_export_csv(bytes, entity, everyNTicks),
  scenario: (): Scenario => JSON.parse(wasm.scenario_json()),
  scenarioResult: (): ScenarioResult | null => JSON.parse(wasm.scenario_result()),
  contractVersion: (): string => wasm.contract_version(),
  /** Live repair bill for a vehicle (workstream 16; `garage.rs`). `requestJson` may be "". */
  repairQuote: (entity: number, requestJson: string): LiveBill => JSON.parse(wasm.repair_quote_json(entity, requestJson)),
  /** End-of-run result (`wp_wasm::run::RunReport`), null until a chase or the car ends. */
  runResult: (): RunReport | null => JSON.parse(wasm.run_result_json()),
  /** Every ledger line of the player's account (carries over restarts). */
  runLedger: (): LedgerLine[] => JSON.parse(wasm.run_ledger_json()),
  /** `{ cash, rep, bounty, debt }`, or null without an economy run. */
  runAccount: (): { cash: number; rep: number; bounty: number; debt: number } | null => JSON.parse(wasm.run_account_json()),
  /** "Repair & continue": pays impound + repair, repairs the car, releases the brakes. Throws if refused. */
  runRepairContinue: (): ContinueReport => JSON.parse(wasm.run_repair_continue()),
  /** Race picker for a player in `car` (`wp_wasm::race::RaceCard[]`). */
  raceList: (car: string, mode: string): RaceCard[] => JSON.parse(wasm.race_list(car, mode)),
  /** Start a picker race (new session; the account carries over). Returns entity ids, player first. */
  raceStart: (id: string, car: string, mode: string, autopilot: boolean): number[] =>
    Array.from(wasm.race_start(id, car, mode, autopilot)),
  /** `[phase, countdown_s, elapsed_s, laps, gates, player, racers, closed, length_m, autopilot, kind, player_done]`; empty without a race. */
  raceInfo: (): Float32Array => wasm.race_info(),
  /** Standings, position order, RACE_STRIDE floats each (see layout.ts RS). */
  raceStandings: (): Float32Array => wasm.race_standings(),
  /** Gates + route of the race (race_marker.ts parseCourse). */
  raceCourse: (): Float32Array => wasm.race_course(),
  /** Results once the player is done (`wp_wasm::race::RaceReport`), else null. */
  raceResult: (): RaceReport | null => JSON.parse(wasm.race_result_json()),
  /** Debug / e2e: set a racer's police heat (not recorded in replays). */
  debugSetHeat: (entity: number, heat: number): void => wasm.debug_set_heat(entity, heat),
  /** Debug / e2e: park `entity` stopped `aheadM` in front of the first police car (throws if none yet). */
  debugParkNearPolice: (entity: number, aheadM: number): number => wasm.debug_park_near_police(entity, aheadM),
};

export type { ContinueReport, LedgerLine, RunReport } from "../ui/runEnd";
export type { RaceCard, RaceReport } from "../ui/race";
import type { RaceCard, RaceReport } from "../ui/race";
import type { ContinueReport, LedgerLine, RunReport } from "../ui/runEnd";

/** `wp_wasm::garage::LiveBill` (typed in apps/ui-kit/src/live/bill.ts as LiveBillJson). */
export type LiveBill = import("../../../ui-kit/src/live/bill").LiveBillJson;
