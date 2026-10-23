// Police light flash patterns and the light rig of one car (from claude/detailed-car-models
// render/police_units.ts; spec docs/design/vehicles/research_police.md §2).
//
// - Flash patterns as data (`PATTERNS`), shared by sim police cars (vehicle_view.ts) and the
//   instanced fleet (render/lod.ts → render/fleet.ts lens instances). Lens meshes may strobe
//   fast; the world-lighting envelope (`light`, the scene's one siren light) is slower (≤ 3 Hz
//   with ~40 ms ramps, WCAG 2.3.1). `?flash=slow` or the "reduced flashing" setting (F6,
//   settings.ts) swaps every pattern for `P-SLOW`.
// - Wig-wag headlamps (`wigwag`, P-HEADLAMP-WW): left / right head lamps alternate at 2 Hz
//   (1 Hz with reduced flashing) while the siren is on.
// - `LightRig` drives one car's lens materials (left / right sides independently, clear
//   takedowns, amber advisors). It owns no THREE light: round 12 keeps one shared siren point
//   light in the scene (scene.ts `setSiren`), fed from `LightRig.light`.
import * as THREE from "three";
import type { LightbarSide } from "./panels";
import { hash32 } from "./livery";
import { layers } from "../settings";

/** Channels: L / R = left / right lens halves (any colour), W = clear takedowns, A = amber. */
export type Channel = "L" | "R" | "W" | "A";
/** On-windows (seconds inside the period) per channel. */
export interface FlashPattern {
  period: number;
  /** Lens on-windows (fast strobing allowed). */
  ch: Partial<Record<Channel, [number, number][]>>;
  /** Scene-light envelope windows for L / R (≤ 3 Hz); defaults to `ch`. */
  env?: Partial<Record<"L" | "R", [number, number][]>>;
  /** Rotating beacons: lens / light brightness from a rotating lobe, L and R 180° apart. */
  rotate?: boolean;
}

/** research_police.md §2. */
export const PATTERNS: Record<string, FlashPattern> = {
  "P-GUMBALL": { period: 0.75, rotate: true, ch: {} },
  "P-ROTATOR": { period: 0.6, rotate: true, ch: { W: [[0, 0.15]] } },
  "P-STROBE": {
    period: 0.8,
    ch: { L: [[0, 0.04], [0.1, 0.14]], R: [[0.4, 0.44], [0.5, 0.54]], A: [[0.4, 0.44], [0.5, 0.54]] },
    env: { L: [[0, 0.18]], R: [[0.4, 0.58]] },
  },
  "P-LED-QUAD": {
    period: 0.5,
    ch: {
      L: [[0, 0.04], [0.06, 0.1], [0.12, 0.16], [0.18, 0.22]],
      R: [[0.25, 0.29], [0.31, 0.35], [0.37, 0.41], [0.43, 0.47]],
      W: [[0.22, 0.25], [0.47, 0.5]],
      A: [[0.25, 0.29], [0.31, 0.35], [0.37, 0.41], [0.43, 0.47]],
    },
    env: { L: [[0, 0.22]], R: [[0.25, 0.47]] },
  },
  "P-LED-WIGWAG": { period: 0.7, ch: { L: [[0, 0.3]], R: [[0.35, 0.65]], A: [[0, 0.3]] } },
  "P-LED-SIM": {
    period: 1.0,
    ch: { L: [[0, 0.08], [0.15, 0.23]], R: [[0, 0.08], [0.15, 0.23]], W: [[0.5, 0.58], [0.65, 0.73]], A: [[0, 0.5]] },
    env: { L: [[0, 0.23]], R: [[0, 0.23]] },
  },
  "P-SLOW": { period: 1.0, ch: { L: [[0, 0.45]], R: [[0.5, 0.95]], A: [[0, 0.45]] } },
};

/** Moving pattern per model (era): rotators for the 80s car, LED quad flash otherwise. */
export function patternFor(carId: string, parked = false): string {
  if (reducedFlashing()) return "P-SLOW";
  if (carId === "sentinel_warden") return "P-ROTATOR";
  return parked ? "P-LED-SIM" : "P-LED-QUAD";
}

let reducedUrl: boolean | null = null;
/** Reduced flashing (P-SLOW everywhere): `?flash=slow` in the URL or the settings toggle (F6). */
export function reducedFlashing(): boolean {
  if (reducedUrl === null) {
    try {
      reducedUrl = new URLSearchParams(globalThis.location?.search ?? "").get("flash") === "slow";
    } catch {
      reducedUrl = false;
    }
  }
  return reducedUrl || layers.on("reducedFlashing");
}

/**
 * Wig-wag head-lamp levels (P-HEADLAMP-WW) at time `t` (s) with phase offset `phase` (0..1):
 * left / right alternate, 2 Hz per lamp (1 Hz with reduced flashing), ~40 ms ramps.
 */
export function wigwag(t: number, phase: number): { left: number; right: number } {
  const period = reducedFlashing() ? 1.0 : 0.5;
  const ph = (((t + phase * period) % period) + period) % period;
  const half = period / 2;
  const wins = (a: number): [number, number][] => [[a + 0.02 * period, a + half - 0.02 * period]];
  return { left: windowLevel(wins(0), ph, true), right: windowLevel(wins(half), ph, true) };
}

/** Stable phase offset (s) for a unit / entity id, de-syncs a convoy. */
export function phaseFor(id: number): number {
  return (hash32(id) % 1000) / 1000;
}

const RAMP_S = 0.04;

/** 0..1: is `t` (within the period) inside a window; `soft` adds the ~40 ms ramps. */
function windowLevel(wins: [number, number][] | undefined, t: number, soft: boolean): number {
  if (!wins) return 0;
  let v = 0;
  for (let k = 0; k < wins.length; k++) {
    const a = wins[k][0];
    const b = wins[k][1];
    if (!soft) {
      if (t >= a && t < b) return 1;
      continue;
    }
    const e = Math.min(t - a, b - t) / RAMP_S + 0.5;
    v = Math.max(v, Math.min(1, Math.max(0, e)));
  }
  return v;
}

/**
 * Channel levels of pattern `p` at time `t` (s) into `out` (no allocation; the fleet runs it per
 * police car per frame): lens L, R, W, A (fast) then scene light L, R (slow envelope).
 */
export function patternLevels(p: FlashPattern, t: number, out: Float32Array | number[]): void {
  const ph = ((t % p.period) + p.period) % p.period;
  if (p.rotate) {
    const th = (2 * Math.PI * ph) / p.period;
    // Mirrors in the dome: two lobes per revolution, as seen from one direction.
    const a = Math.pow(Math.max(0, Math.cos(th)), 4);
    const b = Math.pow(Math.max(0, Math.cos(th + Math.PI)), 4);
    const l = Math.max(a, 0.6 * b);
    const r = Math.max(b, 0.6 * a);
    out[0] = 0.3 + 0.7 * l;
    out[1] = 0.3 + 0.7 * r;
    out[2] = windowLevel(p.ch.W, ph, false);
    out[3] = 0;
    out[4] = 0.15 + 0.85 * l;
    out[5] = 0.15 + 0.85 * r;
    return;
  }
  const env = p.env ?? p.ch;
  out[0] = windowLevel(p.ch.L, ph, false);
  out[1] = windowLevel(p.ch.R, ph, false);
  out[2] = windowLevel(p.ch.W, ph, false);
  out[3] = windowLevel(p.ch.A, ph, false);
  out[4] = windowLevel(env.L, ph, true);
  out[5] = windowLevel(env.R, ph, true);
}

const evalScratch = new Float32Array(6);
/** Channel levels at time `t` (s): lens (fast) and light (slow envelope). */
export function evalPattern(p: FlashPattern, t: number): { lens: Record<Channel, number>; light: { L: number; R: number } } {
  const v = evalScratch;
  patternLevels(p, t, v);
  return { lens: { L: v[0], R: v[1], W: v[2], A: v[3] }, light: { L: v[4], R: v[5] } };
}

/**
 * Lens levels (L, R, W, A, then light L, R) of a police unit at time `t` into `out`: its model's
 * pattern, de-synced by its stable id; all 0 when not `on`.
 */
export function unitLevels(carId: string, id: number, t: number, on: boolean, out: Float32Array): Float32Array {
  if (!on) {
    out.fill(0);
    return out;
  }
  const p = PATTERNS[patternFor(carId)] ?? PATTERNS["P-LED-QUAD"];
  patternLevels(p, t + phaseFor(id) * p.period, out);
  return out;
}

const ON = {
  red: new THREE.Color(0xff2040),
  blue: new THREE.Color(0x2060ff),
  clear: new THREE.Color(0xfff6e0),
  amber: new THREE.Color(0xffa010),
};
/** Lens colour when lit (render/fleet.ts lens instances use the same). */
export const LENS_ON = ON;

/** Lens materials of one police car, flashed from a pattern. */
export class LightRig {
  private readonly amberIdle: THREE.Color[];
  /** Scene-light envelope of the last update, per side (0..1; 0 while off). */
  readonly light = { L: 0, R: 0 };
  /** Emissive intensity of lit lenses (the art style raises it so they bloom). */
  glow = 2.2;
  constructor(
    readonly left: LightbarSide,
    readonly right: LightbarSide,
    readonly amber: THREE.MeshStandardMaterial[],
    public pattern: string,
    readonly phase: number,
  ) {
    this.amberIdle = amber.map((m) => m.emissive.clone());
  }

  /** Set every lens for time `t` (s); `on` = siren / pursuing. */
  update(t: number, on: boolean): void {
    const p = PATTERNS[this.pattern] ?? PATTERNS["P-LED-QUAD"];
    const lv = on ? evalPattern(p, t + this.phase * p.period) : null;
    const set = (s: LightbarSide, k: number) => {
      s.red.emissive.copy(ON.red).multiplyScalar(k);
      s.blue.emissive.copy(ON.blue).multiplyScalar(k);
      s.red.emissiveIntensity = s.blue.emissiveIntensity = s.clear.emissiveIntensity = this.glow;
    };
    set(this.left, lv ? lv.lens.L : 0);
    set(this.right, lv ? lv.lens.R : 0);
    const w = lv ? lv.lens.W : 0;
    this.left.clear.emissive.copy(ON.clear).multiplyScalar(w);
    this.right.clear.emissive.copy(ON.clear).multiplyScalar(w);
    this.amber.forEach((m, i) => {
      if (lv && lv.lens.A > 0) m.emissive.copy(ON.amber).multiplyScalar(lv.lens.A);
      else m.emissive.copy(this.amberIdle[i]);
    });
    this.light.L = lv ? lv.light.L : 0;
    this.light.R = lv ? lv.light.R : 0;
  }
}
