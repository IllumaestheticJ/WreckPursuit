// Typed wrapper over the wp_wasm JSON API. Prompt 04 replaces JSON with packed arrays.
import initWasm, * as wasm from "./pkg/wp_wasm.js";

/** Mirrors wp_contracts::VehicleInput (all fields optional; missing = 0/false). */
export interface VehicleInput {
  steer?: number;
  throttle?: number;
  brake?: number;
  handbrake?: number;
  clutch?: number;
  gear_up?: boolean;
  gear_down?: boolean;
  horn?: boolean;
  headlights?: boolean;
  siren?: boolean;
  tool_use?: number;
}

/** Subset of wp_contracts::VehicleState the testbed reads. */
export interface VehicleState {
  pos: [number, number, number];
  rot: [number, number, number, number];
  speed_mps: number;
  engine_rpm: number;
  gear: number;
  health: number;
}

export interface VehicleInfo {
  entity: number;
  car_id: string;
  body_size_m: [number, number, number];
}

export async function initSim(): Promise<void> {
  await initWasm();
  wasm.init();
}

export function stepSim(dtSeconds: number): number {
  return wasm.step(dtSeconds);
}

export function vehicleStates(): { entity: number; state: VehicleState }[] {
  return JSON.parse(wasm.get_vehicle_state_json());
}

export function vehicleInfo(): VehicleInfo[] {
  return JSON.parse(wasm.get_vehicle_info_json());
}

export function setInput(input: VehicleInput): void {
  wasm.set_input_json(JSON.stringify(input));
}

export function resetVehicle(): void {
  wasm.reset_vehicle();
}
