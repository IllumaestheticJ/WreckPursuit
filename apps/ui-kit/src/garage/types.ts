// Types of the browser career (machine G): the JSON of wp_wasm's `career_*` exports
// (crates/wp_wasm/src/career.rs). Keep in sync with the Rust structs named in each comment.

export type DamageMode = "Arcade" | "Simulation" | "Extreme";
export type WantedLevel = "None" | "L1" | "L2" | "L3" | "L4" | "L5";
export type Condition = "New" | "Used" | "Worn";
export type ShopKind = "Legit" | "Crooked";
/** `wp_contracts::RepairItem` serde form. */
export type RepairItem = { Region: string } | { Component: string } | { DetachedPart: string };
/** `wp_economy::RepairOption` serde form. */
export type RepairOption = "RepairAll" | "Scrap" | { RepairSelected: RepairItem[] };

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
  insurance: string | null;
  premium_per_day: number | null;
  repair_all: RepairQuote | null;
  repair_lines: RepairLineView[];
  impound: ImpoundStatus | null;
  stats: StatsView | null;
  damage_while_wanted: boolean;
  /** Tyres: a repair never fixes them; replace them here. */
  tires: TireView;
}

/** `career::TireView` (FL, FR, RL, RR). */
export interface TireView {
  tread: number[];
  flat: boolean[];
  worn_out: boolean[];
  grip: number[];
  price_each: number;
}

export type LotKind = "Dealership" | "UsedLot" | "Junkyard";
export type ListingGrade = "New" | "Used" | "Junk";

/** `career::LotSummary`. */
export interface LotSummary {
  id: string;
  name: string;
  kind: LotKind;
  tier: number;
  district: string;
  classes: string[];
  in_stock: number;
  from_price: number | null;
}

/** `career::ListingView`: junkyards hide `condition`, `runs`, `tired_parts` (`null`). */
export interface ListingView {
  slot: number;
  def_id: string;
  name: string;
  class: string;
  grade: ListingGrade;
  price: number;
  rare: boolean;
  mods: string[];
  condition: number | null;
  /** Repair All at a legit residential shop, tyres excluded. */
  est_repairs: number | null;
  runs: boolean | null;
  tired_parts: [string, number][] | null;
  missing: string[];
  dents: string[];
  tread: number[];
  flat: boolean[];
  stats: StatsView | null;
  sold: boolean;
}

/** `career::LotStockView`. */
export interface LotStockView {
  lot: LotSummary;
  day: number;
  cars: ListingView[];
}

/** `lots::LotVisit`: which sim entity is which parking space. */
export interface LotVisit {
  lot: string;
  own_car: number | null;
  cars: { entity: number; slot: number }[];
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
  /** Dealerships, used lots and junkyards with today's counts. */
  lots: LotSummary[];
  floor_car: [string, number] | null;
  insurance: { id: string; premium_per_day_frac: number; coverage: number; deductible: number }[];
  districts: { id: string; labour: number }[];
  stats: CareerStats;
  captures: number;
  escapes: number;
  ledger: LedgerLine[];
}

/** `career::ModOption`. */
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

/** `career::ModTreeView`. */
export interface ModTreeView {
  uid: number;
  stats: StatsView;
  mods: ModOption[];
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
  mods(uid: number): ModTreeView;
  quote(uid: number, req: ShopRequest): RepairQuote;
  buy(defId: string, condition: Condition): CareerView;
  floorCar(): CareerView;
  sell(uid: number): CareerView;
  setActive(uid: number): CareerView;
  install(uid: number, modId: string): CareerView;
  uninstall(uid: number, modId: string): CareerView;
  repair(uid: number, req: ShopRequest): CareerView;
  release(uid: number): CareerView;
  insure(uid: number, tier: string): CareerView;
  nextNight(): CareerView;
  /** Today's stock of a lot. */
  lot(lotId: string): LotStockView;
  /** Drive to the lot: leaves the garage and loads the lot itself. */
  visitLot(lotId: string): void;
  /** Buy the car in a lot space. */
  buyListing(lotId: string, slot: number): CareerView;
  /** Replace tyres (FL, FR, RL, RR); repairs never fix tyres. */
  replaceTires(uid: number, wheels: boolean[]): CareerView;
  newCareer(mode: DamageMode): CareerView;
  exportSave(): void;
  importSave(text: string): CareerView;
  startRun(kind: RunKind): void;
  close(): void;
}
