// Types of the browser career (machine G): the JSON of wp_wasm's `career_*` exports
// (crates/wp_wasm/src/career.rs). Keep in sync with the Rust structs named in each comment.

export type DamageMode = "Arcade" | "Simulation";
export type WantedLevel = "None" | "L1" | "L2" | "L3" | "L4" | "L5";
export type Condition = "New" | "Used" | "Worn";
export type ShopKind = "Legit" | "Crooked";
/** `wp_contracts::RepairItem` serde form. */
export type RepairItem = { Region: string } | { Component: string } | { DetachedPart: string };
/** `wp_economy::RepairOption` serde form. */
export type RepairOption = "RepairAll" | "Scrap" | { RepairSelected: RepairItem[] };

/** `wp_economy::CarTune` (= `wp_vehicle::CarTune`): the free tune, each slider 1..=5, 3 = stock. */
export interface CarTune {
  /** 1 soft … 5 stiff. */
  suspension: number;
  /** 1 short … 5 long. */
  gearing: number;
  /** 1 open … 3 stock … 5 locked. */
  diff: number;
  /** 1 rear … 5 front. */
  brake_balance: number;
}

/** `career::StatsView`. */
export interface StatsView {
  class_index: number;
  bracket: string;
  mass_kg: number;
  est_0_100_s: number;
  top_speed_kmh: number;
  lateral_g: number;
  brake_g: number;
}

/** `wp_economy::QuotedLine`. */
export interface QuotedLine {
  item: RepairItem;
  severity: number;
  parts: number;
  labour: number;
  total: number;
}

/** `wp_economy::RepairQuote`. */
export interface RepairQuote {
  lines: QuotedLine[];
  labour_multiplier: number;
  gross: number;
  insurance_covers: number;
  claim_denied: boolean;
  player_pays: number;
  salvage: number;
  reports_to_police: boolean;
}

/** `wp_economy::ImpoundStatus`. */
export interface ImpoundStatus {
  car: number;
  since_day: number;
  days: number;
  level: WantedLevel;
  fee: number;
  fee_per_day: number;
  days_until_auction: number;
  affordable: boolean;
}

/** `career::RepairLineView`. */
export interface RepairLineView {
  item: RepairItem;
  label: string;
  severity: number;
  price: number;
}

/** `career::CarView`. */
export interface CarView {
  uid: number;
  def_id: string;
  name: string;
  class: string;
  active: boolean;
  location: "Garage" | "Impound";
  health: number;
  total_loss: boolean;
  drivable: boolean;
  value: number;
  sale_price: number;
  /** What the car body cost (price paid, mods excluded). */
  paid: number;
  scrap_value: number;
  mods: { id: string; name: string; slot: string }[];
  /** The free tune, applied when the next run spawns the car. */
  tune: CarTune;
  insurance: string | null;
  premium_per_day: number | null;
  repair_all: RepairQuote | null;
  repair_lines: RepairLineView[];
  impound: ImpoundStatus | null;
  stats: StatsView | null;
  /** Some outstanding damage dates from a police capture: insurance will not pay for it. */
  damage_before_capture: boolean;
}

/** `career::DealerCar`. */
export interface DealerCar {
  def_id: string;
  name: string;
  class: string;
  base_price: number;
  price_new: number;
  price_used: number;
  price_worn: number;
  stats: StatsView;
}

/** `wp_wasm::run::LedgerLine`. */
export interface LedgerLine {
  seq: number;
  day: number;
  reason: string;
  detail: unknown;
  cash_delta: number;
  rep_delta: number;
  bounty_delta: number;
  cash_after: number;
}

/** `wp_economy::CareerStats`. */
export interface CareerStats {
  runs: number;
  runs_free: number;
  runs_captured: number;
  runs_wrecked: number;
  best_escape_bounty: number;
  /** Physics ticks driven (240 per second). */
  driven_ticks: number;
}

/** `career::CareerView`: the whole garage screen. */
export interface CareerView {
  version: number;
  mode: DamageMode;
  day: number;
  cash: number;
  debt: number;
  rep: number;
  bounty: number;
  net_worth: number;
  carried_heat: number;
  carried_level: WantedLevel;
  run_active: boolean;
  active_car: number | null;
  can_start: boolean;
  start_blocker: string | null;
  cars: CarView[];
  dealer: DealerCar[];
  floor_car: [string, number] | null;
  insurance: { id: string; premium_per_day_frac: number; coverage: number; deductible: number }[];
  districts: { id: string; labour: number }[];
  stats: CareerStats;
  captures: number;
  escapes: number;
  ledger: LedgerLine[];
}

/** `career::ModOption`: a mod with what installing it (or, installed, removing it) would do. */
export interface ModOption {
  id: string;
  name: string;
  slot: string;
  line: string;
  tier: number;
  price: number;
  tradeoff: string;
  group: "performance" | "durability" | "cosmetic" | "tool";
  installed: boolean;
  allowed: boolean;
  reason: string | null;
  replaces: string | null;
  stats_after: StatsView | null;
  requires: string[];
}

/** `career::ShopRequest`. */
export interface ShopRequest {
  option?: RepairOption;
  district?: string;
  shop?: ShopKind;
}

/** `career::RunClose`. */
export interface RunClose {
  end: "Free" | "Captured" | "Wrecked";
  carried_heat: number;
  cash: number;
  ledger: LedgerLine[];
}

/** Which run the garage starts. */
export type RunKind = "free" | "chase";

/** What the garage screen can do (the testbed implements it over wp_wasm). Every action
 * returns the new view, or throws an Error whose message is shown to the player. */
export interface CareerApi {
  view(): CareerView;
  quote(uid: number, req: ShopRequest): RepairQuote;
  buy(defId: string, condition: Condition): CareerView;
  floorCar(): CareerView;
  sell(uid: number): CareerView;
  setActive(uid: number): CareerView;
  /** Set the car's free tune (no money). */
  setTune(uid: number, tune: CarTune): CareerView;
  repair(uid: number, req: ShopRequest): CareerView;
  release(uid: number): CareerView;
  insure(uid: number, tier: string): CareerView;
  nextNight(): CareerView;
  newCareer(mode: DamageMode): CareerView;
  exportSave(): void;
  importSave(text: string): CareerView;
  startRun(kind: RunKind): void;
  /** Open the 3D garage workshop on car `uid`: parts, the parts catalog and upgrades. */
  workshop(uid: number): void;
  close(): void;
}

// ----- garage workshop (crates/wp_economy/src/parts.rs, wp_wasm career_workshop_json) --------

/** `wp_economy::PartGroup`. */
export type PartGroup =
  | "Body" | "Glass" | "Aero" | "Cooling" | "Engine" | "Electronics" | "Drivetrain" | "Fuel"
  | "Wheels" | "Brakes" | "Suspension" | "Steering" | "Protection" | "Interior";

/** A part as the workshop shows it (`PartView`). */
export interface PartView {
  grade: string | null;
  name: string;
  tier: number;
  condition: number;
}

/** One slot of the car (`SlotView`). */
export interface SlotView {
  id: string;
  name: string;
  group: PartGroup;
  kind: string;
  /** Assembly slot it lives inside (engine internals: "engine"), or null. */
  parent: string | null;
  fitted: PartView | null;
  required: boolean;
  /** Fitted slots in the way (empty = the slot can be worked on). */
  blocked_by: string[];
  /** Why the fitted part cannot come off now (null = it can). */
  remove_blocker: string | null;
}

/** A loose part (`LoosePart`). */
export interface LoosePart {
  uid: number;
  kind: string;
  grade: string | null;
  model: string | null;
  condition: number;
  damage: unknown[];
  value: number;
  /** Parts still inside a loose assembly (an engine's internals); `kind` is their slot. */
  contents?: LoosePart[];
}

/** An empty slot of a loose engine a part can go into (`StandFit`). */
export interface StandFit {
  assembly: number;
  slot: string;
}

/** A loose engine on the engine stand (`StandView`). */
export interface StandView {
  uid: number;
  name: string;
  /** Assembly slot id ("engine"). */
  slot: string;
  /** Its internals, in table order. */
  slots: SlotView[];
  /** Names of internals it is missing. */
  missing: string[];
}

/** A loose part with where it fits on this car (`InventoryView`). */
export interface InventoryView {
  part: LoosePart;
  name: string;
  fits: string[];
  /** Empty slots of loose engines on the stand that take it. */
  stand_fits: StandFit[];
  reason: string | null;
  sale_price: number;
}

/** One line of the parts catalog (`CatalogEntry`). */
export interface CatalogEntry {
  kind: string;
  slot_name: string;
  group: PartGroup;
  grade: string | null;
  name: string;
  tier: number;
  price: number;
  count: number;
  reason: string | null;
}

/** `career_workshop_json` (`WorkshopScreen`). */
export interface WorkshopScreen {
  car: number;
  slots: SlotView[];
  /** Names of empty required slots. */
  missing: string[];
  /** Ids of installed mods that are not parts (`upgrade_options` has them with names). */
  upgrades: string[];
  inventory: InventoryView[];
  /** Loose engines in the inventory, as the engine stand shows them. */
  stands: StandView[];
  /** Engine torque left after worn parts (1 = nothing lost). */
  torque_factor: number;
  catalog: CatalogEntry[];
  /** The car's stats as built. */
  stats: StatsView;
  /** Stats with a catalog grade fitted in place of what its slot has now, by mod id. */
  previews: Record<string, StatsView>;
  /** Mods that are not parts (paint, vinyls, gadgets, final drive...), installed straight on the car. */
  upgrade_options: ModOption[];
  cash: number;
  def_id: string;
}

/** What the workshop screen calls; every call returns the new screen (throws on refusal). */
export interface WorkshopApi {
  screen(): WorkshopScreen;
  remove(slot: string): WorkshopScreen;
  fit(slot: string, part: number): WorkshopScreen;
  buy(kind: string, grade: string | null): WorkshopScreen;
  sell(part: number): WorkshopScreen;
  /** Engine stand: take the part in `slot` out of loose engine `assembly`. */
  strip(assembly: number, slot: string): WorkshopScreen;
  /** Engine stand: put inventory part `part` into `slot` of loose engine `assembly`. */
  build(assembly: number, slot: string, part: number): WorkshopScreen;
  /** Install an upgrade (one of `upgrade_options`; pays, a same-line upgrade is swapped). */
  install(modId: string): WorkshopScreen;
  /** Remove an upgrade (refunded at resale value). */
  uninstall(modId: string): WorkshopScreen;
}
