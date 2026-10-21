/**
 * The UI state tree. Every screen renders from one slice of [`UiState`]; every user action is a
 * [`UiCommand`](./commands.ts). A provider (the mock, later the real sim/net bridge) owns the
 * state and applies commands — screens never mutate anything themselves.
 *
 * Field comments name the crate that will own the data. `docs/integration/ui.md` has the full
 * table, including what is missing from the contract today.
 */
import type {
  AssistProfile,
  BodyRegion,
  Component,
  DamageMode,
  RepairEstimate,
  Role,
  VehicleClass,
} from "../generated/contracts";
import type { InputBindings } from "./bindings";
import type { WantedLevel } from "../generated/contracts";
import type { District, PursuitStatus, ToolReadiness, UnitKind, UnitOrder } from "./police";

/** Every screen the kit can show. */
export type ScreenId =
  | "mainMenu"
  | "hud"
  | "pause"
  | "settings"
  | "garage"
  | "modTree"
  | "market"
  | "lobby"
  | "results"
  | "impound"
  | "crewMap"
  | "police"
  | "captain"
  | "scanner"
  | "replay"
  | "chaseHud"
  | "repairBill";

/** World position on the 2D map plane, metres (+X east, +Y north). */
export type Vec2 = [number, number];

// ---------------------------------------------------------------- HUD

/** Race progress shown on the HUD (owner: race events, prompt 15; not in the contract yet). */
export interface RaceHud {
  /** 1-based position. */
  position: number;
  /** Number of racers. */
  of: number;
  lap: number;
  laps: number;
  checkpoint: number;
  checkpoints: number;
  /** Gap to the car ahead, s (null when leading). */
  gapAheadS: number | null;
  /** Gap to the car behind, s (null when last). */
  gapBehindS: number | null;
  elapsedS: number;
  /** Next checkpoint, world position. */
  nextCheckpoint: Vec2;
}

/** A blip on the minimap / captain map. */
export interface MapBlip {
  id: number;
  kind: "player" | "racer" | "police" | "helicopter" | "traffic" | "checkpoint" | "incident";
  pos: Vec2;
  headingRad?: number;
}

/** One line in the scanner feed / HUD alert stack. */
export interface ScannerEntry {
  /** Monotonic id (for keys and dedup). */
  id: number;
  /** Physics tick the entry was produced at. */
  tick: number;
  /** Simulation time, s. */
  tS: number;
  /** Who is talking. `dispatch` = police radio, `unit` = a police unit, `system` = game. */
  channel: "dispatch" | "unit" | "civil" | "system";
  severity: "info" | "warn" | "alert";
  text: string;
  /** Entity the entry is about, if any. */
  entity?: number;
}

/** Police heat as the HUD shows it (owner: wp_police). */
export interface HeatHud {
  level: WantedLevel;
  /** Heat 0..=1 within the current level. */
  heat: number;
  /** `clear`/`noticed`/`pursuit`/`cooldown` ↔ `wp_police::PursuitStatus` Clear/Wanted/Pursuit/Searching. */
  state: "clear" | "noticed" | "pursuit" | "cooldown";
  /**
   * Cooldown progress 0..=1 (1 = escaped), only meaningful in `cooldown`:
   * time without line of sight ÷ escape time for the level and day phase.
   */
  cooldown: number;
  /** Units actively chasing the player. */
  unitsInPursuit: number;
  bounty: number;
  /** False when the source only knows the level (events), not the heat value (default true). */
  heatKnown?: boolean;
  /** False when the source has no bounty figure (default true). */
  bountyKnown?: boolean;
}

/** Everything the in-race HUD shows for the local player's car. */
export interface HudState {
  /** Player's vehicle `EntityId`. */
  entity: number;
  /** Speed, m/s (owner: wp_vehicle via `VehicleState`). */
  speedMps: number;
  engineRpm: number;
  redlineRpm: number;
  /** -1 reverse, 0 neutral, 1.. forward. */
  gear: number;
  /** Global health 0..=1. */
  health: number;
  /** Region damage severity 0..=1 (owner: wp_damage; `RegionDamaged` in v0.2.0). */
  regions: Partial<Record<BodyRegion, number>>;
  /** Regions whose detachable parts fell off. */
  detached: BodyRegion[];
  /** Component health 0..=1; absent = 1. */
  components: Partial<Record<Component, number>>;
  failed: Component[];
  wheelsDetached: number[];
  totalLoss: boolean;
  heat: HeatHud;
  /** Player position and heading for the minimap. */
  pos: Vec2;
  headingRad: number;
  blips: MapBlip[];
  race: RaceHud | null;
  assists: AssistProfile;
  damageMode: DamageMode;
  /** In-game clock, seconds since midnight (owner: wp_world). */
  timeOfDayS: number;
}

// ---------------------------------------------------------------- settings

/** Colour palettes. The non-default ones remap danger/ok/warn hues (see theme.css). */
export type Palette = "default" | "deuteranopia" | "protanopia" | "tritanopia" | "highContrast";

/** Named assist presets; `custom` once any toggle differs from a preset. */
export type AssistPreset = "off" | "sim" | "standard" | "full" | "custom";

/** Player settings (local only; persisted by the host app). */
export interface Settings {
  bindings: InputBindings;
  assistPreset: AssistPreset;
  assists: AssistProfile;
  damageMode: DamageMode;
  units: "metric" | "imperial";
  /** Analog input shaping, applied by the host before building `VehicleInput`. */
  input: {
    /** Steering deadzone 0..=0.3 (fraction of travel). */
    steerDeadzone: number;
    /** Steering response exponent 1 (linear) ..= 3. */
    steerLinearity: number;
    /** Physical wheel rotation lock-to-lock, degrees (wheel only). */
    wheelRotationDeg: number;
    /** Throttle and brake share one axis (older pedal sets / triggers). */
    combinedPedals: boolean;
  };
  accessibility: {
    palette: Palette;
    /** CSS scale factor for the whole UI, 0.75..=1.5. */
    uiScale: number;
    reduceMotion: boolean;
    /** Larger, slower scanner text. */
    largeScannerText: boolean;
  };
  feedback: {
    rumble: boolean;
    rumbleStrength: number;
    /** Wheel force feedback gain 0..=1 (`wp_vehicle::VehicleSim::ffb`). */
    ffbStrength: number;
  };
  audio: { master: number; engine: number; scanner: number; music: number };
}

// ---------------------------------------------------------------- economy

/** Player account summary (owner: wp_economy `PlayerAccount`). */
export interface AccountView {
  cash: number;
  rep: number;
  bounty: number;
  /** In-game day. */
  day: number;
  copXp: number;
  copLevel: number;
  inDebt: boolean;
}

/** Insurance as shown in the garage (owner: wp_economy `InsurancePolicy`). */
export interface InsuranceView {
  tier: string;
  coverage: number;
  deductible: number;
  premiumPerDay: number;
}

/** One owned car (owner: wp_economy `OwnedCar` + valuation). */
export interface GarageCar {
  uid: number;
  defId: string;
  name: string;
  class: VehicleClass;
  basePrice: number;
  mods: string[];
  health: number;
  totalLoss: boolean;
  drivable: boolean;
  location: { kind: "Garage" } | { kind: "Impound"; sinceDay: number; level: WantedLevel };
  insurance: InsuranceView | null;
  /** Repair line items not yet paid. */
  outstanding: RepairEstimate;
  sustainedWhileWanted: boolean;
  /** Current value (damage + mods), from `Economy::current_value`. */
  value: number;
  /**
   * Stat bars of this car as built, physics-measured (`wp_wasm::car_stats_build_json`), when the
   * provider can run the sim. Absent → the kit estimates from the stock table (`data/stats.ts`).
   */
  stats?: import("../data/catalog").CarStats | null;
}

/** Repair decision options (`wp_economy::RepairOption`). */
export type RepairChoice =
  | { kind: "RepairAll" }
  | { kind: "RepairSelected"; lines: number[] }
  | { kind: "DriveDamaged" }
  | { kind: "Scrap" };

/** Itemised bill line after shop/district/mode pricing. */
export interface BillLine {
  label: string;
  severity: number;
  parts: number;
  labourHours: number;
  labour: number;
  total: number;
}

/** A priced repair quote (owner: wp_economy `quote_repair`). */
export interface RepairQuote {
  lines: BillLine[];
  subtotal: number;
  /** Mode `repair_cost_scale` applied. */
  modeScale: number;
  insuranceCovered: number;
  insuranceDenied: boolean;
  /** What the player pays. */
  total: number;
  /** Salvage value if scrapped. */
  scrapValue: number;
  affordable: boolean;
}

/** A garage/repair shop. */
export interface ShopView {
  id: string;
  name: string;
  kind: "legit" | "crooked";
  district: string;
  labourMultiplier: number;
}

/** One car offered in the market. */
export interface MarketListing {
  defId: string;
  name: string;
  analogOf: string;
  class: VehicleClass;
  condition: "New" | "Used" | "Worn";
  price: number;
  classIndex: number;
  bracket: string;
}

/** What the repair-bill screen asks the economy to price. */
export interface BillRequest {
  shop: "Legit" | "Crooked";
  /** District id for the labour multiplier (`prices.toml` `[district_labour]`). */
  district: string;
  option: "RepairAll" | "DriveDamaged" | "Scrap";
  insurance: "basic" | "full" | null;
  /** Wanted level of the car (crooked markup / legit shops report). */
  wanted: WantedLevel;
}

/** One priced line of the bill (`wp_economy::QuotedLine`). */
export interface BillLineView {
  label: string;
  severity: number;
  parts: number;
  labour: number;
  total: number;
}

/**
 * The repair bill for the car the player is driving (owner: `wp_damage` estimate priced by
 * `wp_economy::Economy::quote_repair`; in the testbed via `wp_wasm::repair_quote_json`).
 */
export interface RepairBillView {
  /** `wp_economy` (live) or the UI's TS port (mock). */
  source: "wp_economy" | "mock";
  carName: string;
  carClass: string;
  health: number;
  totalLoss: boolean;
  mode: string;
  lines: BillLineView[];
  labourMultiplier: number;
  gross: number;
  insuranceCovers: number;
  claimDenied: boolean;
  playerPays: number;
  salvage: number;
  reportsToPolice: boolean;
  /** Why there is no priced quote ("nothing to repair"…), else null. */
  error: string | null;
  request: BillRequest;
}

// ---------------------------------------------------------------- races

/** Kinds of race (owner: prompt 15 race events; economy stakes use these ids). */
export type RaceKind = "Sprint" | "Circuit" | "Drag" | "Meet" | "PinkSlip";

/** A participant in the lobby. */
export interface LobbyEntrant {
  player: number;
  name: string;
  role: Role;
  car: string | null;
  classIndex: number | null;
  ready: boolean;
  isLocal: boolean;
  crew: string | null;
}

/** Race lobby / street meet (owner: wp_net roles + prompt 15). */
export interface LobbyState {
  eventName: string;
  kind: RaceKind;
  location: string;
  laps: number;
  bracket: string;
  maxClassIndex: number;
  buyIn: number;
  purse: number;
  damageMode: DamageMode;
  policeEnabled: boolean;
  entrants: LobbyEntrant[];
  /** Seconds to start when every racer is ready, else null. */
  countdownS: number | null;
  maxRacers: number;
  roleSlots: Record<Role, number>;
}

/** Result row. */
export interface ResultRow {
  position: number;
  name: string;
  car: string;
  timeS: number | null;
  bestLapS: number | null;
  status: "Finished" | "DNF" | "Busted" | "Wrecked";
  isLocal: boolean;
}

/** One payout line (from typed `EconomyEvent`s). */
export interface PayoutLine {
  label: string;
  cash: number;
  rep: number;
  bounty: number;
}

/** Post-race results + payout (owner: race events + wp_economy `settle_race`). */
export interface ResultsState {
  eventName: string;
  rows: ResultRow[];
  payout: PayoutLine[];
  damageEstimate: number;
  cashBefore: number;
  cashAfter: number;
}

// ---------------------------------------------------------------- impound

/** One car in the police pound (owner: wp_economy capture/impound). */
export interface ImpoundEntry {
  carUid: number;
  name: string;
  daysIn: number;
  fee: number;
  feePerDay: number;
  auctionInDays: number;
  level: WantedLevel;
  reason: string;
}

/** Impound screen. */
export interface ImpoundState {
  entries: ImpoundEntry[];
  finesOwed: number;
  lastCapture: { level: WantedLevel; fine: number; bail: number; repLost: number; fate: string } | null;
}

// ---------------------------------------------------------------- crews / territory

/** A crew (owner: prompt 15 crews; not in the contract). */
export interface Crew {
  id: string;
  name: string;
  tag: string;
  /** Hue 0..360 used to tint territory. */
  hue: number;
  rep: number;
  members: number;
  isPlayers: boolean;
}

/** One district/zone of the territory map. */
export interface Territory {
  zoneId: number;
  kind: string;
  polygon: Vec2[];
  owner: string | null;
  /** Control 0..=1 for the owner. */
  control: number;
  contested: boolean;
}

/** Crew & territory screen. */
export interface CrewState {
  crews: Crew[];
  territories: Territory[];
  pois: { kind: string; name: string; pos: Vec2 }[];
}

// ---------------------------------------------------------------- police

/** Police career (owner: wp_economy cop XP + wp_police unlocks). */
export interface PoliceCareer {
  rank: string;
  level: number;
  xp: number;
  xpToNext: number;
  unlocks: { id: string; name: string; level: number; unlocked: boolean; description: string }[];
  stats: { busts: number; escapes: number; pursuits: number; damageCaused: number };
  orders: ScannerEntry[];
  /** Current assignment from the captain, if any. */
  assignment: string | null;
}

/** A police unit on the captain map (from `wp_police::captain::UnitView`). */
export interface PoliceUnit {
  id: number;
  callsign: string;
  kind: UnitKind;
  /** `Assignment::name()`: idle, patrol, respond, pursue, intercept, roadblock, spikes, helo-track, out-of-service. */
  assignment: string;
  pos: Vec2;
  headingRad: number;
  speedMps: number;
  /** Player-driven chase unit (vs AI). */
  human: boolean;
  /** Racer the unit is on (pursue / intercept / helo-track). */
  target: number | null;
  /** Incident the unit responds to. */
  incident: number | null;
  /** Pursuit tactic (shadow, box-in, pit, rolling-block). */
  tactic: string | null;
  /** Suggested order (player units). */
  suggestion: UnitOrder | null;
  health: number;
  /** Order the captain sent that the source has not reflected yet (recorded runs only). */
  pendingOrder: UnitOrder | null;
  /** Mock only: point a patrolling toy unit drives to. */
  patrolPoint?: Vec2;
}

/** A suspect on the captain map (from `wp_police::captain::RacerSighting`). */
export interface Suspect {
  id: number;
  name: string;
  /** Last known position (what the captain sees). */
  pos: Vec2;
  headingRad: number;
  speedMps: number;
  level: WantedLevel;
  /** Heat points 0..100. */
  heat: number;
  status: PursuitStatus;
  /** Seconds since `pos` was current (0 = in sight). */
  stalenessS: number | null;
  bounty: number;
  unitsAssigned: number;
}

/** An incident (police report / crash from wp_police; violations in non-police runs). */
export interface Incident {
  id: number;
  kind: "Report" | "Speeding" | "RedLight" | "Crash" | "Sighting" | "Reckless" | "WrongWay" | "Collision";
  pos: Vec2;
  tS: number;
  suspect: number | null;
  /** Unit sent to it, if any. */
  unit: number | null;
}

/** A placed tool (roadblock / spike strip; `wp_police::captain::ToolView`). */
export interface DeployedTool {
  id: number;
  tool: "Roadblock" | "SpikeStrip";
  pos: Vec2;
  /** Spike strip ends, when known. */
  a?: Vec2;
  b?: Vec2;
  remainingS: number | null;
}

/** Captain map (owner: wp_police `CaptainView`; map from the run or wp_world). */
export interface CaptainState {
  /** Where the data comes from: a recorded `wp_police` run, or the UI's toy police. */
  source: "wp_police" | "mock";
  units: PoliceUnit[];
  suspects: Suspect[];
  incidents: Incident[];
  tools: DeployedTool[];
  /** Heat sources for the heat layer: position + weight 0..=1. */
  heat: { pos: Vec2; weight: number }[];
  /** `CaptainView::heat_by_district`. */
  heatByDistrict: [District, number][];
  /** Tool cooldowns and budgets (`ToolReadiness`). */
  readiness: ToolReadiness;
  /** Racer whose reinforcements go first. */
  priorityTarget: number | null;
}

/** Static map geometry (owner: wp_world `RoadNetwork`). */
export interface CityMap {
  id: string;
  boundsMin: Vec2;
  boundsMax: Vec2;
  /** Road polylines; `id` is the police `SegmentId` on police maps. */
  roads: { id: number; points: Vec2[]; speedLimitMps: number; district?: District }[];
  zones: { id: number; kind: string; polygon: Vec2[] }[];
  pois: { kind: string; name: string; pos: Vec2 }[];
}

// ---------------------------------------------------------------- replay

/** A marker on the replay timeline. */
export interface ReplayMarker {
  tick: number;
  kind: "contact" | "damage" | "detach" | "totalLoss" | "police" | "violation";
  label: string;
}

/** Replay transport (owner: testbed replay / simcli `replay.bin`). */
export interface ReplayState {
  /** Source name (fixture id or replay file). */
  source: string;
  playing: boolean;
  /** Playback rate (1 = real time). */
  speed: number;
  tick: number;
  totalTicks: number;
  ticksPerSecond: number;
  markers: ReplayMarker[];
  camera: "chase" | "orbit" | "free" | "top";
}

// ---------------------------------------------------------------- root

/** The whole UI state. */
export interface UiState {
  screen: ScreenId;
  /** Screen to return to from settings / pause. */
  previousScreen: ScreenId | null;
  role: Role;
  hud: HudState;
  scanner: ScannerEntry[];
  settings: Settings;
  account: AccountView;
  garage: { cars: GarageCar[]; activeCar: number | null; shops: ShopView[]; selectedShop: string };
  /** Mods catalog is static data; the tree screen reads it from `data/mods`. */
  market: { listings: MarketListing[] };
  lobby: LobbyState;
  results: ResultsState;
  impound: ImpoundState;
  crew: CrewState;
  police: PoliceCareer;
  captain: CaptainState;
  map: CityMap;
  replay: ReplayState;
  /** Live repair bill for the current car (null until requested). */
  bill: RepairBillView | null;
  /** Last command refused by the provider (shown as a toast). */
  notice: { id: number; text: string; kind: "info" | "error" } | null;
}
