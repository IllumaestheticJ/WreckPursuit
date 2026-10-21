// Typed wrapper over the wp_wasm surface. Per-frame calls return typed arrays (see layout.ts);
// JSON only for rare structured data.
import initWasm, * as wasm from "./pkg/wp_wasm.js";
import type { PoliceStatus } from "../police";

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
  /** Menu stat bars for every car (`wp_wasm::car_stats_json`, no session needed). */
  carStats: (): CarStatsTable => JSON.parse(wasm.car_stats_json()),
  spawn: (carId: string, pos: [number, number, number], yaw: number): number =>
    wasm.spawn_vehicle(carId, new Float32Array(pos), yaw),
  setInput: (entity: number, input: DriverInput): boolean => wasm.set_input(entity, packInput(input)),
  setAssists: (entity: number, p: AssistProfile): void => wasm.set_assist_profile(entity, JSON.stringify(p)),
  assistPreset: (source: "Keyboard" | "Gamepad" | "Wheel"): AssistProfile => JSON.parse(wasm.assist_preset(source)),
  assists: (entity: number): AssistProfile => JSON.parse(wasm.assist_profile(entity)),
  /** Countersteer help slider, 0..100 % (owner row 57). */
  setCountersteer: (entity: number, percent: number): void => wasm.set_countersteer(entity, percent),
  countersteer: (entity: number): number => wasm.countersteer(entity),
  /** Clutch mode "auto" | "manual" (owner row 59). */
  setClutchMode: (entity: number, mode: "auto" | "manual"): void => wasm.set_clutch_mode(entity, mode),
  clutchMode: (entity: number): string => wasm.clutch_mode(entity),
  engineStalled: (entity: number): boolean => wasm.engine_stalled(entity),
  /** Drift-assist tuning (drift lab dev override; 11 values, see wp_wasm DRIFT_TUNING_FIELDS). */
  setDriftTuning: (entity: number, values: Float32Array): void => wasm.set_drift_tuning(entity, values),
  driftTuning: (entity: number): number[] => Array.from(wasm.drift_tuning(entity)),
  /** [body slip °, assist mode (0 off, 1 straighten, 2 catch, 3 hold, 4 exceeded), envelope °, throttle smoothing in effect]. */
  driftState: (entity: number): number[] => Array.from(wasm.drift_state(entity)),
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
  /** Car models this session can show: every id, plus the traffic and police models (or null). */
  sessionCarModels: (): SessionModels => JSON.parse(wasm.session_car_models()),
  /** Body size and wheels of a car model (no vehicle needed). */
  carModelInfo: (carId: string): CarModelInfo => JSON.parse(wasm.car_model_info(carId)),
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
  /** Aim `entity` at a parked car (`prop` < 0: the nearest) at `speed` m/s; returns the prop index. */
  debugKnockProp: (entity: number, prop: number, speed: number): number => wasm.debug_knock_prop(entity, prop, speed),

  // ----- career (machine G; crates/wp_wasm/src/career.rs). Every call throws on refusal. ---
  careerNew: (mode: string): CareerView => JSON.parse(wasm.career_new(mode)),
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
  careerQuote: (uid: number, req: ShopRequest): RepairQuote => JSON.parse(wasm.career_quote_json(uid, JSON.stringify(req))),
  careerRepair: (uid: number, req: ShopRequest): CareerView => JSON.parse(wasm.career_repair(uid, JSON.stringify(req))),
  careerRelease: (uid: number): CareerView => JSON.parse(wasm.career_release(uid)),
  careerInsure: (uid: number, tier: string): CareerView => JSON.parse(wasm.career_insure(uid, tier)),
  careerNextNight: (): CareerView => JSON.parse(wasm.career_next_night()),
};

import type { CareerView, ModTreeView, RepairQuote, RunClose, ShopRequest } from "../../../ui-kit/src/garage/types";

export type { ContinueReport, LedgerLine, RunReport } from "../ui/runEnd";
export type { RaceCard, RaceReport } from "../ui/race";
import type { RaceCard, RaceReport } from "../ui/race";
import type { ContinueReport, LedgerLine, RunReport } from "../ui/runEnd";

/** `simcli::car_stats::CarStatsTable` (data/stats/cars.json): bars are 0–1 on fixed scales. */
export interface CarStatsTable {
  scales: { zero_to_100_s: [number, number]; top_speed_kmh: [number, number]; cornering_g: [number, number]; skidpad_radius_m: number };
  cars: Record<
    string,
    {
      display_name: string;
      zero_to_100_s: number | null;
      top_speed_kmh: number;
      cornering_g: number;
      bars: { acceleration: number; top_speed: number; cornering: number };
    }
  >;
}

/** `wp_wasm::garage::LiveBill` (typed in apps/ui-kit/src/live/bill.ts as LiveBillJson). */
export type LiveBill = import("../../../ui-kit/src/live/bill").LiveBillJson;
