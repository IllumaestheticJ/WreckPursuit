// Keyboard + Gamepad API → DriverInput, through the rebindable bindings (bindings.ts).
//
// Keyboard values ramp so digital keys are not 0→1 steps (the sim's AssistProfile filters on top
// of that). Gamepad values go to the sim as read (deadzone + steering curve only, no ramps), so
// the sim's gamepad assist preset is the only filter. The source switches to whichever device
// was used last; main applies the matching assist preset.
//
// Everything held is released when it could miss its release: window blur, tab hidden, context
// menu, focus moving into a form control (keyboard), and the pad disconnecting.
import type { DriverInput } from "./wasm/sim";
import { ACTIONS, loadBindings, type ActionId, type Bindings, type PadControl } from "./bindings";

export type InputSource = "Keyboard" | "Gamepad";

const held = new Set<string>();
const pressedEdges = new Set<string>();

let bindings: Bindings = loadBindings();

/** The live bindings (the settings panel edits this object, then calls `setBindings`). */
export const getBindings = (): Bindings => bindings;

/** Replace the live bindings. */
export function setBindings(b: Bindings): void {
  bindings = b;
}

/** While set, the next keydown goes to this callback only (rebinding). */
let keyCapture: ((e: KeyboardEvent) => void) | null = null;

/** Send the next keydown to `cb` instead of the game (settings panel). `null` stops. */
export function captureKey(cb: ((e: KeyboardEvent) => void) | null): void {
  keyCapture = cb;
}

/** While true (settings panel open) the driving input is neutral and no actions fire. */
let blocked = false;

/** Block / unblock game input (settings panel). Releases everything held. */
export function setInputBlocked(on: boolean): void {
  blocked = on;
  held.clear();
  pressedEdges.clear();
}

/** Is game input blocked by the settings panel? */
export const inputBlocked = (): boolean => blocked;

/** `onKey` gets every game keydown (not repeats' edges; check `e.repeat`). */
export function installKeyboard(onKey: (e: KeyboardEvent) => void): void {
  // Capture phase so a rebind swallows the key before anything else (audio, map, layers) sees it.
  window.addEventListener(
    "keydown",
    (e) => {
      if (!keyCapture) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (!e.repeat) keyCapture(e);
    },
    { capture: true },
  );
  window.addEventListener("keydown", (e) => {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "SELECT") return;
    if (!e.repeat) pressedEdges.add(e.code);
    held.add(e.code);
    if (e.code.startsWith("Arrow") || e.code === "Space" || e.code.startsWith("Control")) e.preventDefault();
    onKey(e);
  });
  window.addEventListener("keyup", (e) => {
    held.delete(e.code);
    // macOS sends no keyup for keys released while Cmd is down: drop everything with it.
    if (e.key === "Meta") held.clear();
  });
  // Anything that can swallow a keyup releases every key instead of leaving one stuck down:
  // losing focus, switching tabs, a context menu, or focus moving into a form control.
  const releaseAll = () => held.clear();
  window.addEventListener("blur", releaseAll);
  document.addEventListener("visibilitychange", releaseAll);
  window.addEventListener("contextmenu", releaseAll);
  // A clicked button keeps keyboard focus, so Space (handbrake) or Enter would press it again
  // (restart the chase, re-run a scenario...). Hand focus back to the page after every click.
  window.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement)?.closest?.("button");
    if (el instanceof HTMLButtonElement) el.blur();
  });
  window.addEventListener("focusin", (e) => {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") releaseAll();
  });
  window.addEventListener("gamepadconnected", (e) => console.info(`gamepad connected: ${(e as GamepadEvent).gamepad.id}`));
  window.addEventListener("gamepaddisconnected", () => releasePad());
}

/** Apply a deadzone and rescale to 0..1 (sign kept). */
export function deadzone(v: number, dz: number): number {
  const a = Math.abs(v);
  if (a < dz) return 0;
  return (Math.sign(v) * (a - dz)) / (1 - dz);
}

/** Steering response curve: `sign(v)·|v|^exp` (exp 1 = linear). */
export const curve = (v: number, exp: number): number => Math.sign(v) * Math.abs(v) ** exp;

const DZ_TRIGGER = 0.05;
/** Analogue value above which a pad control counts as pressed (edges, held actions, rebinding). */
export const PAD_PRESS = 0.5;

const kb = { steer: 0, throttle: 0, brake: 0 };
let padPrev: Partial<Record<ActionId, boolean>> = {};
let source: InputSource = "Keyboard";
let headlights = false;
let padHeld: Partial<Record<ActionId, number>> = {};

/** Forget the pad's state: nothing stays held and the source falls back to the keyboard. */
function releasePad(): void {
  padPrev = {};
  padHeld = {};
  if (!firstPad()) source = "Keyboard";
}

/** Toggle headlights. */
export function toggleHeadlights(): void {
  headlights = !headlights;
}

function ramp(cur: number, target: number, up: number, down: number, dt: number): number {
  const rate = Math.abs(target) > Math.abs(cur) || Math.sign(target) !== Math.sign(cur) ? up : down;
  const d = target - cur;
  const step = rate * dt;
  return Math.abs(d) <= step ? target : cur + Math.sign(d) * step;
}

/** The first connected gamepad, if any. */
export function firstPad(): Gamepad | null {
  const pads = navigator.getGamepads ? navigator.getGamepads() : [];
  return Array.from(pads).find((p): p is Gamepad => !!p && p.connected) ?? null;
}

/** Raw 0..1 value of one pad control (axis directions clamp at 0). */
export function padValue(pad: Gamepad, c: PadControl): number {
  if ("button" in c) return pad.buttons[c.button]?.value ?? 0;
  return Math.max(0, (pad.axes[c.axis] ?? 0) * c.dir);
}

/** Largest deadzoned value of an action's pad controls. */
function padAction(pad: Gamepad, id: ActionId): number {
  let v = 0;
  for (const c of bindings.pad[id]) {
    const dz = "button" in c ? DZ_TRIGGER : bindings.padDeadzone;
    v = Math.max(v, deadzone(padValue(pad, c), dz));
  }
  return v;
}

const keyAction = (id: ActionId): boolean => bindings.keys[id].some((c) => held.has(c));
const keyEdge = (id: ActionId): boolean => bindings.keys[id].some((c) => pressedEdges.has(c));

/** Is this action held right now, on either device? (Camera look-back.) */
export const actionHeld = (id: ActionId): boolean => !blocked && (keyAction(id) || (padHeld[id] ?? 0) > PAD_PRESS);

const NEUTRAL: DriverInput = { steer: 0, throttle: 0, brake: 0, handbrake: 0, clutch: 0, gearUp: false, gearDown: false, horn: false, headlights: false };

interface PadRead {
  input: DriverInput;
  active: boolean;
  /** Edge actions pressed this frame (other than gears, which go into the input). */
  edges: ActionId[];
}

function readGamepad(): PadRead | null {
  const pad = firstPad();
  if (!pad) {
    if (Object.keys(padHeld).length) releasePad();
    return null;
  }
  const values: Partial<Record<ActionId, number>> = {};
  const edges: ActionId[] = [];
  const now: Partial<Record<ActionId, boolean>> = {};
  for (const a of ACTIONS) {
    const v = padAction(pad, a.id);
    values[a.id] = v;
    now[a.id] = v > PAD_PRESS;
    if (a.kind === "edge" && now[a.id] && !padPrev[a.id] && a.id !== "gearUp" && a.id !== "gearDown") edges.push(a.id);
  }
  const rising = (id: ActionId) => !!now[id] && !padPrev[id];
  const steer = (values.steerRight ?? 0) - (values.steerLeft ?? 0);
  const input: DriverInput = {
    steer: curve(steer, bindings.padCurve),
    throttle: values.throttle ?? 0,
    brake: values.brake ?? 0,
    handbrake: now.handbrake ? 1 : 0,
    clutch: 0,
    gearUp: rising("gearUp"),
    gearDown: rising("gearDown"),
    horn: !!now.horn,
    headlights,
  };
  padPrev = now;
  padHeld = values;
  const active = Object.values(values).some((v) => (v ?? 0) > 0) || pad.buttons.some((b) => b.pressed);
  return { input, active, edges };
}

/** Read the current input. `dt` drives the keyboard ramps. Switches the source to whichever
 * device was used last. `edges` are the pad's edge actions this frame (keyboard edge actions
 * arrive through `installKeyboard`'s callback). */
export function readInput(dt: number): { input: DriverInput; source: InputSource; edges: ActionId[] } {
  const pad = readGamepad();
  if (blocked) {
    pressedEdges.clear();
    kb.steer = kb.throttle = kb.brake = 0;
    return { input: { ...NEUTRAL, headlights }, source, edges: [] };
  }
  const left = keyAction("steerLeft") ? 1 : 0;
  const right = keyAction("steerRight") ? 1 : 0;
  const keyActive = left || right || keyAction("throttle") || keyAction("brake") || keyAction("handbrake");
  kb.steer = ramp(kb.steer, right - left, 4.0, 6.0, dt);
  // Fast key ramp: the sim's keyboard assist preset filters the pedal again (~0.23 s).
  kb.throttle = ramp(kb.throttle, keyAction("throttle") ? 1 : 0, 12.0, 12.0, dt);
  kb.brake = ramp(kb.brake, keyAction("brake") ? 1 : 0, 6.0, 10.0, dt);
  const keyboard: DriverInput = {
    steer: kb.steer,
    throttle: kb.throttle,
    brake: kb.brake,
    handbrake: keyAction("handbrake") ? 1 : 0,
    clutch: 0,
    gearUp: keyEdge("gearUp"),
    gearDown: keyEdge("gearDown"),
    horn: keyAction("horn"),
    headlights,
  };
  pressedEdges.clear();
  if (pad?.active) source = "Gamepad";
  else if (keyActive) source = "Keyboard";
  const edges = pad?.edges ?? [];
  if (source === "Gamepad" && pad) {
    const input = pad.input;
    // Keyboard gear edges and horn still work alongside a pad.
    input.gearUp ||= keyboard.gearUp;
    input.gearDown ||= keyboard.gearDown;
    input.horn ||= keyboard.horn;
    return { input, source, edges };
  }
  return { input: keyboard, source, edges };
}
