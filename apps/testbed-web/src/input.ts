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

export type InputSource = "Keyboard" | "Gamepad" | "Touch";

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

const kb = { steer: 0, throttle: 0, brake: 0, clutch: 0 };
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

/** Drift Lab W: steer and throttle keys reach the sim raw (−1 / 0 / +1, 0 / 1), as in the
 * reference ("keyboard input is raw"); the rack slew lives in wp_vehicle::plain. */
let rawKeys = false;

/** Turn raw keyboard steer / throttle on (Drift Lab W) or back to the normal key ramps. */
export function setRawKeyboard(on: boolean): void {
  rawKeys = on;
}

function ramp(cur: number, target: number, up: number, down: number, dt: number): number {
  const rate = Math.abs(target) > Math.abs(cur) || Math.sign(target) !== Math.sign(cur) ? up : down;
  const d = target - cur;
  const step = rate * dt;
  return Math.abs(d) <= step ? target : cur + Math.sign(d) * step;
}

/** Every connected gamepad / wheel (a wheel's pedals can be a separate device). */
export function allPads(): Gamepad[] {
  const pads = navigator.getGamepads ? navigator.getGamepads() : [];
  return Array.from(pads).filter((p): p is Gamepad => !!p && p.connected);
}

/** The first connected gamepad, if any. */
export const firstPad = (): Gamepad | null => allPads()[0] ?? null;

/** Raw 0..1 value of one pad control (stick directions clamp at 0; pedals use the full range). */
export function padValue(pad: Gamepad, c: PadControl): number {
  if ("button" in c) return pad.buttons[c.button]?.value ?? 0;
  const a = pad.axes[c.axis] ?? 0;
  if (c.full) return Math.min(1, Math.max(0, (a * c.dir + 1) / 2));
  return Math.max(0, a * c.dir);
}

/** Largest deadzoned value of an action's controls over every connected pad. Bindings are not
 * tied to one device, so a wheel's clutch pedal and a pad's stick can drive together. */
function padAction(pads: Gamepad[], id: ActionId): number {
  let v = 0;
  for (const pad of pads) {
    for (const c of bindings.pad[id]) {
      const dz = "axis" in c && !c.full ? bindings.padDeadzone : DZ_TRIGGER;
      v = Math.max(v, deadzone(padValue(pad, c), dz));
    }
  }
  return v;
}

const keyAction = (id: ActionId): boolean => bindings.keys[id].some((c) => held.has(c));
const keyEdge = (id: ActionId): boolean => bindings.keys[id].some((c) => pressedEdges.has(c));

/** Is this action held right now, on either device? (Camera look-back.) */
export const actionHeld = (id: ActionId): boolean => !blocked && (keyAction(id) || (padHeld[id] ?? 0) > PAD_PRESS);

const NEUTRAL: DriverInput = { steer: 0, throttle: 0, brake: 0, handbrake: 0, clutch: 0, gearUp: false, gearDown: false, horn: false, headlights: false };

/** One frame of an analogue device (gamepad, touch controls). */
export interface AnalogRead {
  input: DriverInput;
  active: boolean;
  /** Edge actions pressed this frame (other than gears, which go into the input). */
  edges: ActionId[];
}
type PadRead = AnalogRead;

/** Touch controls (src/touch.ts) register here; read every frame like the gamepad. */
let touchReader: ((dt: number) => AnalogRead | null) | null = null;

/** Hook an extra analogue device into `readInput` (touch controls). */
export function setTouchReader(fn: ((dt: number) => AnalogRead | null) | null): void {
  touchReader = fn;
}

function readGamepad(): PadRead | null {
  const pads = allPads();
  if (!pads.length) {
    if (Object.keys(padHeld).length) releasePad();
    return null;
  }
  const values: Partial<Record<ActionId, number>> = {};
  const edges: ActionId[] = [];
  const now: Partial<Record<ActionId, boolean>> = {};
  for (const a of ACTIONS) {
    const v = padAction(pads, a.id);
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
    clutch: values.clutch ?? 0,
    gearUp: rising("gearUp"),
    gearDown: rising("gearDown"),
    horn: !!now.horn,
    headlights,
  };
  padPrev = now;
  padHeld = values;
  const active = Object.values(values).some((v) => (v ?? 0) > 0) || pads.some((p) => p.buttons.some((b) => b.pressed));
  return { input, active, edges };
}

/** Read the current input. `dt` drives the keyboard ramps. Switches the source to whichever
 * device was used last. `edges` are the pad's edge actions this frame (keyboard edge actions
 * arrive through `installKeyboard`'s callback). */
export function readInput(dt: number): { input: DriverInput; source: InputSource; edges: ActionId[] } {
  const pad = readGamepad();
  const touch = touchReader?.(dt) ?? null;
  if (blocked) {
    pressedEdges.clear();
    kb.steer = kb.throttle = kb.brake = kb.clutch = 0;
    return { input: { ...NEUTRAL, headlights }, source, edges: [] };
  }
  const left = keyAction("steerLeft") ? 1 : 0;
  const right = keyAction("steerRight") ? 1 : 0;
  const keyActive = left || right || keyAction("throttle") || keyAction("brake") || keyAction("handbrake") || keyAction("clutch");
  // Steer keys go to the sim raw (−1 / 0 / +1), as in the owner's reference game: the key
  // ramp (6/s in and out, row 102) is the car's steering model now
  // (wp_vehicle::steering::key_steer), so replays, the AI and every client see the same
  // steering path, and in a slide or under the handbrake the key acts at once.
  kb.steer = right - left;
  // Fast key ramp: the sim's keyboard assist preset filters the pedal again (~0.23 s).
  const thrKey = keyAction("throttle") ? 1 : 0;
  kb.throttle = rawKeys ? thrKey : ramp(kb.throttle, thrKey, 12.0, 12.0, dt);
  kb.brake = ramp(kb.brake, keyAction("brake") ? 1 : 0, 6.0, 10.0, dt);
  // Clutch key: in fast (0.1 s), out over ~0.3 s so a key release can still launch the car.
  kb.clutch = ramp(kb.clutch, keyAction("clutch") ? 1 : 0, 10.0, 3.5, dt);
  const keyboard: DriverInput = {
    steer: kb.steer,
    throttle: kb.throttle,
    brake: kb.brake,
    handbrake: keyAction("handbrake") ? 1 : 0,
    clutch: kb.clutch,
    gearUp: keyEdge("gearUp"),
    gearDown: keyEdge("gearDown"),
    horn: keyAction("horn"),
    headlights,
  };
  pressedEdges.clear();
  if (pad?.active) source = "Gamepad";
  else if (touch?.active) source = "Touch";
  else if (keyActive) source = "Keyboard";
  const edges = touch?.edges.length ? [...(pad?.edges ?? []), ...touch.edges] : (pad?.edges ?? []);
  if (source === "Touch" && touch) {
    // Touch values go to the sim as read, like the pad's; keyboard gears / horn still work.
    const input = { ...touch.input, headlights };
    input.gearUp ||= keyboard.gearUp;
    input.gearDown ||= keyboard.gearDown;
    input.horn ||= keyboard.horn;
    return { input, source, edges };
  }
  if (source === "Gamepad" && pad) {
    const input = pad.input;
    // Keyboard gear edges, horn and clutch still work alongside a pad.
    input.clutch = Math.max(input.clutch, keyboard.clutch);
    input.gearUp ||= keyboard.gearUp;
    input.gearDown ||= keyboard.gearDown;
    input.horn ||= keyboard.horn;
    return { input, source, edges };
  }
  return { input: keyboard, source, edges };
}
