/**
 * Input actions, default bindings and the rebinding rules used by the settings screen.
 *
 * Bindings map a game action to one code per device. Codes:
 * - keyboard: `KeyboardEvent.code` (`"KeyW"`, `"Space"`, `"ArrowLeft"`)
 * - gamepad / wheel: `"button:<index>"`, `"axis:<index>+"` or `"axis:<index>-"` (Gamepad API
 *   standard mapping indices; a wheel is any pad whose id looks like a wheel, see `deviceOf`).
 *
 * The host app turns these into `wp_contracts::VehicleInput` each frame (`InputSource` tells
 * `wp_vehicle` which assist defaults apply).
 */

/** Game actions that can be bound. Driving actions map 1:1 to `VehicleInput` fields. */
export type InputAction =
  | "steerLeft"
  | "steerRight"
  | "throttle"
  | "brake"
  | "handbrake"
  | "clutch"
  | "gearUp"
  | "gearDown"
  | "horn"
  | "headlights"
  | "siren"
  | "toolUse"
  | "lookBack"
  | "camera"
  | "map"
  | "pause";

/** Input device families (mirrors `wp_contracts::InputSource`). */
export type InputDevice = "keyboard" | "gamepad" | "wheel";

/** Binding table: action → device → code. */
export type InputBindings = Record<InputAction, Partial<Record<InputDevice, string>>>;

/** Display order and labels. `group` drives the settings layout. */
export const ACTIONS: { action: InputAction; label: string; group: "Driving" | "Car" | "Camera & UI" }[] = [
  { action: "steerLeft", label: "Steer left", group: "Driving" },
  { action: "steerRight", label: "Steer right", group: "Driving" },
  { action: "throttle", label: "Throttle", group: "Driving" },
  { action: "brake", label: "Brake / reverse", group: "Driving" },
  { action: "handbrake", label: "Handbrake", group: "Driving" },
  { action: "clutch", label: "Clutch", group: "Driving" },
  { action: "gearUp", label: "Shift up", group: "Driving" },
  { action: "gearDown", label: "Shift down", group: "Driving" },
  { action: "horn", label: "Horn", group: "Car" },
  { action: "headlights", label: "Headlights", group: "Car" },
  { action: "siren", label: "Siren (police)", group: "Car" },
  { action: "toolUse", label: "Use tool (police)", group: "Car" },
  { action: "lookBack", label: "Look back", group: "Camera & UI" },
  { action: "camera", label: "Change camera", group: "Camera & UI" },
  { action: "map", label: "Map", group: "Camera & UI" },
  { action: "pause", label: "Pause", group: "Camera & UI" },
];

/** Factory defaults. Keyboard matches the testbed's current keys (WASD / arrows, Space, Q/E). */
export function defaultBindings(): InputBindings {
  return {
    steerLeft: { keyboard: "KeyA", gamepad: "axis:0-", wheel: "axis:0-" },
    steerRight: { keyboard: "KeyD", gamepad: "axis:0+", wheel: "axis:0+" },
    throttle: { keyboard: "KeyW", gamepad: "button:7", wheel: "axis:2-" },
    brake: { keyboard: "KeyS", gamepad: "button:6", wheel: "axis:3-" },
    handbrake: { keyboard: "Space", gamepad: "button:0", wheel: "button:4" },
    clutch: { keyboard: "ShiftLeft", wheel: "axis:1-" },
    gearUp: { keyboard: "KeyE", gamepad: "button:5", wheel: "button:5" },
    gearDown: { keyboard: "KeyQ", gamepad: "button:4", wheel: "button:6" },
    horn: { keyboard: "KeyH", gamepad: "button:10", wheel: "button:7" },
    headlights: { keyboard: "KeyL", gamepad: "button:12", wheel: "button:8" },
    siren: { keyboard: "KeyN", gamepad: "button:13", wheel: "button:9" },
    toolUse: { keyboard: "KeyF", gamepad: "button:2", wheel: "button:2" },
    lookBack: { keyboard: "KeyB", gamepad: "button:11", wheel: "button:3" },
    camera: { keyboard: "KeyC", gamepad: "button:3", wheel: "button:1" },
    map: { keyboard: "KeyM", gamepad: "button:8", wheel: "button:10" },
    pause: { keyboard: "Escape", gamepad: "button:9", wheel: "button:0" },
  };
}

/** Actions that must always be bound on the keyboard (so a player cannot lock themselves out). */
export const REQUIRED_KEYBOARD: InputAction[] = ["pause", "throttle", "brake", "steerLeft", "steerRight"];

/** The other action currently using `code` on `device`, if any. */
export function conflictFor(
  b: InputBindings,
  action: InputAction,
  device: InputDevice,
  code: string,
): InputAction | null {
  for (const { action: other } of ACTIONS) {
    if (other !== action && b[other][device] === code) return other;
  }
  return null;
}

/**
 * Bind `code` to `action` on `device`. If another action used that code, the two swap (the
 * other action gets this action's previous code, or becomes unbound). Returns a new table.
 */
export function bind(b: InputBindings, action: InputAction, device: InputDevice, code: string): InputBindings {
  const next: InputBindings = structuredCloneBindings(b);
  const other = conflictFor(b, action, device, code);
  const previous = b[action][device];
  if (other) {
    if (previous !== undefined) next[other][device] = previous;
    else delete next[other][device];
  }
  next[action][device] = code;
  return next;
}

/** Remove a binding. Refuses (returns the same table) for required keyboard actions. */
export function unbind(b: InputBindings, action: InputAction, device: InputDevice): InputBindings {
  if (device === "keyboard" && REQUIRED_KEYBOARD.includes(action)) return b;
  const next = structuredCloneBindings(b);
  delete next[action][device];
  return next;
}

function structuredCloneBindings(b: InputBindings): InputBindings {
  const out = {} as InputBindings;
  for (const k of Object.keys(b) as InputAction[]) out[k] = { ...b[k] };
  return out;
}

/** Which device family a Gamepad API id belongs to. */
export function deviceOf(gamepadId: string): Exclude<InputDevice, "keyboard"> {
  return /wheel|racing|steering|pedal/i.test(gamepadId) ? "wheel" : "gamepad";
}

/** Human-readable label for a binding code. */
export function codeLabel(code: string | undefined): string {
  if (!code) return "—";
  if (code.startsWith("button:")) return `Btn ${code.slice(7)}`;
  if (code.startsWith("axis:")) {
    const sign = code.endsWith("+") ? "+" : "−";
    return `Axis ${code.slice(5, -1)}${sign}`;
  }
  const arrows: Record<string, string> = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" };
  if (arrows[code]) return arrows[code];
  return code
    .replace(/^Key/, "")
    .replace(/^Digit/, "")
    .replace("ShiftLeft", "L-Shift")
    .replace("ShiftRight", "R-Shift")
    .replace("ControlLeft", "L-Ctrl")
    .replace("ControlRight", "R-Ctrl")
    .replace("Escape", "Esc");
}

/**
 * Detect a newly pressed control on a gamepad, comparing with a baseline snapshot taken when the
 * capture started (so a resting trigger or a centred wheel does not bind itself).
 */
export function detectPadInput(
  now: { buttons: number[]; axes: number[] },
  baseline: { buttons: number[]; axes: number[] },
): string | null {
  for (let i = 0; i < now.buttons.length; i++) {
    if (now.buttons[i] > 0.5 && (baseline.buttons[i] ?? 0) <= 0.5) return `button:${i}`;
  }
  for (let i = 0; i < now.axes.length; i++) {
    const d = now.axes[i] - (baseline.axes[i] ?? 0);
    if (Math.abs(d) > 0.6) return `axis:${i}${d > 0 ? "+" : "-"}`;
  }
  return null;
}
