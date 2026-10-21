// Typed wrapper over the wp_wasm surface. Per-frame calls return typed arrays (see layout.ts);
// JSON only for rare structured data.
import initWasm, * as wasm from "./pkg/wp_wasm.js";
import type { PoliceStatus } from "../police";
import type { BuildVisuals } from "../render/mod_parts";

/** Driver input in the testbed (packed to Float32Array for set_input). */
/** Result of `resetInPlace`. */
export interface ResetInPlace {
  x: number;
  y: number;
  yaw: number;
  /** Landed on a lane (false: set upright where it was, no lane within 15 m). */
  on_lane: boolean;
  repaired: boolean;
  /** Ghost window, physics ticks: the car passes through other cars, props and debris. */
  ghost_ticks: number;
}

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
  /** Strut compression at rest, m (the wheel centre sits at position_m there). Optional for
   * stand-in models (props); fall back to half the travel. */
  static_compression_m?: number;
  tire_id: string;
}

/** session_car_models() JSON (crates/wp_wasm/src/lod.rs `SessionModels`). */
export interface SessionModels {
  cars: string[];
  traffic: string | null;
  police: string | null;
  /** Model parked cars are drawn as and dent with (null / absent: none parked). */
  parked?: string | null;
}

/** car_model_info(car_id) JSON. */
export interface CarModelInfo {
  car_id: string;
  body_size_m: [number, number, number];
  wheels: VehicleInfo["wheels"];
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

/** TCS / ABS level (contract `TcsLevel` / `AbsLevel`). The sim also accepts a legacy bool
 * (true = Full, false = Off) but always returns a level. */
export type AssistLevel = "Off" | "Half" | "Full";

/** A level from anything stored or typed (legacy bool, any case); `d` otherwise. */
export function assistLevel(v: unknown, d: AssistLevel = "Off"): AssistLevel {
  if (v === true) return "Full";
  if (v === false) return "Off";
  if (typeof v === "string") {
    const l = v.trim().toLowerCase();
    if (l === "off" || l === "half" || l === "full") return (l[0].toUpperCase() + l.slice(1)) as AssistLevel;
  }
  return d;
}

export interface AssistProfile {
  abs: AssistLevel;
  tcs: AssistLevel;
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
  entities: { id: string; car: string; pos: number[]; yaw: number; mods?: string[] }[];
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

/** A detected big crash (crates/wp_wasm/src/crash_replay.rs `CrashMark`). */
export interface CrashMark {
  seq: number;
  /** Impact tick (strongest contact). */
  tick: number;
  player: number;
  /** Other entity, null = static world. */
  other: number | null;
  closing_mps: number;
  delta_v_mps: number;
  health_drop: number;
  /** World contact point (sim space). */
  point: [number, number, number];
  /** Unit vector car centre → contact point, chassis space. */
  dir_local: [number, number, number];
  side: "front" | "rear" | "left" | "right" | "top";
}

/** crash_replay_start() JSON (`CrashReplayInfo`). */
export interface CrashReplayInfo {
  crash: CrashMark;
  start_tick: number;
  end_tick: number;
  live_tick: number;
  live_hash: string;
}

/** Big-crash thresholds (`CrashTuning`). */
export interface CrashTuning {
  min_closing_mps: number;
  min_delta_v_mps: number;
  min_health_drop: number;
  window_ticks: number;
  cooldown_ticks: number;
}

/** One entry of `mod_catalog_json(car)`: `wp_economy::ModDef` plus the parts it draws. */
export interface ModEntry {
  id: string;
  name: string;
  slot: string;
  line: string;
  tier: number;
  price: number;
  requires?: string[];
  tradeoff: string;
  /** `[PartKind, style]` pairs (`wp_economy::visuals::mod_parts`); empty = nothing visible. */
  parts: [string, string][];
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

let lastEventsJson = "";

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
  /** Weather selector choices `[id, label]` (wp_wasm::weather::CHOICES). */
  weatherChoices: (): [string, string][] => JSON.parse(wasm.weather_choices()),
  /** Scenario JSON with a selector choice's weather ("none", "clear", …, "dynamic"). */
  scenarioWithWeather: (json: string, choice: string): string => wasm.scenario_with_weather(json, choice),
  /** The selector choice matching a scenario's weather ("none", a choice id or "custom"). */
  scenarioWeatherChoice: (json: string): string => wasm.scenario_weather_choice(json),
  /** "Rainy night" city chase JSON (downpour, soaked road). */
  cityChaseRainJson: (car: string, mode: string, cityId: string): string => wasm.city_chase_rain_json(car, mode, cityId),
  /** Per-frame weather state (render/weather.ts WX offsets). */
  weatherInfo: (): Float32Array => wasm.weather_info(),
  /** Wetness grid placement [origin_x, origin_y, cell_m, nx, ny] (empty without weather). */
  weatherGridMeta: (): Float32Array => wasm.weather_grid_meta(),
  /** Wetness grid, 2 bytes per cell: wetness × 255, standing water mm × 40. ~1 Hz. */
  weatherGrid: (): Uint8Array => wasm.weather_grid(),
  /** Spray 0..1 per sim vehicle, same order as states(). */
  weatherSpray: (): Float32Array => wasm.weather_spray(),
  scenarioWithSpeed: (json: string, mps: number): string => wasm.scenario_with_speed(json, mps),
  step: (dt: number): number => wasm.step(dt),
  setDamageMode: (mode: string): void => wasm.set_damage_mode(mode),
  carIds: (): string[] => JSON.parse(wasm.car_ids()),
  /** Mods the car's class may fit, with the parts each draws (no session needed). */
  modCatalog: (car: string): ModEntry[] => JSON.parse(wasm.mod_catalog_json(car)),
  /** What a mod set looks like on a car; throws the economy's reason for an illegal set. */
  buildVisuals: (car: string, mods: string[]): BuildVisuals => JSON.parse(wasm.build_visuals_json(car, JSON.stringify(mods))),
  /** Parts of a live entity's installed mods (scenario `mods`); null if unavailable. */
  entityVisuals: (entity: number): BuildVisuals | null => {
    try {
      return JSON.parse(wasm.entity_visuals_json(entity)) as BuildVisuals;
    } catch {
      return null;
    }
  },
  spawn: (carId: string, pos: [number, number, number], yaw: number): number =>
    wasm.spawn_vehicle(carId, new Float32Array(pos), yaw),
  setInput: (entity: number, input: DriverInput): boolean => wasm.set_input(entity, packInput(input)),
  setAssists: (entity: number, p: AssistProfile): void => wasm.set_assist_profile(entity, JSON.stringify(p)),
  assistPreset: (source: "Keyboard" | "Gamepad" | "Wheel"): AssistProfile => JSON.parse(wasm.assist_preset(source)),
  assists: (entity: number): AssistProfile => JSON.parse(wasm.assist_profile(entity)),
  /** Countersteer help slider, 0..100 % (owner row 57). */
  setCountersteer: (entity: number, percent: number): void => wasm.set_countersteer(entity, percent),
  countersteer: (entity: number): number => wasm.countersteer(entity),
  /** Drift Lab B: drift-assist constants in effect / shipped defaults (wp_vehicle::drift::DriftTuning). */
  driftTuning: (): Record<string, number | boolean> => JSON.parse(wasm.drift_tuning_json()),
  driftTuningDefaults: (): Record<string, number | boolean> => JSON.parse(wasm.drift_tuning_defaults_json()),
  /** Set the runtime override from JSON ("" = defaults). */
  driftTuningSet: (json: string): void => wasm.drift_tuning_set(json),
  /** Unify A/B (wp_vehicle::lab_scope): the pre-unify stock handling where no override is set. */
  labScopeSetLegacy: (on: boolean): void => wasm.lab_scope_set_legacy(on),
  labScopeLegacy: (): boolean => wasm.lab_scope_legacy(),
  /** Unify A/B: plain steering off for player cars too. */
  labScopeSetPlainOff: (on: boolean): void => wasm.lab_scope_set_plain_off(on),
  labScopePlainOff: (): boolean => wasm.lab_scope_plain_off(),
  /** Re-apply a vehicle's body inertia after a ride override (live and replay sessions). */
  refreshBodyInertia: (entity: number): void => wasm.refresh_body_inertia(entity),
  /** Whether a vehicle gets the player steering aids (player cars; not AI, police or traffic). */
  playerInput: (entity: number): boolean => wasm.player_input(entity),
  /** [mode, slide_deg, onset, envelope_deg, authority, throttle_scale, key_throttle, engine_throttle]; mode 0 off … 5 exceeded. */
  driftReadout: (entity: number): Float32Array => wasm.drift_readout(entity),
  /** Plain keyboard steering constants a player's car drives with (override or shipped; null = off / legacy) / the shipped defaults (wp_vehicle::plain::PlainSteer). */
  plainSteer: (): Record<string, number> | null => JSON.parse(wasm.plain_steer_json()),
  plainSteerDefaults: (): Record<string, number> => JSON.parse(wasm.plain_steer_defaults_json()),
  /** Set the plain steering override with these constants ("" = the shipped default; plain off: labScopeSetPlainOff). */
  plainSteerSet: (json: string): void => wasm.plain_steer_set(json),
  /** [steer_key, rack, lock_frac, slide_deg, throttle_key, engine_throttle, gear, speed_kmh, wheel_deg, heading_deg,
   * auto_clutch (1/0), clutch engagement 0..1, clutch_locked (1/0), engine_rpm, driven_wheel_rpm, clutch_pedal]. */
  plainReadout: (entity: number): Float32Array => wasm.plain_readout(entity),
  /** Drift Lab W ride tuning in effect (null = car data) / {data, lab} defaults (wp_vehicle::ride_lab::RideTune). */
  rideTune: (): Record<string, number> | null => JSON.parse(wasm.ride_tune_json()),
  rideTuneDefaults: (): { data: Record<string, number>; lab: Record<string, number>; slow_weight: Record<string, number>; medium_weight: Record<string, number>; firm: Record<string, number>; owner: Record<string, number>; shipped: Record<string, number> } =>
    JSON.parse(wasm.ride_tune_defaults_json()),
  /** Set the ride tuning override ("" = the shipped ride). */
  rideTuneSet: (json: string): void => wasm.ride_tune_set(json),
  /** [roll_hz, roll_zeta, roll_deg_per_g, pitch_hz, pitch_zeta, body_roll_deg]. */
  rideReadout: (entity: number): Float32Array => wasm.ride_readout(entity),
  /** Drift Lab W car / tyre experiments in effect (null = data) / {data, spin_damper, spin} (wp_vehicle::car_lab::CarTune). */
  carTune: (): Record<string, number | boolean> | null => JSON.parse(wasm.car_tune_json()),
  carTuneDefaults: (): {
    data: Record<string, number | boolean>;
    spin_damper: Record<string, number | boolean>;
    spin: Record<string, number | boolean>;
    spin_lock: Record<string, number | boolean>;
    spin_vr_plain: Record<string, number>;
    transition_plain: Record<string, number>;
    spin_v2: Record<string, number | boolean>;
    spin_v2_plain: Record<string, number>;
    step22: Record<string, number | boolean>;
    shipped: Record<string, number | boolean>;
  } =>
    JSON.parse(wasm.car_tune_defaults_json()),
  /** Set the car / tyre override ("" = the shipped tyres). */
  carTuneSet: (json: string): void => wasm.car_tune_set(json),
  /** Grip-balance lab in effect (null = data) / {before, new} (wp_vehicle::grip_lab::GripBalance). */
  gripBalance: (): Record<string, number | boolean | string> | null => JSON.parse(wasm.grip_balance_json()),
  gripBalanceDefaults: (): {
    before: Record<string, number | boolean | string>;
    new: Record<string, number | boolean | string>;
    grip: Record<string, number | boolean | string>;
    step14: Record<string, number | boolean | string>;
    step2: Record<string, number | boolean | string>;
    slow_weight: Record<string, number | boolean | string>;
    drift_best: Record<string, number | boolean | string>;
    lock_only: Record<string, number | boolean | string>;
    shipped: Record<string, number | boolean | string>;
    owner_ref: Record<string, number | boolean | string>;
  } =>
    JSON.parse(wasm.grip_balance_defaults_json()),
  /** Set the grip-balance override ("" = the shipped balance). */
  gripBalanceSet: (json: string): void => wasm.grip_balance_set(json),
  /** Drift Lab W reference-parity test cars (not in carIds()). */
  parityCarIds: (): string[] => JSON.parse(wasm.parity_car_ids()),
  /** Clutch mode "auto" | "manual" (owner row 59). */
  setClutchMode: (entity: number, mode: "auto" | "manual"): void => wasm.set_clutch_mode(entity, mode),
  clutchMode: (entity: number): string => wasm.clutch_mode(entity),
  engineStalled: (entity: number): boolean => wasm.engine_stalled(entity),
  /** One-click presets in menu order: ["Rookie", "Street", "Sim"]. */
  assistPresets: (): string[] => JSON.parse(wasm.assist_presets()),
  /** Damage mode + assists of a one-click preset for a driver on `source`. */
  assistPresetNamed: (
    name: string,
    source: "Keyboard" | "Gamepad" | "Wheel",
  ): { mode: "Arcade" | "Simulation"; assists: AssistProfile } => JSON.parse(wasm.assist_preset_named(name, source)),
  /** The preset a vehicle's assists + the session's damage mode match ("" = customised). */
  assistPresetOf: (entity: number, source: "Keyboard" | "Gamepad" | "Wheel"): string =>
    wasm.assist_preset_of(entity, source),
  /** Back to the spawn pose, pristine (scenario restart). */
  reset: (entity: number): void => wasm.reset_vehicle(entity),
  /** Reset in place (R): upright at rest on the nearest clear lane within 15 m, heading its own
   * way; repaired unless a career run is active. Heat, pursuit and race progress carry on. */
  resetInPlace: (entity: number): ResetInPlace => JSON.parse(wasm.reset_in_place(entity)),
  /** Physics ticks the vehicle stays a ghost after a reset in place (0 = solid). */
  ghostTicks: (entity: number): number => wasm.ghost_ticks(entity),
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
  /** Roadblock barriers and spike strips as oriented boxes (layout.ts TOOL_STRIDE, TL, TOOL_KIND). */
  policeTools: (): Float32Array => wasm.police_tools(),
  /** Tyre deflation per vehicle, 4 floats each (FL, FR, RL, RR), same order as states(). */
  tirePunctures: (): Float32Array => wasm.tire_punctures(),
  /** Every traffic agent at any LOD (layout.ts DRAW_STRIDE / D): id, entity, ground-seated pose, flags. */
  trafficDraw: (): Float32Array => wasm.traffic_draw(),
  /** Every police unit, bound or not (layout.ts DRAW_STRIDE / D). */
  policeDraw: (): Float32Array => wasm.police_draw(),
  /** Every city prop once per session (layout.ts PROP_STRIDE / P): kind, box-centre pose, half extents. */
  propsStatic: (): Float32Array => wasm.props_static(),
  /** Props whose pose changed since the last call (layout.ts PROP_MOVED_STRIDE / PM). Once per frame. */
  propsMoved: (): Float32Array => wasm.props_moved(),
  /** The sim's pose of one prop `[x, y, z, qx, qy, qz, qw]` (automation). */
  propPose: (index: number): Float32Array => wasm.prop_pose(index),
  /** Dented parked cars (layout.ts WRECK_STRIDE / WK), ascending prop index. Once per frame. */
  parkedWrecks: (): Float32Array => wasm.parked_wrecks(),
  /** Entity id of prop 0's dented parked car; prop i is this + i (lattice_* / vehicle_info). */
  parkedEntityBase: (): number => wasm.parked_entity_base(),
  /** Region indices (vehicle_info().regions) of a dented parked car's shattered glass. */
  parkedWreckShattered: (entity: number): Uint32Array => wasm.parked_wreck_shattered(entity),
  /** Car models this session can show: every id, plus the traffic and police models (or null). */
  sessionCarModels: (): SessionModels => JSON.parse(wasm.session_car_models()),
  /** Body size and wheels of a car model (no vehicle needed). */
  carModelInfo: (carId: string): CarModelInfo => JSON.parse(wasm.car_model_info(carId)),
  /** Tracking helicopters: [x, y, z, aimX, aimY, aimZ, coneHalfAngle, flags (1 visual)] each. */
  policeHelicopters: (): Float32Array => wasm.police_helicopters(),
  policeStatus: (entity: number): PoliceStatus | null => JSON.parse(wasm.police_status(entity)),
  /** Static city map for the UI kit's minimap / full map (`wp_wasm::map`); `null` on the flat world. Once per city. */
  mapStaticJson: (): string => wasm.map_static_json(),
  /** Per-frame map data for `player` (units, tools, search zone; layout in ui-kit `src/map/data.ts`). */
  mapDynamic: (player: number): Float32Array => wasm.map_dynamic(player),
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
    lastEventsJson = bytes.length <= 1 ? "" : wasm.events_decode(bytes);
    return lastEventsJson ? JSON.parse(lastEventsJson) : [];
  },
  /** JSON text of the last `eventsDrain()` ("" when it had no events): the audio director takes
   * the same JSON, so the frame loop passes this instead of re-stringifying the events. */
  lastEventsJson: (): string => lastEventsJson,
  /** Panel states of an entity, PANEL_STRIDE floats per panel (render/panels.ts). */
  panelStates: (entity: number): Float32Array => wasm.panel_states(entity),
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
  /** Big crash detected since the last call (live sessions), or null. */
  crashTake: (): CrashMark | null => JSON.parse(wasm.crash_take()),
  crashTuning: (): CrashTuning => JSON.parse(wasm.crash_tuning_json()),
  crashTuningSet: (t: Partial<CrashTuning>): void => wasm.crash_tuning_set(JSON.stringify(t)),
  /** Replay buffer + crash detection on/off for this session (off for benches). */
  crashDetectEnable: (on: boolean): void => wasm.crash_detect_enable(on),
  crashBufferTicks: (): number[] => Array.from(wasm.crash_buffer_ticks()),
  /** Park the live session and play crash `seq` (impact − 3 s … + 2 s) as a replay. */
  crashReplayStart: (seq: number): CrashReplayInfo => JSON.parse(wasm.crash_replay_start(seq)),
  crashReplayActive: (): boolean => wasm.crash_replay_active(),
  /** Back to the parked live session, exactly as it was: its tick and state hash. */
  crashReplayStop: (): { tick: number; hash: string } | null => JSON.parse(wasm.crash_replay_stop()),
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
  raceStart: (id: string, car: string, mode: string, autopilot: boolean, tune?: CarTune): number[] =>
    Array.from(wasm.race_start_tuned(id, car, mode, autopilot, tune ? JSON.stringify(tune) : "")),
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
  /** Aim `entity` at a parked car (`prop` < 0: the nearest) at `speed` m/s; returns the prop index. */
  debugKnockProp: (entity: number, prop: number, speed: number): number => wasm.debug_knock_prop(entity, prop, speed),

  // ----- career (machine G; crates/wp_wasm/src/career.rs). Every call throws on refusal. ---
  careerNew: (mode: string): CareerView => JSON.parse(wasm.career_new(mode)),
  /** New career with a custom opening balance (preview / demo builds start rich). */
  careerNewWithCash: (mode: string, cash: number): CareerView => JSON.parse(wasm.career_new_with_cash(mode, cash)),
  careerLoad: (text: string): CareerView => JSON.parse(wasm.career_load_json(text)),
  careerSave: (): string => wasm.career_save_json(),
  careerExists: (): boolean => wasm.career_exists(),
  careerView: (): CareerView => JSON.parse(wasm.career_view_json()),
  careerMods: (uid: number): ModTreeView => JSON.parse(wasm.career_mods_json(uid)),
  careerRunActive: (): boolean => wasm.career_run_active(),
  /** Start a career run of a scenario (the first entity becomes the career car). */
  careerStartRun: (scenarioJson: string): number[] => Array.from(wasm.career_start_run(scenarioJson)),
  /** End the career run (back to the garage; one night passes). */
  careerEndRun: (): RunClose | null => JSON.parse(wasm.career_end_run()),
  /** The bust settled on load because the game was closed during a pursuit (B05), once. */
  careerTakeAbandoned: (): RunClose | null => JSON.parse(wasm.career_take_abandoned_json()),
  careerBuy: (defId: string, condition: string): CareerView => JSON.parse(wasm.career_buy_car(defId, condition)),
  careerFloorCar: (): CareerView => JSON.parse(wasm.career_floor_car()),
  careerSell: (uid: number): CareerView => JSON.parse(wasm.career_sell_car(uid)),
  careerSetActive: (uid: number): CareerView => JSON.parse(wasm.career_set_active(uid)),
  careerInstall: (uid: number, modId: string): CareerView => JSON.parse(wasm.career_install_mod(uid, modId)),
  careerUninstall: (uid: number, modId: string): CareerView => JSON.parse(wasm.career_uninstall_mod(uid, modId)),
  /** Set a career car's free tune (no money). */
  careerSetTune: (uid: number, tune: CarTune): CareerView => JSON.parse(wasm.career_set_tune(uid, JSON.stringify(tune))),
  careerQuote: (uid: number, req: ShopRequest): RepairQuote => JSON.parse(wasm.career_quote_json(uid, JSON.stringify(req))),
  careerRepair: (uid: number, req: ShopRequest): CareerView => JSON.parse(wasm.career_repair(uid, JSON.stringify(req))),
  careerRelease: (uid: number): CareerView => JSON.parse(wasm.career_release(uid)),
  careerInsure: (uid: number, tier: string): CareerView => JSON.parse(wasm.career_insure(uid, tier)),
  careerNextNight: (): CareerView => JSON.parse(wasm.career_next_night()),
  /** Put a police helicopter on a pursued racer now, offsetM away at altitude. Returns its unit id. */
  debugLaunchHelicopter: (entity: number, offsetM: number): number => wasm.debug_launch_helicopter(entity, offsetM),
};

import type { CarTune, CareerView, ModTreeView, RepairQuote, RunClose, ShopRequest } from "../../../ui-kit/src/garage/types";

export type { ContinueReport, LedgerLine, RunReport } from "../ui/runEnd";
export type { RaceCard, RaceReport } from "../ui/race";
import type { RaceCard, RaceReport } from "../ui/race";
import type { ContinueReport, LedgerLine, RunReport } from "../ui/runEnd";

/** `wp_wasm::garage::LiveBill` (typed in apps/ui-kit/src/live/bill.ts as LiveBillJson). */
export type LiveBill = import("../../../ui-kit/src/live/bill").LiveBillJson;
