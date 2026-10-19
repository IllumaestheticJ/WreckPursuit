/**
 * Types the UI needs that are not in `wp_contracts` yet and have no owner type to mirror.
 *
 * v0.2.0 landed `GlassShattered`, `RegionDamaged`, `TrafficViolation.pos` and `WantedLevel`
 * (now generated in `../generated/contracts.ts`). Police types mirror `wp_police` in
 * `./police.ts`. What is left here is deferred to v0.3.0 (see `docs/CONTRACT_CHANGELOG.md`).
 */
import type { WantedLevel } from "../generated/contracts";

export type { WantedLevel };

/** Wanted level as a number 0..=5 (`WantedLevel::number`). */
export function wantedLevelNumber(w: WantedLevel): number {
  return w === "None" ? 0 : Number(w.slice(1));
}

/**
 * Mirror of `wp_economy::EconomyEvent` as JSON-encoded inside the contract's opaque
 * `SimEvent::EconomyEvent.payload`. Typed in v0.3.0 (G #1); then this is replaced.
 * `reason` is the externally tagged `EventReason` (e.g. `"CleanRaceBonus"` or
 * `{ "RacePurse": { "race": "sprint", "placement": 1 } }`).
 */
export interface EconomyEventMirror {
  player: number;
  seq: number;
  day: number;
  reason: string | Record<string, Record<string, unknown>>;
  car: number | null;
  cash_delta: number;
  rep_delta: number;
  bounty_delta: number;
  cop_xp_delta: number;
  cash_after: number;
}
