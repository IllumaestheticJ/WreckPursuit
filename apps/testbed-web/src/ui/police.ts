// The one police readout source for the testbed (workstream 16 + C, consolidated round 6):
// wp_wasm `police_status(entity)` JSON (`wp_wasm::police::PoliceStatus`), mapped onto the UI kit's
// `PoliceReadout` so the kit's chase HUD renders it.
import type { PoliceReadout } from "../../../ui-kit/src/live";

/** `police_status(entity)` JSON from wp_wasm (one racer's `wp_police` record). */
export interface PoliceStatus {
  level: PoliceReadout["level"];
  /** Raw heat points. */
  heat: number;
  /** Heat 0..=1 within the current level. */
  heat_in_level: number;
  status: PoliceReadout["status"];
  /** Escape progress 0..=1 while `Searching`. */
  cooldown: number;
  units_in_pursuit: number;
  bounty: number;
  units_total: number;
  /** Seconds since the police last saw the racer (null = never). */
  staleness_s: number | null;
  /** Seconds since line of sight was lost. */
  search_s: number;
  /** Escape time for the level and day phase, s. */
  escape_s: number;
  phase: NonNullable<PoliceReadout["phase"]>;
}

/** Map wp_wasm's status onto the kit's `PoliceReadout`. */
export function toReadout(p: PoliceStatus): PoliceReadout {
  return {
    level: p.level,
    heat: p.heat,
    status: p.status,
    bounty: p.bounty,
    stalenessS: p.staleness_s,
    searchS: p.search_s,
    escapeS: p.escape_s,
    phase: p.phase,
    unitsAssigned: p.units_in_pursuit,
  };
}
