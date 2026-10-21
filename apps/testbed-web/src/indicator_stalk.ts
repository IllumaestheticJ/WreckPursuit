// The player's indicator stalk (owner playtest 2026-10-21, "traffic doesn't use turn signals").
//
// COSMETIC AND LOCAL ONLY: `VehicleInput` (wp_contracts runtime.rs) has no turn-signal field, so
// the player's indicators are not in the sim, replays, the state hash or the netcode, and
// traffic / police cannot see them. They light the player's own VehicleView lamps
// (VehicleView.setIndicators) and nothing else. A contract field is requested in the
// integration notes.
//
// Keys (fixed, like the frame-step keys; every letter is already taken): `-` left, `=` right.
// Pressing the lit side again cancels, the other side switches. Like a real stalk it cancels
// itself once the wheel has been turned past `ARM_STEER` towards the indicated side and comes
// back within `CENTRE_STEER` of centre.
import { IND, indicatorPhaseOn } from "./render/fleet";

/** Steering (-1..1, + right) past which a signalled turn counts as begun. */
const ARM_STEER = 0.35;
/** Steering back within this of centre ends the turn and cancels the indicator. */
const CENTRE_STEER = 0.1;

/** Player indicator stalk state. */
export class IndicatorStalk {
  /** -1 left, 0 off, 1 right. */
  dir: -1 | 0 | 1 = 0;
  /** The wheel has been turned towards `dir` since it was switched on. */
  private armed = false;

  /** Flick the stalk left (-1) or right (1): on, switch side, or cancel if already on. */
  toggle(side: -1 | 1): void {
    this.dir = this.dir === side ? 0 : side;
    this.armed = false;
  }

  /** Self-cancel from the steering input of this frame (-1..1, + right). */
  update(steer: number): void {
    if (this.dir === 0) return;
    if (steer * this.dir > ARM_STEER) this.armed = true;
    else if (this.armed && Math.abs(steer) < CENTRE_STEER) {
      this.dir = 0;
      this.armed = false;
    }
  }

  /** Lit lamps at time `t` (`IND` bits, blink phase applied). */
  lamps(t: number): number {
    if (this.dir === 0 || !indicatorPhaseOn(t)) return 0;
    return this.dir < 0 ? IND.left : IND.right;
  }
}
