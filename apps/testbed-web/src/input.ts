// Keyboard → VehicleInput. Arrows or WASD drive, Space = handbrake, R = reset.
import type { VehicleInput } from "./wasm/sim";

const held = new Set<string>();

export function installKeyboard(onReset: () => void): void {
  window.addEventListener("keydown", (e) => {
    if (e.code === "KeyR" && !e.repeat) onReset();
    held.add(e.code);
    if (e.code.startsWith("Arrow") || e.code === "Space") e.preventDefault();
  });
  window.addEventListener("keyup", (e) => held.delete(e.code));
  window.addEventListener("blur", () => held.clear());
}

const any = (...codes: string[]) => codes.some((c) => held.has(c));

export function readInput(): VehicleInput {
  const left = any("ArrowLeft", "KeyA") ? 1 : 0;
  const right = any("ArrowRight", "KeyD") ? 1 : 0;
  return {
    steer: right - left,
    throttle: any("ArrowUp", "KeyW") ? 1 : 0,
    brake: any("ArrowDown", "KeyS") ? 1 : 0,
    handbrake: any("Space") ? 1 : 0,
  };
}
