/**
 * Menu navigation input: keyboard keys and standard-mapping gamepad state → `NavAction`s.
 * Pure (no DOM), so it is unit tested; the menu applies the actions to its focus list.
 */

/** One navigation step. */
export type NavAction = "up" | "down" | "left" | "right" | "activate" | "back" | "start";

/** Keyboard `KeyboardEvent.code` → action (arrows, WASD, Enter / Space, Esc / Backspace). */
export function keyAction(code: string): NavAction | null {
  switch (code) {
    case "ArrowUp":
    case "KeyW":
      return "up";
    case "ArrowDown":
    case "KeyS":
      return "down";
    case "ArrowLeft":
    case "KeyA":
      return "left";
    case "ArrowRight":
    case "KeyD":
      return "right";
    case "Enter":
    case "NumpadEnter":
    case "Space":
      return "activate";
    case "Escape":
    case "Backspace":
      return "back";
    default:
      return null;
  }
}

/** Standard-mapping buttons the menu reads. */
const BUTTONS: [number, NavAction][] = [
  [0, "activate"], // A / cross
  [1, "back"], // B / circle
  [9, "start"], // Start / Menu / Options
  [12, "up"],
  [13, "down"],
  [14, "left"],
  [15, "right"],
];

/** Directions repeat while held: first repeat after this long (ms)… */
export const REPEAT_DELAY_MS = 380;
/** …then every this long (ms). */
export const REPEAT_EVERY_MS = 120;
/** Stick deflection that counts as a direction press. */
export const STICK_PRESS = 0.55;
/** Button value that counts as pressed. */
const BUTTON_PRESS = 0.5;

const REPEATS = new Set<NavAction>(["up", "down", "left", "right"]);

/**
 * Edge detector for one gamepad. Construct (or `rest`) with the pad's state at the moment the
 * menu opens, so a button already held (the Start that opened the pause menu) does not fire.
 */
export class PadNav {
  private down = new Map<NavAction, number>();

  /** Treat everything currently held as already handled. */
  rest(buttons: readonly number[], axes: readonly number[]): void {
    this.down.clear();
    for (const a of PadNav.held(buttons, axes)) this.down.set(a, Infinity);
  }

  /** Actions for this poll (presses, plus repeats of held directions). */
  update(buttons: readonly number[], axes: readonly number[], nowMs: number): NavAction[] {
    const held = PadNav.held(buttons, axes);
    const out: NavAction[] = [];
    for (const a of [...this.down.keys()]) if (!held.has(a)) this.down.delete(a);
    for (const a of held) {
      const next = this.down.get(a);
      if (next === undefined) {
        out.push(a);
        this.down.set(a, REPEATS.has(a) ? nowMs + REPEAT_DELAY_MS : Infinity);
      } else if (nowMs >= next) {
        out.push(a);
        this.down.set(a, nowMs + REPEAT_EVERY_MS);
      }
    }
    return out;
  }

  /** Actions held in a pad state (buttons + left stick). */
  static held(buttons: readonly number[], axes: readonly number[]): Set<NavAction> {
    const s = new Set<NavAction>();
    for (const [i, a] of BUTTONS) if ((buttons[i] ?? 0) > BUTTON_PRESS) s.add(a);
    const x = axes[0] ?? 0;
    const y = axes[1] ?? 0;
    if (y < -STICK_PRESS) s.add("up");
    if (y > STICK_PRESS) s.add("down");
    if (x < -STICK_PRESS) s.add("left");
    if (x > STICK_PRESS) s.add("right");
    return s;
  }
}

/** Index of the next focus target in a list of `n` (wraps), `dir` ±1; -1 → first / last. */
export function stepIndex(current: number, n: number, dir: 1 | -1): number {
  if (n <= 0) return -1;
  if (current < 0) return dir > 0 ? 0 : n - 1;
  return (current + dir + n) % n;
}
