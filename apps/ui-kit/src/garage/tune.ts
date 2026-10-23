// The free per-car tune (parity gap G10): four 5-step sliders, 3 = stock. The labels and the
// one-line explanations shown on the Tune page and the testbed's developer panel. What each
// step does to the car is `wp_vehicle::tune` with the tables in `data/tune/steps.toml`.

import type { CarTune } from "./types";

/** Steps on every slider. */
export const TUNE_STEPS = 5;
/** Stock position of every slider. */
export const TUNE_STOCK = 3;

/** Every slider at stock. */
export const STOCK_TUNE: Readonly<CarTune> = { suspension: 3, gearing: 3, diff: 3, brake_balance: 3 };

/** One slider of the tune page. */
export interface TuneSlider {
  key: keyof CarTune;
  label: string;
  /** Label at step 1 and at step 5. */
  low: string;
  high: string;
  /** Short name of each step, step 1 first. */
  steps: [string, string, string, string, string];
  /** What moving it does (shown under the slider). */
  hint: string;
}

export const TUNE_SLIDERS: readonly TuneSlider[] = [
  {
    key: "suspension",
    label: "Suspension",
    low: "Soft",
    high: "Stiff",
    steps: ["Soft", "Softer", "Stock", "Firmer", "Stiff"],
    hint: "Springs and anti-roll bars (dampers follow). Stiff: sharper turn-in, less roll, less grip over bumps.",
  },
  {
    key: "gearing",
    label: "Gear ratio",
    low: "Short",
    high: "Long",
    steps: ["Short", "Shorter", "Stock", "Longer", "Long"],
    hint: "Final drive. Short: quicker acceleration, lower top speed. Long: the opposite.",
  },
  {
    key: "diff",
    label: "Differential",
    low: "Open",
    high: "Locked",
    steps: ["Open", "Light LSD", "Stock", "Tight LSD", "Locked"],
    hint: "Driven axle. Open turns in easily but spins the inside wheel; locked puts the power down and slides the rear.",
  },
  {
    key: "brake_balance",
    label: "Brake balance",
    low: "Rear",
    high: "Front",
    steps: ["Rear", "Rearward", "Stock", "Forward", "Front"],
    hint: "Rear: the car rotates under braking (and can spin). Front: stable, but locks the fronts sooner.",
  },
];

/** Clamp a step to 1..=5 (whole steps). */
export function clampStep(v: number): number {
  return Math.min(TUNE_STEPS, Math.max(1, Math.round(Number.isFinite(v) ? v : TUNE_STOCK)));
}

/** A tune with every slider clamped; missing sliders read as stock. */
export function normalizeTune(raw: Partial<CarTune> | null | undefined): CarTune {
  const r = raw ?? {};
  return {
    suspension: clampStep(r.suspension ?? TUNE_STOCK),
    gearing: clampStep(r.gearing ?? TUNE_STOCK),
    diff: clampStep(r.diff ?? TUNE_STOCK),
    brake_balance: clampStep(r.brake_balance ?? TUNE_STOCK),
  };
}

/** True when every slider is at stock. */
export function isStockTune(t: CarTune): boolean {
  return TUNE_SLIDERS.every((s) => t[s.key] === TUNE_STOCK);
}

/** One-line summary, e.g. "Stiff · Long · Tight LSD · Rearward" ("Stock" when untouched). */
export function tuneSummary(t: CarTune): string {
  if (isStockTune(t)) return "Stock";
  return TUNE_SLIDERS.filter((s) => t[s.key] !== TUNE_STOCK)
    .map((s) => (s.key === "diff" ? s.steps[t[s.key] - 1] : `${s.label} ${s.steps[t[s.key] - 1].toLowerCase()}`))
    .join(" · ");
}

// ----- per-viewer tunes outside the career (quick runs, the developer panel) ----------------

/** localStorage key of the per-car tunes used outside the career. */
export const QUICK_TUNE_KEY = "wp.tune.v1";

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** All stored quick-run tunes by car id (empty when storage is unavailable or unreadable). */
export function loadQuickTunes(): Record<string, CarTune> {
  try {
    const raw = JSON.parse(storage()?.getItem(QUICK_TUNE_KEY) ?? "{}") as Record<string, Partial<CarTune>>;
    const out: Record<string, CarTune> = {};
    for (const [id, t] of Object.entries(raw ?? {})) out[id] = normalizeTune(t);
    return out;
  } catch {
    return {};
  }
}

/** The last tune used for `carId` outside the career (stock if none). */
export function quickTune(carId: string): CarTune {
  return loadQuickTunes()[carId] ?? { ...STOCK_TUNE };
}

/** Remember `tune` for `carId` (a stock tune is forgotten). */
export function saveQuickTune(carId: string, tune: CarTune): void {
  const all = loadQuickTunes();
  if (isStockTune(tune)) delete all[carId];
  else all[carId] = normalizeTune(tune);
  try {
    storage()?.setItem(QUICK_TUNE_KEY, JSON.stringify(all));
  } catch {
    /* storage unavailable: the tune lasts for this page only */
  }
}
