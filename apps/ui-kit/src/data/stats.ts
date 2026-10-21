/**
 * Stat bars for a car **as built** (with its installed mods).
 *
 * Stock numbers are physics-measured (`data/stats/cars.json`). A build is any set of mods, so
 * it cannot be precomputed:
 * - a provider that can run the sim measures it (`wp_wasm::car_stats_build_json`, the same rigs
 *   as the table with `wp_economy::apply_mods` applied) and passes it in as `measured`;
 * - otherwise the kit **estimates** it: each stock stat scaled by how much the economy's
 *   estimator (`previewBuild`, port of `wp_economy::Economy::preview`) says the mods change it.
 *   The ratio, not the estimator's absolute number, so a stock car still shows its measured
 *   stats exactly.
 */
import { CAR_STAT_SCALES, CAR_STATS, CARS, type CarStats } from "./catalog";
import { previewBuild } from "./estimate";

/** Stats of a build and of the same car stock. */
export interface BuildStats {
  /** The build (equal to `stock` without mods). */
  stats: CarStats;
  /** Stock, physics-measured. */
  stock: CarStats;
  /** `measured` = physics rig (stock car, or a provider-measured build); `estimated` = stock × estimator ratio. */
  source: "measured" | "estimated";
}

/** `v` placed from `empty` (0) to `full` (1), clamped (`wp_vehicle::stats::scale`). */
function place(v: number, [empty, full]: [number, number]): number {
  return Math.min(1, Math.max(0, (v - empty) / (full - empty)));
}

/** Bars for raw stat values on the table's scales. */
export function barsFor(s: Pick<CarStats, "zero_to_100_s" | "top_speed_kmh" | "cornering_g">): CarStats["bars"] {
  return {
    acceleration: s.zero_to_100_s === null ? 0 : place(s.zero_to_100_s, CAR_STAT_SCALES.zero_to_100_s),
    top_speed: place(s.top_speed_kmh, CAR_STAT_SCALES.top_speed_kmh),
    cornering: place(s.cornering_g, CAR_STAT_SCALES.cornering_g),
  };
}

/**
 * Stats of `defId` with `mods` installed (`null` if the car has no stock stats). `measured`:
 * physics-measured build stats from the provider, used as is when given.
 */
export function buildStats(defId: string, mods: string[], measured?: CarStats | null): BuildStats | null {
  const stock = CAR_STATS[defId];
  const def = CARS[defId];
  if (!stock) return null;
  if (measured) return { stats: measured, stock, source: "measured" };
  if (mods.length === 0 || !def) return { stats: stock, stock, source: "measured" };
  const base = previewBuild(def, []).perf;
  const built = previewBuild(def, mods).perf;
  const ratio = (a: number, b: number) => (b > 0 ? a / b : 1);
  const raw = {
    zero_to_100_s: stock.zero_to_100_s === null ? null : stock.zero_to_100_s * ratio(built.est0to100S, base.est0to100S),
    top_speed_kmh: stock.top_speed_kmh * ratio(built.estTopSpeedMps, base.estTopSpeedMps),
    cornering_g: stock.cornering_g * ratio(built.lateralGripG, base.lateralGripG),
  };
  return { stats: { display_name: stock.display_name, ...raw, bars: barsFor(raw) }, stock, source: "estimated" };
}
