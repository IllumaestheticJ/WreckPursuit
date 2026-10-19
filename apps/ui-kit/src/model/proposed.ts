/**
 * Types the UI needs that are not in `wp_contracts` yet and have no owner type to mirror.
 *
 * v0.2.0 and v0.3.0 typed everything this file used to mirror (damage, police, economy, race
 * events are generated in `../generated/contracts.ts`). Only helpers remain.
 */
import type { WantedLevel } from "../generated/contracts";

export type { WantedLevel };

/** Wanted level as a number 0..=5 (`WantedLevel::number`). */
export function wantedLevelNumber(w: WantedLevel): number {
  return w === "None" ? 0 : Number(w.slice(1));
}
