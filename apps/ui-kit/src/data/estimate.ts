/**
 * Mock of `wp_economy::Economy::preview`: applies mods to a `VehicleDef` and estimates
 * performance + class index.
 *
 * `estimate` and `classIndex` are line-by-line ports of `crates/wp_economy/src/estimate.rs` so
 * the before/after numbers in the mod tree match the economy to within float noise.
 * `applyMods` handles only the parameters the estimate reads (power, gearing, mass, grip, brakes,
 * aero); handling-only parameters (springs, dampers, diffs...) are listed as "handling" deltas.
 * The real preview arrives through wp_wasm (request UI-5 in docs/integration/ui.md).
 */
import type { BodyRegion, VehicleDef } from "../generated/contracts";
import { MODS, type ModDef } from "./catalog";

const RHO = 1.225;
const CRR = 0.013;
const ETA = 0.85;
const RPM_TO_RADS = 0.10471976;
const REF_TIRE_WIDTH_M = 0.215;
const G = 9.81;

/** `data/economy/market.toml` `[tire_grip]`. */
export const TIRE_GRIP: Record<string, number> = {
  street: 1.0,
  sport: 1.08,
  semi_slick: 1.18,
  drag_radial: 1.03,
  all_terrain: 0.92,
};

/** `market.toml` `[class_index]`. */
const CLASS_INDEX = {
  base: 100,
  perWPerKg: 2.2,
  perLateralG: 250,
  perBrakeG: 80,
  min: 100,
  max: 999,
  brackets: [
    ["D", 100],
    ["C", 300],
    ["B", 450],
    ["A", 600],
    ["S", 750],
    ["X", 900],
  ] as [string, number][],
};

/** `wp_economy::PerfMetrics`. */
export interface PerfMetrics {
  peakPowerKw: number;
  peakPowerRpm: number;
  massKg: number;
  powerToWeight: number;
  est0to100S: number;
  estTopSpeedMps: number;
  lateralGripG: number;
  brakeDecelG: number;
}

/** Full-load torque at `rpm` (port of `torque_at`). */
export function torqueAt(curve: [number, number][], rpm: number): number {
  if (curve.length === 0) return 0;
  const [r0, t0] = curve[0];
  if (rpm <= r0) return t0;
  for (let i = 0; i + 1 < curve.length; i++) {
    const [ra, ta] = curve[i];
    const [rb, tb] = curve[i + 1];
    if (rpm <= rb) return ta + ((tb - ta) * (rpm - ra)) / (rb - ra);
  }
  return 0;
}

/** Port of `wp_economy::estimate::estimate`. */
export function estimate(def: VehicleDef, grip: number): PerfMetrics {
  const e = def.engine;
  const m = def.mass_kg;
  let peakW = 0;
  let peakRpm = e.idle_rpm;
  const steps = 128;
  for (let i = 0; i <= steps; i++) {
    const rpm = e.idle_rpm + ((e.rev_limit_rpm - e.idle_rpm) * i) / steps;
    const p = torqueAt(e.torque_curve_nm, rpm) * rpm * RPM_TO_RADS;
    if (p > peakW) {
      peakW = p;
      peakRpm = rpm;
    }
  }
  const layout = def.drivetrain.layout;
  const r = layout === "Fwd" ? def.wheels[0].radius_m : def.wheels[2].radius_m;
  const fd = def.drivetrain.final_drive;
  const gears = def.drivetrain.gear_ratios;
  const cda = def.drag_cd_a_m2;
  const roll = CRR * m * G;
  const rpmOf = (v: number, ratio: number) => ((v / r) * ratio * fd) / RPM_TO_RADS;
  const wheelForce = (rpm: number, ratio: number) => (torqueAt(e.torque_curve_nm, rpm) * ratio * fd * ETA) / r;

  let vmax = 0;
  for (let v = 0.5; v <= 120; v += 0.1) {
    const resist = 0.5 * RHO * cda * v * v + roll;
    const ok = gears.some((ratio) => {
      const rpm = rpmOf(v, ratio);
      return rpm <= e.rev_limit_rpm && wheelForce(Math.max(rpm, e.idle_rpm), ratio) >= resist;
    });
    if (ok) vmax = v;
  }

  const w = def.wheels.reduce((s, wh) => s + wh.width_m, 0) / 4;
  const mu = Math.max(grip * (1 + (w - REF_TIRE_WIDTH_M)), 0.1);
  const launchRpm = 0.6 * e.redline_rpm;
  const dt = 0.01;
  let t = 0;
  let vel = 0;
  let accPrev = 0;
  let gear = 0;
  let shiftTimer = 0;
  while (vel < 27.78 && t < 60) {
    if (gear + 1 < gears.length && rpmOf(vel, gears[gear]) >= 0.98 * e.redline_rpm) {
      gear += 1;
      shiftTimer = def.drivetrain.shift_time_s;
    }
    const ratio = gears[gear];
    let f: number;
    if (shiftTimer > 0) {
      shiftTimer -= dt;
      f = 0;
    } else {
      const rpm = rpmOf(vel, ratio);
      const torque =
        gear === 0 && rpm < launchRpm
          ? Math.min(torqueAt(e.torque_curve_nm, launchRpm), def.drivetrain.clutch_max_nm)
          : torqueAt(e.torque_curve_nm, Math.min(Math.max(rpm, e.idle_rpm), e.rev_limit_rpm));
      f = (torque * ratio * fd * ETA) / r;
    }
    const transfer = (m * accPrev * def.cg_height_m) / def.wheelbase_m;
    const nDriven =
      layout === "Fwd"
        ? m * G * def.front_weight_frac - transfer
        : layout === "Rwd"
          ? m * G * (1 - def.front_weight_frac) + transfer
          : m * G;
    f = Math.min(f, mu * Math.max(nDriven, 0));
    const a = (f - 0.5 * RHO * cda * vel * vel - roll) / (m * 1.04);
    vel = Math.max(vel + a * dt, 0);
    accPrev = Math.max(a, 0);
    t += dt;
  }

  const vRef = 30;
  const cla = Math.max(def.aero_downforce_coeffs[0] + def.aero_downforce_coeffs[1] - def.lift_front - def.lift_rear, 0);
  const df = 0.5 * RHO * cla * vRef * vRef;
  const lateral = mu * (1 + df / (m * G));
  const b = def.brakes;
  const brakeForce = 2 * (b.max_torque_front_nm / def.wheels[0].radius_m + b.max_torque_rear_nm / def.wheels[2].radius_m);
  const brake = Math.min(brakeForce / (m * G), lateral);
  return {
    peakPowerKw: peakW / 1000,
    peakPowerRpm: peakRpm,
    massKg: m,
    powerToWeight: peakW / m,
    est0to100S: Math.min(t, 60),
    estTopSpeedMps: vmax,
    lateralGripG: lateral,
    brakeDecelG: brake,
  };
}

/** Port of `class_index`. */
export function classIndex(p: PerfMetrics): number {
  const c = CLASS_INDEX;
  const pi = c.base + c.perWPerKg * p.powerToWeight + c.perLateralG * (p.lateralGripG - 1) + c.perBrakeG * (p.brakeDecelG - 1);
  return Math.min(Math.max(Math.round(pi), c.min), c.max);
}

/** Port of `bracket`. */
export function bracket(index: number): string {
  for (let i = CLASS_INDEX.brackets.length - 1; i >= 0; i--) {
    if (index >= CLASS_INDEX.brackets[i][1]) return CLASS_INDEX.brackets[i][0];
  }
  return "?";
}

/** Result of applying mods (the subset of `wp_economy::AppliedMods` the UI shows). */
export interface ModPreview {
  def: VehicleDef;
  perf: PerfMetrics;
  classIndex: number;
  bracket: string;
  /** Durability multipliers per region (1 = stock). */
  durability: Partial<Record<BodyRegion | "Cabin cage", number>>;
  /** Handling-only parameters changed (not in the estimate): param → final multiplier/offset. */
  handling: Record<string, number>;
}

function deepCopy<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

/** Apply `mods` (ids) to `base` and estimate. Unknown ids are ignored. */
export function previewBuild(base: VehicleDef, mods: string[]): ModPreview {
  const def = deepCopy(base);
  let torqueScale = 1;
  let boost = 0;
  let curveShift = 0;
  const handling: Record<string, number> = {};
  const durability: ModPreview["durability"] = {};
  const list: ModDef[] = mods.map((id) => MODS[id]).filter(Boolean);
  const num = (v: number | string) => (typeof v === "number" ? v : 0);
  const combine = (cur: number, op: string, v: number) => (op === "Add" ? cur + v : op === "Mul" ? cur * v : v);
  for (const mod of list) {
    for (const eff of mod.effects) {
      if ("Vehicle" in eff) {
        const { param, op, value } = (eff as { Vehicle: { param: string; op: string; value: number | string } }).Vehicle;
        const v = num(value);
        switch (param) {
          case "engine.torque_scale":
            torqueScale = combine(torqueScale, op, v);
            break;
          case "engine.turbo_boost":
          case "engine.supercharger_boost":
            boost = combine(boost, op, v);
            break;
          case "engine.curve_shift_rpm":
            curveShift = combine(curveShift, op, v);
            break;
          case "engine.redline_rpm":
            def.engine.redline_rpm = combine(def.engine.redline_rpm, op, v);
            def.engine.rev_limit_rpm = combine(def.engine.rev_limit_rpm, op, v);
            break;
          case "drivetrain.final_drive":
            def.drivetrain.final_drive = combine(def.drivetrain.final_drive, op, v);
            break;
          case "drivetrain.shift_time_scale":
            def.drivetrain.shift_time_s *= v;
            break;
          case "drivetrain.clutch_scale":
            def.drivetrain.clutch_max_nm *= v;
            break;
          case "brakes.torque_scale":
            def.brakes.max_torque_front_nm *= v;
            def.brakes.max_torque_rear_nm *= v;
            break;
          case "wheels.tire_id":
            for (const w of def.wheels) w.tire_id = String(value);
            break;
          case "wheels.width_m":
            for (const w of def.wheels) w.width_m = combine(w.width_m, op, v);
            break;
          case "wheels.mass_kg":
            def.mass_kg += 4 * v;
            break;
          case "body.mass_kg":
            def.mass_kg = combine(def.mass_kg, op, v);
            break;
          case "body.drag_scale":
            def.drag_cd_a_m2 *= v;
            break;
          case "body.cg_height_m":
            def.cg_height_m = combine(def.cg_height_m, op, v);
            break;
          case "aero.downforce_front":
            def.aero_downforce_coeffs[0] = combine(def.aero_downforce_coeffs[0], op, v);
            break;
          case "aero.downforce_rear":
            def.aero_downforce_coeffs[1] = combine(def.aero_downforce_coeffs[1], op, v);
            break;
          default:
            handling[param] = combine(handling[param] ?? (op === "Mul" ? 1 : 0), op, v);
        }
      } else if ("Durability" in eff) {
        const d = (eff as Extract<typeof eff, { Durability: unknown }>).Durability as {
          target: { Region: BodyRegion } | { Component: string } | "CabinCage";
          scale: number;
          mass_kg: number;
        };
        def.mass_kg += d.mass_kg;
        const key = d.target === "CabinCage" ? "Cabin cage" : "Region" in d.target ? d.target.Region : null;
        if (key) durability[key] = (durability[key] ?? 1) * (d.target === "CabinCage" ? 1 + d.scale : d.scale);
      }
    }
  }
  if (torqueScale !== 1 || boost !== 0 || curveShift !== 0) {
    def.engine.torque_curve_nm = def.engine.torque_curve_nm.map(([rpm, nm]) => [rpm + curveShift, nm * torqueScale * (1 + boost)]);
  }
  const perf = estimate(def, TIRE_GRIP[def.wheels[0].tire_id] ?? 1);
  const ci = classIndex(perf);
  return { def, perf, classIndex: ci, bracket: bracket(ci), durability, handling };
}
