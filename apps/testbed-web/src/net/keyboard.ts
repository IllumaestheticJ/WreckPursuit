// Minimal keyboard → NetInput for the net page (kept separate from the testbed's input.ts).
import type { NetInput } from "./wasm";

const held = new Set<string>();
let lights = true;
let siren = false;

export function installKeyboard(): void {
  window.addEventListener("keydown", (e) => {
    if (e.target instanceof HTMLInputElement) return;
    held.add(e.code);
    if (e.code === "KeyL" && !e.repeat) lights = !lights;
    if (e.code === "KeyQ" && !e.repeat) siren = !siren;
    if (e.code.startsWith("Arrow") || e.code === "Space") e.preventDefault();
  });
  window.addEventListener("keyup", (e) => held.delete(e.code));
  window.addEventListener("blur", () => held.clear());
}

const any = (...codes: string[]) => codes.some((c) => held.has(c));

export function readInput(): NetInput {
  const left = any("ArrowLeft", "KeyA") ? 1 : 0;
  const right = any("ArrowRight", "KeyD") ? 1 : 0;
  return {
    steer: right - left,
    throttle: any("ArrowUp", "KeyW") ? 1 : 0,
    brake: any("ArrowDown", "KeyS") ? 1 : 0,
    handbrake: any("Space") ? 1 : 0,
    headlights: lights,
    siren,
    horn: any("KeyH"),
  };
}
