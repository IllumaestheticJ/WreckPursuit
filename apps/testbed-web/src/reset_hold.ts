// Reset key: tap = reset in place, hold for HOLD_S = "return to road" (feature F41).
//
// The reset action (R / pad Y / touch) no longer fires on press: a press starts a hold, a
// release before HOLD_S is a tap (wasm `reset_in_place`: upright on the nearest clear lane),
// and keeping it down until HOLD_S fires `reset_to_road` (the player's last on-road spot, at
// any distance: the road it came off; in a running race: `race_restart_checkpoint`, back to
// the last gate passed, F42). A small bar under the car shows the hold filling up.

/** Seconds the reset key must be held for "return to road". */
export const HOLD_S = 1.0;
/** The ring only shows after this much holding (a quick tap shows nothing), s. */
const SHOW_AFTER_S = 0.15;

/** What a hold resolved to this frame. */
export type ResetHoldResult = "tap" | "road" | null;

/** Tracks one press of the reset action and draws its hold progress. */
export class ResetHold {
  private startedAt: number | null = null;
  private readonly el: HTMLDivElement;
  private readonly fill: HTMLDivElement;
  /** Last result (automation). */
  last: ResetHoldResult = null;

  /** `prompt` = the text over the bar, e.g. "HOLD R · BACK TO THE ROAD" (read each frame:
   * the binding can change, and in a race the hold restarts from the last checkpoint). */
  constructor(
    parent: HTMLElement,
    private readonly prompt: () => string,
  ) {
    const style = document.createElement("style");
    style.textContent = `
      #reset-hold { position: absolute; left: 50%; bottom: 22vh; transform: translateX(-50%); z-index: 41;
        display: none; flex-direction: column; align-items: center; gap: 6px; pointer-events: none;
        font: 600 12px/1.2 Inter, system-ui, sans-serif; color: #f2f2f2; letter-spacing: 0.06em;
        text-shadow: 0 1px 2px rgba(0,0,0,0.7); }
      #reset-hold .track { width: 160px; height: 5px; border-radius: 3px; background: rgba(255,255,255,0.18); overflow: hidden; }
      #reset-hold .fill { height: 100%; width: 0%; background: #2ee59d; }`;
    document.head.appendChild(style);
    this.el = document.createElement("div");
    this.el.id = "reset-hold";
    this.el.innerHTML = `<div class="t"></div><div class="track"><div class="fill"></div></div>`;
    this.fill = this.el.querySelector(".fill") as HTMLDivElement;
    parent.appendChild(this.el);
  }

  /** The reset action was pressed at `nowMs` (performance.now()). */
  press(nowMs: number): void {
    this.startedAt ??= nowMs;
  }

  /** Is a press being held right now? */
  get holding(): boolean {
    return this.startedAt !== null;
  }

  /** Per frame: `held` = the reset action is still down. Returns "tap" on a release before
   * HOLD_S, "road" once it has been held HOLD_S (then nothing until the next press). */
  frame(nowMs: number, held: boolean): ResetHoldResult {
    if (this.startedAt === null) return null;
    const t = (nowMs - this.startedAt) / 1000;
    let out: ResetHoldResult = null;
    if (t >= HOLD_S) out = "road";
    else if (!held) out = "tap";
    if (out) {
      this.startedAt = null;
      this.el.style.display = "none";
      this.last = out;
      return out;
    }
    if (t >= SHOW_AFTER_S) {
      this.el.style.display = "flex";
      (this.el.querySelector(".t") as HTMLDivElement).textContent = this.prompt();
      this.fill.style.width = `${Math.min(100, (t / HOLD_S) * 100).toFixed(0)}%`;
    }
    return null;
  }

  /** Drop a press in progress (new session, menu opened). */
  cancel(): void {
    this.startedAt = null;
    this.el.style.display = "none";
  }
}
