// Rebindable controls: every player action, its default keyboard keys and gamepad controls, and
// the saved overrides (localStorage `wp.bindings.v1`). Pure data + lookups; the input reader is
// `input.ts` and the settings panel is `ui/controls.ts`.
//
// Keyboard slots hold `KeyboardEvent.code` values. Gamepad slots hold a button index or a
// signed axis of the browser's standard mapping (https://w3c.github.io/gamepad/#remapping).
// Camera angles (1–8), debug-layer keys (settings.ts) and the keys inside the open city map are
// fixed and not listed here.

/** Every rebindable action. */
export type ActionId =
  | "steerLeft"
  | "steerRight"
  | "throttle"
  | "brake"
  | "handbrake"
  | "gearUp"
  | "gearDown"
  | "horn"
  | "lookBack"
  | "cameraCycle"
  | "reset"
  | "headlights"
  | "followNext"
  | "map"
  | "pause"
  | "help"
  | "controls"
  | "mute"
  | "repairBill";

/** How an action is read: `analog` 0..1 every frame, `held` on/off every frame, `edge` once
 * per press. */
export type ActionKind = "analog" | "held" | "edge";

/** One row of the settings panel. */
export interface ActionDef {
  id: ActionId;
  label: string;
  kind: ActionKind;
}

/** A gamepad control: a button (value 0..1, triggers are analogue) or one direction of an axis. */
export type PadControl = { button: number } | { axis: number; dir: 1 | -1 };

/** The full binding set, as saved. */
export interface Bindings {
  keys: Record<ActionId, string[]>;
  pad: Record<ActionId, PadControl[]>;
  /** Stick deadzone, 0..0.5 (fraction of travel). */
  padDeadzone: number;
  /** Steering response exponent: 1 = linear, >1 = finer near centre. */
  padCurve: number;
}

/** Number of slots per action and device in the panel. */
export const SLOTS = 2;

/** The actions, in panel order. */
export const ACTIONS: ActionDef[] = [
  { id: "steerLeft", label: "steer left", kind: "analog" },
  { id: "steerRight", label: "steer right", kind: "analog" },
  { id: "throttle", label: "throttle", kind: "analog" },
  { id: "brake", label: "brake", kind: "analog" },
  { id: "handbrake", label: "handbrake", kind: "held" },
  { id: "gearUp", label: "shift up", kind: "edge" },
  { id: "gearDown", label: "shift down", kind: "edge" },
  { id: "horn", label: "horn", kind: "held" },
  { id: "lookBack", label: "look back (hold)", kind: "held" },
  { id: "cameraCycle", label: "next camera", kind: "edge" },
  { id: "reset", label: "reset car", kind: "edge" },
  { id: "headlights", label: "headlights", kind: "edge" },
  { id: "followNext", label: "follow the next car", kind: "edge" },
  { id: "map", label: "city map", kind: "edge" },
  { id: "pause", label: "pause", kind: "edge" },
  { id: "help", label: "help", kind: "edge" },
  { id: "controls", label: "controls (this panel)", kind: "edge" },
  { id: "mute", label: "audio mute", kind: "edge" },
  { id: "repairBill", label: "repair bill", kind: "edge" },
];

/** Defaults. Look-back keeps V (driving); audio mute moved to F8 so the two no longer share V. */
export function defaultBindings(): Bindings {
  return {
    keys: {
      steerLeft: ["KeyA", "ArrowLeft"],
      steerRight: ["KeyD", "ArrowRight"],
      throttle: ["KeyW", "ArrowUp"],
      brake: ["KeyS", "ArrowDown"],
      handbrake: ["Space"],
      gearUp: ["ShiftLeft", "ShiftRight"],
      gearDown: ["ControlLeft", "ControlRight"],
      horn: ["KeyQ"],
      lookBack: ["KeyV"],
      cameraCycle: ["KeyC"],
      reset: ["KeyR"],
      headlights: ["KeyO"],
      followNext: ["Backquote"],
      map: ["KeyM", "Tab"],
      pause: ["KeyP"],
      help: ["F1", "Slash"],
      controls: ["F9"],
      mute: ["F8"],
      repairBill: ["KeyF"],
    },
    // Standard mapping: 0 A, 1 B, 2 X, 3 Y, 4 LB, 5 RB, 6 LT, 7 RT, 8 View, 9 Menu, 10 L3, 11 R3,
    // 12–15 d-pad up/down/left/right.
    pad: {
      steerLeft: [{ axis: 0, dir: -1 }],
      steerRight: [{ axis: 0, dir: 1 }],
      throttle: [{ button: 7 }],
      brake: [{ button: 6 }],
      handbrake: [{ button: 0 }, { button: 2 }],
      gearUp: [{ button: 5 }],
      gearDown: [{ button: 4 }],
      horn: [{ button: 10 }],
      lookBack: [{ button: 11 }],
      cameraCycle: [{ button: 8 }],
      reset: [{ button: 3 }],
      headlights: [{ button: 13 }],
      followNext: [],
      map: [{ button: 15 }],
      pause: [{ button: 9 }],
      help: [],
      controls: [],
      mute: [],
      repairBill: [],
    },
    padDeadzone: 0.12,
    padCurve: 1.0,
  };
}

/** localStorage key of the saved bindings. */
export const BINDINGS_KEY = "wp.bindings.v1";

const isPadControl = (c: unknown): c is PadControl => {
  const o = c as Record<string, unknown>;
  if (!o || typeof o !== "object") return false;
  if (Number.isInteger(o.button)) return (o.button as number) >= 0;
  return Number.isInteger(o.axis) && (o.dir === 1 || o.dir === -1);
};

/** Read saved bindings over the defaults. Unknown or malformed entries fall back to defaults. */
export function loadBindings(): Bindings {
  const b = defaultBindings();
  let saved: Partial<Bindings> | null = null;
  try {
    saved = JSON.parse(localStorage.getItem(BINDINGS_KEY) ?? "null");
  } catch {
    saved = null;
  }
  if (!saved || typeof saved !== "object") return b;
  for (const a of ACTIONS) {
    const k = saved.keys?.[a.id];
    if (Array.isArray(k) && k.every((c) => typeof c === "string")) b.keys[a.id] = k.slice(0, SLOTS);
    const p = saved.pad?.[a.id];
    if (Array.isArray(p) && p.every(isPadControl)) b.pad[a.id] = p.slice(0, SLOTS);
  }
  if (typeof saved.padDeadzone === "number" && saved.padDeadzone >= 0 && saved.padDeadzone <= 0.5) b.padDeadzone = saved.padDeadzone;
  if (typeof saved.padCurve === "number" && saved.padCurve >= 1 && saved.padCurve <= 3) b.padCurve = saved.padCurve;
  return b;
}

/** Save bindings; storage may be unavailable (private window), then they last for the session. */
export function saveBindings(b: Bindings): void {
  try {
    localStorage.setItem(BINDINGS_KEY, JSON.stringify(b));
  } catch {
    // ignore
  }
}

/** Forget saved bindings. */
export function clearSavedBindings(): void {
  try {
    localStorage.removeItem(BINDINGS_KEY);
  } catch {
    // ignore
  }
}

/** Same gamepad control? */
export const samePad = (a: PadControl, b: PadControl): boolean =>
  "button" in a ? "button" in b && a.button === b.button : "axis" in b && a.axis === b.axis && a.dir === b.dir;

/** Set slot `slot` of `action` on `device` to `value` (null clears it). The same key or control
 * is removed from every other action first, so one input never drives two actions. Returns the
 * action it was taken from, if any. */
export function assign(b: Bindings, device: "keys", action: ActionId, slot: number, value: string | null): ActionId | null;
export function assign(b: Bindings, device: "pad", action: ActionId, slot: number, value: PadControl | null): ActionId | null;
export function assign(b: Bindings, device: "keys" | "pad", action: ActionId, slot: number, value: string | PadControl | null): ActionId | null {
  let takenFrom: ActionId | null = null;
  if (value !== null) {
    for (const a of ACTIONS) {
      if (device === "keys") {
        const list = b.keys[a.id];
        const i = list.indexOf(value as string);
        if (i >= 0 && !(a.id === action && i === slot)) {
          list.splice(i, 1);
          if (a.id !== action) takenFrom = a.id;
        }
      } else {
        const list = b.pad[a.id];
        const i = list.findIndex((c) => samePad(c, value as PadControl));
        if (i >= 0 && !(a.id === action && i === slot)) {
          list.splice(i, 1);
          if (a.id !== action) takenFrom = a.id;
        }
      }
    }
  }
  const list = (device === "keys" ? b.keys[action] : b.pad[action]) as (string | PadControl)[];
  const at = Math.min(slot, list.length);
  if (value === null) {
    if (slot < list.length) list.splice(slot, 1);
  } else if (at < list.length) list[at] = value;
  else list.push(value);
  return takenFrom;
}

/** Actions bound to a keyboard code. */
export function actionsForKey(b: Bindings, code: string): ActionId[] {
  return ACTIONS.filter((a) => b.keys[a.id].includes(code)).map((a) => a.id);
}

const KEY_NAMES: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  ShiftLeft: "L-Shift",
  ShiftRight: "R-Shift",
  ControlLeft: "L-Ctrl",
  ControlRight: "R-Ctrl",
  AltLeft: "L-Alt",
  AltRight: "R-Alt",
  Backquote: "`",
  Slash: "/",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
};

/** Short display name of a keyboard code (`KeyW` → `W`). */
export function keyName(code: string): string {
  return KEY_NAMES[code] ?? code.replace(/^(Key|Digit)/, "").replace(/^Numpad/, "Num ");
}

const PAD_BUTTONS = ["A", "B", "X", "Y", "LB", "RB", "LT", "RT", "View", "Menu", "L3", "R3", "D-pad ↑", "D-pad ↓", "D-pad ←", "D-pad →", "Home"];
const PAD_AXES = ["LS", "LS", "RS", "RS"];

/** Short display name of a gamepad control (standard mapping names, Xbox labels). */
export function padName(c: PadControl): string {
  if ("button" in c) return PAD_BUTTONS[c.button] ?? `button ${c.button}`;
  const stick = PAD_AXES[c.axis];
  if (!stick) return `axis ${c.axis} ${c.dir > 0 ? "+" : "−"}`;
  const horiz = c.axis % 2 === 0;
  return `${stick} ${horiz ? (c.dir > 0 ? "→" : "←") : c.dir > 0 ? "↓" : "↑"}`;
}

/** One line per action for the F1 help: `throttle — W / ↑ · RT`. */
export function bindingsHelp(b: Bindings): string[] {
  return ACTIONS.map((a) => {
    const k = b.keys[a.id].map(keyName).join(" / ") || "–";
    const p = b.pad[a.id].map(padName).join(" / ");
    return `${a.label} — ${k}${p ? ` · pad ${p}` : ""}`;
  });
}
