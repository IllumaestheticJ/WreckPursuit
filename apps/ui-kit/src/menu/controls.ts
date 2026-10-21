/**
 * Per-device input shaping ("Advanced" controller settings): steering dead zone, saturation,
 * sensitivity and speed sensitivity, plus dead zone + saturation for each pedal.
 *
 * Pure functions + data, no DOM. The host applies `shapeSteer` / `shapePedal` to the device's
 * value after its own processing (keyboard ramps, gamepad deadzone + curve) and passes
 * `steerSpeedSensitivity` to the sim as `AssistProfile.steer_speed_sensitivity` when steering
 * help is on.
 *
 * The formulas are mirrored in Rust by the lead's measurement suite: do not change them without
 * changing that copy too.
 */

/** Which input device a profile belongs to. A gamepad whose id mentions "wheel" is a wheel. */
export type ControlDevice = "keyboard" | "gamepad" | "wheel";

/** Every device, in menu order. */
export const CONTROL_DEVICES: readonly ControlDevice[] = ["keyboard", "gamepad", "wheel"];

/** Shaping settings of one device. All values are fractions (the menu shows them as %). */
export interface DeviceControls {
  /** Steering input below this is ignored, 0..0.5. */
  steerDeadzone: number;
  /** Steering input at or above this reads as full lock, 0.5..1. */
  steerSaturation: number;
  /** Steering response, 0..1: 1 = linear, lower = finer near centre (exponent up to 3). */
  steerSensitivity: number;
  /** Less steering lock at speed, 0..1 (→ `AssistProfile.steer_speed_sensitivity`; 0 = off). */
  steerSpeedSensitivity: number;
  /** Throttle dead zone, 0..0.5. */
  throttleDeadzone: number;
  /** Throttle saturation, 0.5..1. */
  throttleSaturation: number;
  /** Brake dead zone, 0..0.5. */
  brakeDeadzone: number;
  /** Brake saturation, 0.5..1. */
  brakeSaturation: number;
  /** Clutch dead zone, 0..0.5. */
  clutchDeadzone: number;
  /** Clutch saturation, 0.5..1. */
  clutchSaturation: number;
}

/** Valid range of each field: `[min, max]`. */
export const CONTROL_RANGES: Readonly<Record<keyof DeviceControls, readonly [number, number]>> = {
  steerDeadzone: [0, 0.5],
  steerSaturation: [0.5, 1],
  steerSensitivity: [0, 1],
  steerSpeedSensitivity: [0, 1],
  throttleDeadzone: [0, 0.5],
  throttleSaturation: [0.5, 1],
  brakeDeadzone: [0, 0.5],
  brakeSaturation: [0.5, 1],
  clutchDeadzone: [0, 0.5],
  clutchSaturation: [0.5, 1],
};

const pedalsIdentity = {
  throttleDeadzone: 0,
  throttleSaturation: 1,
  brakeDeadzone: 0,
  brakeSaturation: 1,
  clutchDeadzone: 0,
  clutchSaturation: 1,
};

/** Default profile of one device (identity shaping; only the speed sensitivity differs). */
export function defaultDeviceControls(device: ControlDevice): DeviceControls {
  const speed = device === "keyboard" ? 0.75 : device === "gamepad" ? 0.7 : 0;
  return { steerDeadzone: 0, steerSaturation: 1, steerSensitivity: 1, steerSpeedSensitivity: speed, ...pedalsIdentity };
}

/** Default profiles of every device. */
export function defaultControls(): Record<ControlDevice, DeviceControls> {
  return { keyboard: defaultDeviceControls("keyboard"), gamepad: defaultDeviceControls("gamepad"), wheel: defaultDeviceControls("wheel") };
}

/** One device's profile from untrusted data (stored JSON): each field clamped into its range,
 * missing / non-numeric fields from `base`. */
export function normalizeDeviceControls(raw: unknown, base: DeviceControls): DeviceControls {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const out = { ...base };
  for (const k of Object.keys(CONTROL_RANGES) as (keyof DeviceControls)[]) {
    const v = r[k];
    const [lo, hi] = CONTROL_RANGES[k];
    if (typeof v === "number" && Number.isFinite(v)) out[k] = Math.min(hi, Math.max(lo, v));
  }
  return out;
}

/** Every device's profile from untrusted data (missing devices from `base`). */
export function normalizeControls(raw: unknown, base: Record<ControlDevice, DeviceControls>): Record<ControlDevice, DeviceControls> {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  return {
    keyboard: normalizeDeviceControls(r.keyboard, base.keyboard),
    gamepad: normalizeDeviceControls(r.gamepad, base.gamepad),
    wheel: normalizeDeviceControls(r.wheel, base.wheel),
  };
}

/** Dead zone + saturation of a magnitude `a` (0..1): 0 at or below `dz`, 1 at or above `sat`,
 * linear between. */
function window01(a: number, dz: number, sat: number): number {
  return a <= dz ? 0 : Math.min(1, (a - dz) / Math.max(1e-3, sat - dz));
}

/** Shaped steering, -1..1: dead zone + saturation on |x|, then the sensitivity exponent
 * `1 + 2·(1 − sensitivity)` (1 = linear, 3 = finest near centre). The sign is kept. */
export function shapeSteer(x: number, c: DeviceControls): number {
  if (!Number.isFinite(x)) return 0;
  const a = Math.abs(x);
  const t = window01(a, c.steerDeadzone, c.steerSaturation);
  const exp = 1 + 2 * (1 - c.steerSensitivity);
  return Math.sign(x) * t ** exp;
}

/** Shaped pedal, 0..1: dead zone + saturation, no exponent. */
export function shapePedal(x: number, dz: number, sat: number): number {
  if (!Number.isFinite(x)) return 0;
  return window01(Math.max(0, x), dz, sat);
}

/** Shaped pedals of one device (throttle, brake, clutch). */
export function shapePedals(p: { throttle: number; brake: number; clutch: number }, c: DeviceControls): { throttle: number; brake: number; clutch: number } {
  return {
    throttle: shapePedal(p.throttle, c.throttleDeadzone, c.throttleSaturation),
    brake: shapePedal(p.brake, c.brakeDeadzone, c.brakeSaturation),
    clutch: shapePedal(p.clutch, c.clutchDeadzone, c.clutchSaturation),
  };
}

/** Processed input values the host reports for the menu's test bars. */
export interface InputProbe {
  /** Which device the values come from (the profile in use). */
  device: ControlDevice;
  /** -1..1 */
  steer: number;
  /** 0..1 */
  throttle: number;
  /** 0..1 */
  brake: number;
  /** 0..1 */
  clutch: number;
}
