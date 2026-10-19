// End-of-chase detection for live hosts (round 6): the frame a racer stops being chased, so the
// host can open the repair bill — the demo's end-of-chase moment.
import type { PursuitStatus } from "../model/police";

/** Is a racer with this status being chased (`Pursuit` or `Searching`)? */
export function isChased(status: PursuitStatus | null | undefined): boolean {
  return status === "Pursuit" || status === "Searching";
}

/** How a chase ended. */
export type ChaseEnd = "escaped" | "wrecked";

/**
 * Detects the end of a chase: the frame the racer goes from `Pursuit` / `Searching` back to
 * `Clear` / `Wanted` (escaped), or the car is written off mid-chase (wrecked). Feed one status
 * per frame; a missing status (no police in the session) counts as not chased.
 */
export class ChaseEndDetector {
  private wasChased = false;

  /** Returns how the chase ended on the frame it ends, else null. */
  update(status: PursuitStatus | null | undefined, totalLoss: boolean): ChaseEnd | null {
    const now = isChased(status);
    const was = this.wasChased;
    this.wasChased = now && !totalLoss;
    if (!was) return null;
    if (totalLoss) return "wrecked";
    return now ? null : "escaped";
  }

  /** Forget the previous frame (new session). */
  reset(): void {
    this.wasChased = false;
  }
}
