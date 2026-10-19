// End-of-chase detection for live hosts: the frame a racer stops being chased, so the host can
// open the repair bill — the demo's end-of-chase moment.
//
// Since contract v0.3.0 the police emit a typed `PoliceEvent::PursuitEnded { racer, outcome,
// level, .. }` (level = before the reset), which is the authoritative signal. Hosts that see no
// police events (e.g. a client that only gets a status readout) fall back to status transitions.
import type { EntityId, PursuitOutcome, SimEvent, WantedLevel } from "../generated/contracts";
import type { PursuitStatus } from "../model/police";
import { wantedLevelNumber } from "../model/proposed";

/** Is a racer with this status being chased (`Pursuit` or `Searching`)? */
export function isChased(status: PursuitStatus | null | undefined): boolean {
  return status === "Pursuit" || status === "Searching";
}

/** How a chase ended, from the player's point of view. */
export type ChaseEnd = "escaped" | "captured" | "wrecked" | "calledOff";

/** One detected chase end. */
export interface ChaseEndInfo {
  how: ChaseEnd;
  /** The police outcome (from `PursuitEnded` or the host's fresh outcome); null when only inferred from status. */
  outcome: PursuitOutcome | null;
  /** Wanted level the chase ended at (before the police reset it). */
  level: WantedLevel;
}

/** Player-facing reading of a police outcome. */
export function chaseEndOf(outcome: PursuitOutcome): ChaseEnd {
  switch (outcome) {
    case "Escaped":
    case "Safehouse":
      return "escaped";
    case "Captured":
      return "captured";
    case "CapturedTotalLoss":
      return "wrecked";
    case "CalledOff":
      return "calledOff";
  }
}

/**
 * Detects the end of a chase. Feed one frame at a time: the player's entity, its current pursuit
 * status (null = no police in the session) and level, the events drained this frame, and — if
 * the host reads the police record — the outcome of a pursuit that ended within the last second.
 *
 * - A `PursuitEnded` for the racer ends the chase with its outcome and level.
 * - Otherwise, a total loss mid-chase reports `wrecked`, and `Pursuit` / `Searching` →
 *   `Clear` / `Wanted` reports the fresh outcome if given, else `escaped` (at the highest level
 *   seen during the chase).
 *
 * Each chase is reported once: after an event-reported end, status-based detection waits until
 * the status has read "not chased" once; after a status-inferred end, a late `PursuitEnded` for
 * the same chase is swallowed.
 */
export class ChaseEndDetector {
  private wasChased = false;
  private maxLevel: WantedLevel = "None";
  /** An event already ended the chase; ignore the status until it catches up. */
  private settled = false;
  /** The status already ended the chase; the matching `PursuitEnded` may still arrive. */
  private inferred = false;

  /** Returns the chase end on the frame it happens, else null. */
  update(
    racer: EntityId,
    status: PursuitStatus | null | undefined,
    level: WantedLevel,
    events: readonly SimEvent[] = [],
    freshOutcome: PursuitOutcome | null = null,
  ): ChaseEndInfo | null {
    let ended: ChaseEndInfo | null = null;
    let totalLoss = false;
    for (const e of events) {
      if ("Police" in e && "PursuitEnded" in e.Police && e.Police.PursuitEnded.racer === racer) {
        const { outcome, level: at } = e.Police.PursuitEnded;
        ended = { how: chaseEndOf(outcome), outcome, level: at };
      } else if ("TotalLoss" in e && e.TotalLoss.vehicle === racer) {
        totalLoss = true;
      }
    }
    const now = isChased(status);
    if (ended) {
      const late = this.inferred && !this.wasChased;
      this.inferred = false;
      this.endChase(now);
      return late ? null : ended;
    }
    if (this.settled) {
      if (!now) this.settled = false;
      return null;
    }
    if (now && wantedLevelNumber(level) > wantedLevelNumber(this.maxLevel)) this.maxLevel = level;
    if (this.wasChased && (totalLoss || !now)) {
      const info: ChaseEndInfo = totalLoss
        ? { how: "wrecked", outcome: freshOutcome, level: this.maxLevel }
        : { how: freshOutcome ? chaseEndOf(freshOutcome) : "escaped", outcome: freshOutcome, level: this.maxLevel };
      this.endChase(now);
      this.inferred = true;
      return info;
    }
    if (now && !this.wasChased) this.inferred = false;
    this.wasChased = now;
    return null;
  }

  private endChase(statusStillChased: boolean): void {
    this.wasChased = false;
    this.maxLevel = "None";
    this.settled = statusStillChased;
  }

  /** Forget the previous frames (new session). */
  reset(): void {
    this.wasChased = false;
    this.maxLevel = "None";
    this.settled = false;
    this.inferred = false;
  }
}
