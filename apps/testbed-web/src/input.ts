// Keyboard + Gamepad API → DriverInput. Keyboard: WASD/arrows, Space handbrake, Shift/Ctrl
// gear up/down (edges). Gamepad (standard mapping): left stick steer, RT throttle, LT brake,
// A/X handbrake, RB/LB gears, with deadzones. Keyboard values ramp so digital keys are not
// 0→1 steps (the sim's AssistProfile filters on top of that).
import type { DriverInput } from "./wasm/sim";

export type InputSource = "Keyboard" | "Gamepad";

const held = new Set<string>();
const pressedEdges = new Set<string>();

export function installKeyboard(onKey: (e: KeyboardEvent) => void): void {
  window.addEventListener("keydown", (e) => {
    if ((e.target as HTMLElement)?.tagName === "INPUT" || (e.target as HTMLElement)?.tagName === "SELECT") return;
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
}

/** Is this key (KeyboardEvent.code) held right now? (Camera look-back.) */
export const isHeld = (code: string): boolean => held.has(code);

const any = (...codes: string[]) => codes.some((c) => held.has(c));
const edge = (...codes: string[]) => codes.some((c) => pressedEdges.has(c));

/** Apply a radial-ish deadzone and rescale to 0..1. */
export function deadzone(v: number, dz: number): number {
  const a = Math.abs(v);
  if (a < dz) return 0;
  return (Math.sign(v) * (a - dz)) / (1 - dz);
}

const DZ_STICK = 0.12;
const DZ_TRIGGER = 0.05;

const kb = { steer: 0, throttle: 0, brake: 0 };
let lastPadButtons: boolean[] = [];
let source: InputSource = "Keyboard";
let headlights = false;

/** Toggle headlights (bound to the "O" key in main). */
export function toggleHeadlights(): void {
  headlights = !headlights;
}

function ramp(cur: number, target: number, up: number, down: number, dt: number): number {
  // A reversal (countersteering against a slide) starts from centre at once instead of first
  // ramping back through it (owner row 90); the sim's rack still moves the wheels smoothly.
  if (target !== 0 && cur !== 0 && Math.sign(target) !== Math.sign(cur)) cur = 0;
  const rate = Math.abs(target) > Math.abs(cur) || Math.sign(target) !== Math.sign(cur) ? up : down;
  const d = target - cur;
  const step = rate * dt;
  return Math.abs(d) <= step ? target : cur + Math.sign(d) * step;
}

function readGamepad(): (DriverInput & { active: boolean }) | null {
  const pads = navigator.getGamepads ? navigator.getGamepads() : [];
  const pad = Array.from(pads).find((p) => p && p.connected);
  if (!pad) return null;
  const btn = (i: number) => pad.buttons[i]?.value ?? 0;
  const pressed = pad.buttons.map((b) => b.pressed);
  const rising = (i: number) => pressed[i] && !lastPadButtons[i];
  const out = {
    steer: deadzone(pad.axes[0] ?? 0, DZ_STICK),
    throttle: deadzone(btn(7), DZ_TRIGGER),
    brake: deadzone(btn(6), DZ_TRIGGER),
    handbrake: btn(0) > 0.5 || btn(2) > 0.5 ? 1 : 0,
    clutch: 0,
    gearUp: rising(5),
    gearDown: rising(4),
    horn: btn(3) > 0.5,
    headlights,
    active: false,
  };
  lastPadButtons = pressed;
  out.active =
    Math.abs(out.steer) > 0 || out.throttle > 0 || out.brake > 0 || pressed.some((p) => p);
  return out;
}

/** Read the current input. `dt` drives the keyboard ramps. Switches source to whichever
 * device was used last. */
export function readInput(dt: number): { input: DriverInput; source: InputSource } {
  const left = any("ArrowLeft", "KeyA") ? 1 : 0;
  const right = any("ArrowRight", "KeyD") ? 1 : 0;
  const keyActive = left || right || any("ArrowUp", "KeyW", "ArrowDown", "KeyS", "Space");
  // Release at 12/s (83 ms; was 6/s): until the key value reaches 0 the sim's self-aligning
  // countersteer cannot start (owner row 90).
  kb.steer = ramp(kb.steer, right - left, 4.0, 12.0, dt);
  // Fast key ramp: the sim's keyboard assist preset filters the pedal again (~0.23 s).
  kb.throttle = ramp(kb.throttle, any("ArrowUp", "KeyW") ? 1 : 0, 12.0, 12.0, dt);
  kb.brake = ramp(kb.brake, any("ArrowDown", "KeyS") ? 1 : 0, 6.0, 10.0, dt);
  const keyboard: DriverInput = {
    steer: kb.steer,
    throttle: kb.throttle,
    brake: kb.brake,
    handbrake: any("Space") ? 1 : 0,
    clutch: 0,
    gearUp: edge("ShiftLeft", "ShiftRight"),
    gearDown: edge("ControlLeft", "ControlRight"),
    horn: any("KeyQ"),
    headlights,
  };
  pressedEdges.clear();
  const pad = readGamepad();
  if (pad?.active) source = "Gamepad";
  else if (keyActive) source = "Keyboard";
  if (source === "Gamepad" && pad) {
    const { active: _active, ...input } = pad;
    // Keyboard gear edges still work alongside a pad.
    input.gearUp ||= keyboard.gearUp;
    input.gearDown ||= keyboard.gearDown;
    return { input, source };
  }
  return { input: keyboard, source };
}
