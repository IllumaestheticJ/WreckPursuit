/**
 * Repair-bill data for the `repairBill` screen, from either source:
 * - `billFromWasm`: the testbed's live bill (`wp_wasm::repair_quote_json`, i.e. the running car's
 *   `wp_damage` estimate priced by `wp_economy::Economy::quote_repair`);
 * - `mockBill`: the UI's TS port over a mock garage car (demo page).
 */
import type { RepairEstimate, RepairItem, WantedLevel } from "../generated/contracts";
import type { BillRequest, GarageCar, RepairBillView, ShopView } from "../model/types";
import { quoteRepair, repairItemLabel } from "../data/economy";

/** Default request: legit shop, residential district, repair everything, uninsured. */
export const DEFAULT_BILL_REQUEST: BillRequest = { shop: "Legit", district: "Residential", option: "RepairAll", insurance: null, wanted: "None" };

/** `wp_economy::QuotedLine` as JSON. */
interface QuotedLineJson {
  item: RepairItem;
  severity: number;
  parts: number;
  labour: number;
  total: number;
}

/** `wp_wasm::garage::LiveBill` as JSON. */
export interface LiveBillJson {
  entity: number;
  car_id: string;
  display_name: string;
  class: string;
  base_price: number;
  health: number;
  total_loss: boolean;
  mode: string;
  estimate: RepairEstimate;
  quote: {
    lines: QuotedLineJson[];
    labour_multiplier: number;
    gross: number;
    insurance_covers: number;
    claim_denied: boolean;
    player_pays: number;
    salvage: number;
    reports_to_police: boolean;
  } | null;
  error: string | null;
}

/** The JSON request `repair_quote_json` takes (`BillRequest` in wasm's spelling). */
export function billRequestJson(r: BillRequest): string {
  return JSON.stringify({ shop: r.shop, district: r.district, wanted: r.wanted, option: r.option, insurance: r.insurance ?? undefined });
}

/** Convert the wasm bill into the screen's view. */
export function billFromWasm(b: LiveBillJson, request: BillRequest): RepairBillView {
  const q = b.quote;
  return {
    source: "wp_economy",
    carName: b.display_name,
    carClass: b.class,
    health: b.health,
    totalLoss: b.total_loss,
    mode: b.mode,
    lines: (q?.lines ?? []).map((l) => ({ label: repairItemLabel(l.item), severity: l.severity, parts: l.parts, labour: l.labour, total: l.total })),
    labourMultiplier: q?.labour_multiplier ?? 1,
    gross: q?.gross ?? 0,
    insuranceCovers: q?.insurance_covers ?? 0,
    claimDenied: q?.claim_denied ?? false,
    playerPays: q?.player_pays ?? 0,
    salvage: q?.salvage ?? 0,
    reportsToPolice: q?.reports_to_police ?? false,
    error: b.error,
    request,
  };
}

/** Mock bill for a garage car with the UI's TS port of the billing rules. */
export function mockBill(car: GarageCar, request: BillRequest, mode: "Arcade" | "Simulation" | "Extreme", cash: number): RepairBillView {
  const shop: ShopView = {
    id: "mock",
    name: request.shop,
    kind: request.shop === "Crooked" ? "crooked" : "legit",
    district: request.district === "default" ? "Residential" : request.district,
    labourMultiplier: 1,
  };
  const wanted = request.wanted !== ("None" satisfies WantedLevel);
  const q = quoteRepair({ ...car, insurance: request.insurance ? car.insurance : null }, { kind: request.option === "RepairAll" ? "RepairAll" : request.option }, shop, mode, cash, wanted);
  const nothing = car.outstanding.lines.length === 0 && request.option === "RepairAll";
  return {
    source: "mock",
    carName: car.name,
    carClass: car.class,
    health: car.health,
    totalLoss: car.totalLoss,
    mode,
    lines: q.lines.map((l) => ({ label: l.label, severity: l.severity, parts: l.parts, labour: l.labour, total: l.total })),
    labourMultiplier: 1,
    gross: q.subtotal,
    insuranceCovers: q.insuranceCovered,
    claimDenied: q.insuranceDenied,
    playerPays: Math.max(0, q.total),
    salvage: q.scrapValue,
    reportsToPolice: shop.kind === "legit" && ["L2", "L3", "L4", "L5"].includes(request.wanted),
    error: nothing ? "nothing to repair" : null,
    request,
  };
}
