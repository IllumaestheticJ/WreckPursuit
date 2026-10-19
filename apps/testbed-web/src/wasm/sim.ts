// Typed wrapper over the wp_wasm surface. Per-frame calls return typed arrays (see layout.ts);
// JSON only for rare structured data.
import initWasm, * as wasm from "./pkg/wp_wasm.js";

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

/** Testbed scenario (superset of the simcli .ron format). */
export interface Scenario {
  name: string;
  car: string;
  mode: "Arcade" | "Simulation";
  seed: number;
  duration_s: number;
  time_of_day_h: number;
  cruise_kmh: number | null;
  others: { car: string; pos: number[]; yaw: number; cruise_kmh: number | null }[];
  barriers: Barrier[];
  [k: string]: unknown;
}

export interface CheckResult {
  metric: string;
  min: number;
  max: number;
  value: number | null;
  pass: boolean;
  pending: boolean;
}

export interface ScenarioResult {
  name: string;
  done: boolean;
  tick: number;
  end_tick: number;
  pass: boolean;
  checks: CheckResult[];
  state_hash: number;
}

export interface ReplayStatus {
  tick: number;
  start_tick: number;
  end_tick: number;
  speed: number;
  verified_hashes: number;
  stored_hashes: number;
  drift_ticks: number[];
  state_hash: string;
  recorded_final_hash: string | null;
  scenario: string;
  mode: string;
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
  init: (seed: number, scenarioJson = ""): number[] => Array.from(wasm.init(seed, scenarioJson)),
  scenarioFromRon: (text: string): Scenario => JSON.parse(wasm.scenario_from_ron(text)),
  step: (dt: number): number => wasm.step(dt),
  setDamageMode: (mode: string): number[] => Array.from(wasm.set_damage_mode(mode)),
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
};
