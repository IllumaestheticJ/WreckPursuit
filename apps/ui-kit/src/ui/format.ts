/** Display formatting. The model is always SI; conversion happens only here. */

/** Speed in the player's units. */
export function speed(mps: number, units: "metric" | "imperial"): { value: string; unit: string } {
  return units === "metric" ? { value: Math.round(mps * 3.6).toString(), unit: "km/h" } : { value: Math.round(mps * 2.23694).toString(), unit: "mph" };
}

/** `m:ss.mmm` (or `--:--.---` for null). */
export function raceTime(s: number | null): string {
  if (s === null || !Number.isFinite(s)) return "--:--.---";
  const m = Math.floor(s / 60);
  const r = s - m * 60;
  return `${m}:${r.toFixed(3).padStart(6, "0")}`;
}

/** `+1.234` gap. */
export function gap(s: number | null): string {
  return s === null ? "—" : `${s >= 0 ? "+" : "−"}${Math.abs(s).toFixed(2)}`;
}

/** Percentage. */
export function pct(v: number, digits = 0): string {
  return `${(v * 100).toFixed(digits)}%`;
}

/** `HH:MM` from seconds since midnight. */
export function clock(s: number): string {
  const h = Math.floor(s / 3600) % 24;
  const m = Math.floor((s % 3600) / 60);
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

/** `m:ss` from seconds. */
export function mmss(s: number): string {
  const m = Math.floor(s / 60);
  return `${m}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
}

/** Split `CamelCase` into words. */
export function words(s: string): string {
  return s.replace(/([a-z0-9])([A-Z])/g, "$1 $2");
}

/** Gear label: R, N, 1.. */
export function gearLabel(g: number): string {
  return g < 0 ? "R" : g === 0 ? "N" : String(g);
}
