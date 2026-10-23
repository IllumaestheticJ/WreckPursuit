/**
 * Player settings of the game menu (owner request 2026-10-20, playtest row 68): graphics,
 * audio volumes, assists, damage mode, units, camera and the developer-panels switch.
 *
 * Pure data + validation; persistence goes through `loadGameSettings` / `saveGameSettings`, which
 * never throw (private mode, blocked storage, corrupt JSON all fall back to defaults). The host
 * applies the values (it owns the sim, the renderer and the audio buses).
 */

import { defaultControls, normalizeControls, type ControlDevice, type DeviceControls } from "./controls";

/** Graphics quality levels (the testbed's `render/quality.ts` modes). */
export type GfxLevel = "auto" | "high" | "low";

/** City draw distance choices, m (`0` = the quality level's own distance). */
export const DRAW_DISTANCES = [0, 300, 600, 900, 1500, 3000] as const;

/** Speed units. */
export type SpeedUnits = "kmh" | "mph";

/** Camera angles the host offers (the testbed's `CAMERA_MODES`), in its own ids. */
export type CameraId = string;

/** Volumes 0..=1 (1 = the mix as designed). */
export interface Volumes {
  master: number;
  engine: number;
  /** Crashes, tyres, world sounds (the `World` bus). */
  effects: number;
  /** Police radio and sirens (the `Police` bus). */
  police: number;
  /** City ambience bed. */
  ambience: number;
}

/** Driving assists the menu exposes (independent of the damage mode). */
export interface AssistSettings {
  abs: boolean;
  tcs: boolean;
  esc: boolean;
  autoGears: boolean;
  /** Speed-sensitive steering (the input-device preset's value, or 0 when off). */
  steerHelp: boolean;
  /** Countersteer help strength 0..=1. Only applied once the host can set it (`countersteer` capability). */
  countersteer: number;
}

/** Every setting the menu edits. */
export interface GameSettings {
  version: 1;
  gfx: GfxLevel;
  /** m; 0 = from the quality level. */
  drawDistanceM: number;
  volumes: Volumes;
  /** Speak police radio lines (browser speech; owner playtest row 79). Off = squelch, static
   * and the radio strip's text only. The volume is the Police radio slider. */
  radioVoice: boolean;
  assists: AssistSettings;
  damageMode: "Arcade" | "Simulation" | "Extreme";
  units: SpeedUnits;
  /** Default camera angle (host id); "" = the host's default. */
  camera: CameraId;
  /** Show the testbed's developer panels (scenario menu, telemetry, feeds). */
  developer: boolean;
  /** Input shaping per device (dead zones, saturation, sensitivity; see `controls.ts`). */
  controls: Record<ControlDevice, DeviceControls>;
}

/** localStorage key. */
export const SETTINGS_KEY = "wp.gameSettings.v1";

/** Defaults. `developer` is the host's choice (on in the testbed preview, off in the demo). */
export function defaultGameSettings(developer = true): GameSettings {
  return {
    version: 1,
    gfx: "auto",
    drawDistanceM: 0,
    volumes: { master: 0.8, engine: 1, effects: 1, police: 1, ambience: 0.5 },
    radioVoice: true,
    assists: { abs: true, tcs: false, esc: false, autoGears: false, steerHelp: true, countersteer: 0.5 },
    damageMode: "Arcade",
    units: "kmh",
    camera: "",
    developer,
    controls: defaultControls(),
  };
}

const clamp01 = (v: unknown, d: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : d);
const bool = (v: unknown, d: boolean) => (typeof v === "boolean" ? v : d);
const oneOf = <T extends string>(v: unknown, opts: readonly T[], d: T): T => (opts.includes(v as T) ? (v as T) : d);

/** Merge untrusted data (stored JSON) onto `base`, field by field; bad fields keep the base. */
export function normalizeSettings(raw: unknown, base: GameSettings): GameSettings {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const v = (r.volumes && typeof r.volumes === "object" ? r.volumes : {}) as Record<string, unknown>;
  const a = (r.assists && typeof r.assists === "object" ? r.assists : {}) as Record<string, unknown>;
  const dd = typeof r.drawDistanceM === "number" && (DRAW_DISTANCES as readonly number[]).includes(r.drawDistanceM) ? r.drawDistanceM : base.drawDistanceM;
  return {
    version: 1,
    gfx: oneOf(r.gfx, ["auto", "high", "low"] as const, base.gfx),
    drawDistanceM: dd,
    volumes: {
      master: clamp01(v.master, base.volumes.master),
      engine: clamp01(v.engine, base.volumes.engine),
      effects: clamp01(v.effects, base.volumes.effects),
      police: clamp01(v.police, base.volumes.police),
      ambience: clamp01(v.ambience, base.volumes.ambience),
    },
    radioVoice: bool(r.radioVoice, base.radioVoice),
    assists: {
      abs: bool(a.abs, base.assists.abs),
      tcs: bool(a.tcs, base.assists.tcs),
      esc: bool(a.esc, base.assists.esc),
      autoGears: bool(a.autoGears, base.assists.autoGears),
      steerHelp: bool(a.steerHelp, base.assists.steerHelp),
      countersteer: clamp01(a.countersteer, base.assists.countersteer),
    },
    damageMode: oneOf(r.damageMode, ["Arcade", "Simulation", "Extreme"] as const, base.damageMode),
    units: oneOf(r.units, ["kmh", "mph"] as const, base.units),
    camera: typeof r.camera === "string" && r.camera.length < 32 ? r.camera : base.camera,
    developer: bool(r.developer, base.developer),
    controls: normalizeControls(r.controls, base.controls),
  };
}

/** Minimal storage interface (window.localStorage in the browser, a map in tests). */
export interface SettingsStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function browserStorage(): SettingsStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** Was anything stored yet? (Hosts seed first-run values, e.g. assists, from their own state.) */
export function hasStoredSettings(storage: SettingsStorage | null = browserStorage()): boolean {
  try {
    return storage?.getItem(SETTINGS_KEY) != null;
  } catch {
    return false;
  }
}

/** Stored settings over `base` (defaults when nothing usable is stored). Never throws. */
export function loadGameSettings(base: GameSettings, storage: SettingsStorage | null = browserStorage()): GameSettings {
  try {
    const text = storage?.getItem(SETTINGS_KEY);
    return text ? normalizeSettings(JSON.parse(text), base) : base;
  } catch {
    return base;
  }
}

/** Store settings. Returns false when storage is unavailable (the settings still apply this page). */
export function saveGameSettings(s: GameSettings, storage: SettingsStorage | null = browserStorage()): boolean {
  try {
    if (!storage) return false;
    storage.setItem(SETTINGS_KEY, JSON.stringify(s));
    return true;
  } catch {
    return false;
  }
}

/** km/h or mph from m/s. */
export function speedIn(units: SpeedUnits, mps: number): number {
  return units === "mph" ? mps * 2.236936 : mps * 3.6;
}

/** "km/h" / "mph". */
export function unitLabel(units: SpeedUnits): string {
  return units === "mph" ? "mph" : "km/h";
}
