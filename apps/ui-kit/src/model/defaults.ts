/**
 * Defaults shared by every provider (mock and live): settings, assist presets, and an empty
 * `UiState` for hosts that only use a few screens. No mock data and no fixtures in here.
 */
import type { AssistProfile } from "../generated/contracts";
import { defaultBindings } from "./bindings";
import { initialReadiness } from "./readiness";
import type { AssistPreset, Settings, UiState } from "./types";

/** Assist presets, matching `wp_contracts::AssistProfile` constructors. */
export const ASSIST_PRESETS: Record<Exclude<AssistPreset, "custom">, AssistProfile> = {
  // Everything off (not a contract preset: the hardcore option).
  off: {
    abs: false,
    tcs: false,
    esc: false,
    steer_speed_sensitivity: 0,
    countersteer_help: 0,
    throttle_filter: 0,
    brake_filter: 0,
    auto_gears: false,
    auto_clutch: false,
  },
  // AssistProfile::wheel_default()
  sim: {
    abs: true,
    tcs: false,
    esc: false,
    steer_speed_sensitivity: 0,
    countersteer_help: 0,
    throttle_filter: 0,
    brake_filter: 0,
    auto_gears: false,
    auto_clutch: true,
  },
  // AssistProfile::gamepad_default()
  standard: {
    abs: true,
    tcs: true,
    esc: false,
    steer_speed_sensitivity: 0.7,
    countersteer_help: 0.3,
    throttle_filter: 0.2,
    brake_filter: 0.2,
    auto_gears: true,
    auto_clutch: true,
  },
  // AssistProfile::keyboard_default()
  full: {
    abs: true,
    tcs: true,
    esc: true,
    steer_speed_sensitivity: 1,
    countersteer_help: 0.6,
    throttle_filter: 0.7,
    brake_filter: 0.6,
    auto_gears: true,
    auto_clutch: true,
  },
};

/** Which preset (if any) a profile equals. */
export function presetOf(a: AssistProfile): AssistPreset {
  for (const [name, p] of Object.entries(ASSIST_PRESETS)) {
    if ((Object.keys(p) as (keyof AssistProfile)[]).every((k) => p[k] === a[k])) return name as AssistPreset;
  }
  return "custom";
}

/** Default settings. */
export function defaultSettings(): Settings {
  return {
    bindings: defaultBindings(),
    assistPreset: "full",
    assists: { ...ASSIST_PRESETS.full },
    damageMode: "Arcade",
    units: "metric",
    input: { steerDeadzone: 0.04, steerLinearity: 1.4, wheelRotationDeg: 900, combinedPedals: false },
    accessibility: { palette: "default", uiScale: 1, reduceMotion: false, largeScannerText: false },
    feedback: { rumble: true, rumbleStrength: 0.7, ffbStrength: 0.8 },
    audio: { master: 0.9, engine: 0.8, scanner: 0.7, music: 0.5 },
  };
}

/** A complete but empty `UiState` (live hosts fill only the slices their screens read). */
export function emptyUiState(screen: UiState["screen"] = "hud"): UiState {
  const settings = defaultSettings();
  return {
    screen,
    previousScreen: null,
    role: "Racer",
    hud: {
      entity: 0,
      speedMps: 0,
      engineRpm: 0,
      redlineRpm: 7000,
      gear: 0,
      health: 1,
      regions: {},
      detached: [],
      components: {},
      failed: [],
      wheelsDetached: [],
      totalLoss: false,
      heat: { level: "None", heat: 0, state: "clear", cooldown: 0, unitsInPursuit: 0, bounty: 0 },
      pos: [0, 0],
      headingRad: 0,
      blips: [],
      race: null,
      assists: settings.assists,
      damageMode: settings.damageMode,
      timeOfDayS: 0,
    },
    scanner: [],
    settings,
    account: { cash: 0, rep: 0, bounty: 0, day: 0, copXp: 0, copLevel: 0, inDebt: false },
    garage: { cars: [], activeCar: null, shops: [], selectedShop: "" },
    market: { listings: [] },
    lobby: {
      eventName: "",
      kind: "Sprint",
      location: "",
      laps: 1,
      bracket: "",
      maxClassIndex: 999,
      buyIn: 0,
      purse: 0,
      damageMode: settings.damageMode,
      policeEnabled: true,
      entrants: [],
      countdownS: null,
      maxRacers: 8,
      roleSlots: { Racer: 8, PoliceCaptain: 1, PoliceChase: 4, Spectator: 8 },
    },
    results: { eventName: "", rows: [], payout: [], damageEstimate: 0, cashBefore: 0, cashAfter: 0 },
    impound: { entries: [], finesOwed: 0, lastCapture: null },
    crew: { crews: [], territories: [], pois: [] },
    police: { rank: "", level: 0, xp: 0, xpToNext: 0, unlocks: [], stats: { busts: 0, escapes: 0, pursuits: 0, damageCaused: 0 }, orders: [], assignment: null },
    captain: { source: "wp_police", units: [], suspects: [], incidents: [], tools: [], heat: [], heatByDistrict: [], readiness: initialReadiness(), priorityTarget: null },
    map: { id: "", boundsMin: [0, 0], boundsMax: [1, 1], roads: [], zones: [], pois: [] },
    replay: { source: "live", playing: true, speed: 1, tick: 0, totalTicks: 0, ticksPerSecond: 240, markers: [], camera: "chase" },
    bill: null,
    notice: null,
  };
}
