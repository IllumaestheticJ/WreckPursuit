/**
 * Mock economy pricing for the garage, market and impound screens.
 *
 * Numbers mirror `data/economy/{prices,market,consequences}.toml` and the billing rules in
 * `crates/wp_economy/README.md` (re-pricing each repair line by class labour rate × district ×
 * shop, then × the mode's `repair_cost_scale`). The real quote comes from
 * `wp_economy::Economy::quote_repair`; this exists so the UI can be built and tested now.
 */
import type { DamageMode, RepairEstimate, RepairItem, VehicleClass } from "../generated/contracts";
import type { BillLine, GarageCar, InsuranceView, RepairChoice, RepairQuote, ShopView } from "../model/types";
import { MODS } from "./catalog";

/** `prices.toml` `[class.*]` (labour per hour, reference value, towing). */
export const CLASS_PRICING: Record<VehicleClass, { labour: number; referenceValue: number; towing: number }> = {
  Beater: { labour: 55, referenceValue: 4000, towing: 120 },
  Compact: { labour: 85, referenceValue: 18000, towing: 150 },
  Sport: { labour: 110, referenceValue: 40000, towing: 200 },
  Muscle: { labour: 95, referenceValue: 34000, towing: 220 },
  Exotic: { labour: 180, referenceValue: 160000, towing: 450 },
  Truck: { labour: 90, referenceValue: 30000, towing: 300 },
  PoliceCruiser: { labour: 90, referenceValue: 32000, towing: 200 },
  PoliceInterceptor: { labour: 110, referenceValue: 55000, towing: 220 },
  PoliceSuv: { labour: 100, referenceValue: 45000, towing: 280 },
};

/** `prices.toml` `[district_labour]`. */
export const DISTRICT_LABOUR: Record<string, number> = {
  default: 1.0,
  Core: 1.25,
  Suburb: 1.0,
  Industrial: 0.85,
  Docks: 0.8,
  Hills: 1.1,
};

/** `prices.toml` `[shop]`. */
export const SHOP = { crookedMarkup: 1.15, crookedWantedMarkup: 1.6 };

/** `data/modes/*.toml` `repair_cost_scale`. */
export const MODE_REPAIR_SCALE: Record<DamageMode, number> = { Arcade: 0.5, Simulation: 0.5, Extreme: 1.0 };

/** `prices.toml` `[insurance.*]`. */
export const INSURANCE_TIERS: Record<string, { premiumPerDayFrac: number; coverage: number; deductible: number }> = {
  basic: { premiumPerDayFrac: 0.0025, coverage: 0.5, deductible: 500 },
  full: { premiumPerDayFrac: 0.005, coverage: 0.8, deductible: 250 },
};

/** `market.toml`. */
export const MARKET = {
  dealerBuyFrac: 0.7,
  modResaleFrac: 0.4,
  modValueFrac: 0.5,
  damagedValueFloor: 0.25,
  condition: { New: 1.0, Used: 0.8, Worn: 0.6 } as Record<"New" | "Used" | "Worn", number>,
};

/** `prices.toml` `[scrap]`. */
export const SCRAP = { minFrac: 0.08, healthFrac: 0.32, modsFrac: 0.3 };

/** Human label for a repair item. */
export function repairItemLabel(item: RepairItem): string {
  const split = (s: string) => s.replace(/([a-z])([A-Z])/g, "$1 $2");
  if ("Region" in item) return `${split(item.Region)} — panel`;
  if ("Component" in item) return split(item.Component);
  return `${split(item.DetachedPart)} — replace detached part`;
}

/** Total price of installed mods. */
export function modsPrice(mods: string[]): number {
  return mods.reduce((s, id) => s + (MODS[id]?.price ?? 0), 0);
}

/** Labour multiplier for a shop and a car (`district × shop markup`). */
export function labourMultiplier(shop: ShopView, wanted: boolean): number {
  const district = DISTRICT_LABOUR[shop.district] ?? DISTRICT_LABOUR.default;
  const markup = shop.kind === "crooked" ? (wanted ? SHOP.crookedWantedMarkup : SHOP.crookedMarkup) : 1;
  return district * markup;
}

/** Salvage value (`base × (min + health_frac × health) + mods_frac × mods price`). */
export function scrapValue(car: Pick<GarageCar, "basePrice" | "health" | "mods">): number {
  return Math.round(car.basePrice * (SCRAP.minFrac + SCRAP.healthFrac * car.health) + SCRAP.modsFrac * modsPrice(car.mods));
}

/**
 * Price a repair decision. `lines` of `RepairSelected` index into `car.outstanding.lines`.
 * Insurance pays `coverage` of the bill above the deductible, unless the damage was sustained
 * while wanted.
 */
export function quoteRepair(
  car: GarageCar,
  choice: RepairChoice,
  shop: ShopView,
  mode: DamageMode,
  cash: number,
  wanted: boolean,
): RepairQuote {
  const rate = CLASS_PRICING[car.class].labour;
  const mult = labourMultiplier(shop, wanted);
  const modeScale = MODE_REPAIR_SCALE[mode];
  const scrap = scrapValue(car);
  const picked = pickLines(car.outstanding, choice);
  const lines: BillLine[] = picked.map((l) => {
    const labour = l.labour_hours * rate * mult;
    return {
      label: repairItemLabel(l.item),
      severity: l.severity,
      parts: Math.round(l.parts_cost * modeScale),
      labourHours: l.labour_hours,
      labour: Math.round(labour * modeScale),
      total: Math.round((l.parts_cost + labour) * modeScale),
    };
  });
  const subtotal = lines.reduce((s, l) => s + l.total, 0);
  let covered = 0;
  const denied = !!car.insurance && car.sustainedWhileWanted && subtotal > 0;
  if (car.insurance && !denied && subtotal > car.insurance.deductible) {
    covered = Math.round((subtotal - car.insurance.deductible) * car.insurance.coverage);
  }
  const total = choice.kind === "Scrap" ? -scrap : subtotal - covered;
  return {
    lines,
    subtotal,
    modeScale,
    insuranceCovered: covered,
    insuranceDenied: denied,
    total,
    scrapValue: scrap,
    affordable: total <= 0 || total <= cash,
  };
}

function pickLines(est: RepairEstimate, choice: RepairChoice): RepairEstimate["lines"] {
  switch (choice.kind) {
    case "RepairAll":
      return est.lines;
    case "RepairSelected":
      return est.lines.filter((_, i) => choice.lines.includes(i));
    case "DriveDamaged":
    case "Scrap":
      return [];
  }
}

/** `market.toml` `[class_bands]`: dealer prices are clamped into these. */
export const CLASS_BANDS: Record<VehicleClass, [number, number]> = {
  Beater: [1500, 6000],
  Compact: [9000, 30000],
  Sport: [25000, 70000],
  Muscle: [22000, 60000],
  Exotic: [90000, 320000],
  Truck: [15000, 50000],
  PoliceCruiser: [20000, 45000],
  PoliceInterceptor: [35000, 80000],
  PoliceSuv: [30000, 65000],
};

/** Port of `Economy::market_price`: `base × condition`, clamped into the class band. */
export function marketPrice(basePrice: number, cls: VehicleClass, condition: "New" | "Used" | "Worn"): number {
  const [lo, hi] = CLASS_BANDS[cls];
  return Math.min(Math.max(Math.round(basePrice * MARKET.condition[condition]), lo), hi);
}

type ValuedCar = Pick<GarageCar, "basePrice" | "health" | "mods" | "totalLoss" | "outstanding">;

/** Port of `Economy::car_value`: pristine value, `base + mod_value_frac × mods`. */
export function pristineValue(car: Pick<GarageCar, "basePrice" | "mods">): number {
  return car.basePrice + Math.round(MARKET.modValueFrac * modsPrice(car.mods));
}

/** Port of `Economy::current_value`: pristine minus outstanding repairs, floored. */
export function carValue(car: ValuedCar): number {
  const pristine = pristineValue(car);
  if (car.totalLoss) return scrapValue(car);
  const floor = Math.round(pristine * MARKET.damagedValueFloor);
  return Math.max(pristine - Math.round(car.outstanding.total), floor);
}

/** Port of `Economy::sale_price`: what a dealer pays. */
export function salePrice(car: ValuedCar): number {
  const modsBack = Math.round(MARKET.modResaleFrac * modsPrice(car.mods));
  const body = Math.round(MARKET.dealerBuyFrac * car.basePrice);
  const ratio = carValue(car) / Math.max(pristineValue(car), 1);
  return Math.round((body + modsBack) * Math.min(Math.max(ratio, 0), 1));
}

/** Insurance quote for a tier. */
export function insuranceQuote(car: Pick<GarageCar, "value">, tier: string): InsuranceView {
  const t = INSURANCE_TIERS[tier];
  return {
    tier,
    coverage: t.coverage,
    deductible: t.deductible,
    premiumPerDay: Math.round(car.value * t.premiumPerDayFrac),
  };
}

/** `consequences.toml` `[impound]`: base fee + per-day fee by class, auction day. */
export const IMPOUND = {
  auctionAfterDays: 10,
  baseFee: { Beater: 150, Compact: 400, Sport: 700, Muscle: 650, Exotic: 2500, Truck: 500, PoliceCruiser: 0, PoliceInterceptor: 0, PoliceSuv: 0 } as Record<VehicleClass, number>,
  perDay: { Beater: 40, Compact: 120, Sport: 200, Muscle: 180, Exotic: 600, Truck: 150, PoliceCruiser: 0, PoliceInterceptor: 0, PoliceSuv: 0 } as Record<VehicleClass, number>,
};

/** Retrieval fee after `days` in the pound. */
export function impoundFee(cls: VehicleClass, days: number): number {
  return IMPOUND.baseFee[cls] + IMPOUND.perDay[cls] * days;
}

/** `consequences.toml` `cop_rank_thresholds` (XP for levels 1..=6). */
export const COP_RANK_THRESHOLDS = [500, 1500, 3500, 7000, 12000, 20000];

/** Display names for cop levels 0..=6 (UI naming; the economy only has numbers). */
export const COP_RANKS = ["Recruit", "Patrol Officer", "Senior Officer", "Corporal", "Sergeant", "Lieutenant", "Captain"];

/** Port of `Economy::cop_level`. */
export function copLevel(xp: number): number {
  return COP_RANK_THRESHOLDS.filter((t) => xp >= t).length;
}

/** `prices.toml` `[region_parts]` reference parts costs (on the reference Compact). */
export const REGION_PARTS: Record<string, number> = {
  FrontBumper: 450, Hood: 600, FrontLeftFender: 380, FrontRightFender: 380, LeftDoorF: 700, LeftDoorR: 650,
  RightDoorF: 700, RightDoorR: 650, Roof: 1200, RearLeftQuarter: 900, RearRightQuarter: 900, Trunk: 550,
  RearBumper: 420, Windshield: 450, RearGlass: 300, FrameFront: 1600, FrameRear: 1300, FrameLeft: 1100,
  FrameRight: 1100, Cabin: 2500,
};

/** `prices.toml` `[component_parts]`. */
export const COMPONENT_PARTS: Record<string, number> = {
  Engine: 3800, Radiator: 450, Battery: 180, FuelTank: 400, SteeringRack: 650, SuspensionFL: 420,
  SuspensionFR: 420, SuspensionRL: 420, SuspensionRR: 420, WheelFL: 260, WheelFR: 260, WheelRL: 260,
  WheelRR: 260, Gearbox: 2200, Driveshaft: 500, BrakeFL: 180, BrakeFR: 180, BrakeRL: 180, BrakeRR: 180,
  Exhaust: 450, Lights: 400,
};

/** `prices.toml` `[class.*].parts_scale`. */
export const PARTS_SCALE: Record<VehicleClass, number> = {
  Beater: 0.35, Compact: 1.0, Sport: 1.9, Muscle: 1.7, Exotic: 7.0, Truck: 1.5, PoliceCruiser: 1.4,
  PoliceInterceptor: 2.4, PoliceSuv: 2.0,
};

/**
 * One unscaled repair line as `wp_damage` would price it with the economy's `PriceTable`
 * (parts = reference × class parts_scale × value factor × severity; hours = 2 × severity).
 */
export function mockRepairLine(item: RepairItem, severity: number, cls: VehicleClass, value: number): RepairEstimate["lines"][number] {
  const ref = "Component" in item ? COMPONENT_PARTS[item.Component] : REGION_PARTS["Region" in item ? item.Region : item.DetachedPart];
  const valueFactor = Math.min(Math.max(value / CLASS_PRICING[cls].referenceValue, 0.4), 2.5);
  const parts = Math.round((ref ?? 300) * PARTS_SCALE[cls] * valueFactor * ("DetachedPart" in item ? 1 : severity));
  const hours = Math.round(2 * severity * 10) / 10;
  return { item, severity, parts_cost: parts, labour_hours: hours, total: Math.round(parts + hours * CLASS_PRICING[cls].labour) };
}
