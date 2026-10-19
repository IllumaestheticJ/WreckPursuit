// Police read-out for the testbed: wp_wasm's `police_status()` JSON, its mapping onto the UI
// kit's `PoliceReadout` (apps/ui-kit, workstream 16), and the big pursuit banner text.
import type { PoliceReadout } from "../../ui-kit/src/live";

/** `police_status(entity)` JSON from wp_wasm (`crates/wp_wasm/src/police.rs`). */
export interface PoliceStatus {
  /** `"None"` | `"L1"` … `"L5"`. */
  level: PoliceReadout["level"];
  /** Raw heat points. */
  heat: number;
  /** Heat 0..=1 within the current level. */
  heat_in_level: number;
  /** `wp_police::PursuitStatus`. */
  status: "Clear" | "Wanted" | "Pursuit" | "Searching";
  /** Escape progress 0..=1 while `Searching`. */
  cooldown: number;
  units_in_pursuit: number;
  bounty: number;
  units_total: number;
  /** Seconds without line of sight (0 unless `Searching`). */
  staleness_s: number;
  /** Escape time for the current level and day phase, s. */
  escape_s: number;
  /** `wp_police::PursuitOutcome` of the latest finished pursuit, if any. */
  last_outcome: PursuitOutcome | null;
  /** Seconds since that pursuit ended. */
  outcome_age_s: number | null;
}

/** `wp_police::PursuitOutcome` names. */
export type PursuitOutcome = "Escaped" | "Safehouse" | "Captured" | "CapturedTotalLoss" | "CalledOff";

/** True for outcomes that put the player in custody. */
export const isCapture = (o: PursuitOutcome | null): boolean => o === "Captured" || o === "CapturedTotalLoss";

/** Map wp_wasm's status onto the kit's live `PoliceReadout`. */
export function toReadout(p: PoliceStatus): PoliceReadout {
  return {
    level: p.level,
    heat: p.heat,
    status: p.status,
    bounty: p.bounty,
    stalenessS: p.status === "Searching" ? p.staleness_s : null,
    escapeS: p.escape_s,
    unitsAssigned: p.units_in_pursuit,
  };
}

/** How long an ESCAPED / CAPTURED banner stays up, s. */
export const OUTCOME_BANNER_S = 6;

/** One banner line. `kind` drives the colour (CSS `data-kind`). */
export interface Banner {
  text: "PURSUIT" | "SEARCHING" | "ESCAPED" | "CAPTURED";
  kind: "pursuit" | "searching" | "escaped" | "captured";
  /** Small line under the headline. */
  sub: string;
}

const OUTCOME_SUB: Record<PursuitOutcome, string> = {
  Escaped: "lost them",
  Safehouse: "laid low in a safehouse",
  Captured: "boxed in and stopped",
  CapturedTotalLoss: "car wrecked while wanted",
  CalledOff: "pursuit called off",
};

/**
 * Banner for a status, or null (clear / merely wanted). A just-finished pursuit shows its
 * outcome for `OUTCOME_BANNER_S` seconds; otherwise an active pursuit or search shows.
 */
export function bannerFor(p: PoliceStatus | null): Banner | null {
  if (!p) return null;
  if (p.status === "Pursuit") {
    const n = p.units_in_pursuit;
    return { text: "PURSUIT", kind: "pursuit", sub: `${p.level} · ${n} unit${n === 1 ? "" : "s"} on you` };
  }
  if (p.status === "Searching") {
    return { text: "SEARCHING", kind: "searching", sub: `escape ${Math.round(p.cooldown * 100)}%` };
  }
  if (p.last_outcome && p.outcome_age_s !== null && p.outcome_age_s < OUTCOME_BANNER_S) {
    const caught = isCapture(p.last_outcome);
    return {
      text: caught ? "CAPTURED" : "ESCAPED",
      kind: caught ? "captured" : "escaped",
      sub: OUTCOME_SUB[p.last_outcome],
    };
  }
  return null;
}
