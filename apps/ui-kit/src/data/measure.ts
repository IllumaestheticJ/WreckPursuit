/**
 * Physics measurement of a modded build's stat bars, when the host can run the sim.
 *
 * The kit itself has no wasm: a host registers a measurer with [`setBuildMeasurer`] (the demo
 * uses [`createWorkerMeasurer`] in `src/live/workerMeasurer.ts`, a Web Worker running
 * `wp_wasm::car_stats_build_json`). Screens call [`useBuildStats`]: it returns the estimate from
 * `data/stats.ts` at once, asks the measurer in the background, and re-renders with the measured
 * build when it arrives. Results are cached per (car, sorted mods, mode) for the page's lifetime;
 * a failed measurement keeps the estimate.
 */
import { useEffect, useState } from "preact/hooks";
import type { DamageMode } from "../generated/contracts";
import type { CarStats } from "./catalog";
import { buildStats, type BuildStats } from "./stats";

/** Measures `defId` with `mods` installed in `mode` (rejects if it cannot). */
export type BuildMeasurer = (defId: string, mods: string[], mode: DamageMode) => Promise<CarStats>;

type Entry = { state: "pending"; done: Promise<void> } | { state: "done"; stats: CarStats } | { state: "failed"; error: string };

let measurer: BuildMeasurer | null = null;
const cache = new Map<string, Entry>();

/** Register (or with `null` remove) the host's measurer; clears cached results. */
export function setBuildMeasurer(m: BuildMeasurer | null): void {
  measurer = m;
  cache.clear();
}

/** Cache key: car, sorted mods, mode. */
export function buildKey(defId: string, mods: string[], mode: DamageMode): string {
  return `${defId}|${[...mods].sort().join(",")}|${mode}`;
}

/** The cached measurement state of a build (`undefined` = never asked). */
export function measuredEntry(defId: string, mods: string[], mode: DamageMode): Entry | undefined {
  return cache.get(buildKey(defId, mods, mode));
}

/**
 * Ask the measurer for a build unless it is cached or in flight. Resolves when the entry is
 * settled (done or failed); resolves at once with no measurer, no mods, or a cached entry.
 */
export function requestMeasurement(defId: string, mods: string[], mode: DamageMode): Promise<void> {
  const key = buildKey(defId, mods, mode);
  const hit = cache.get(key);
  if (hit?.state === "pending") return hit.done;
  if (hit || !measurer || mods.length === 0) return Promise.resolve();
  const done = measurer(defId, mods, mode).then(
    (stats) => void cache.set(key, { state: "done", stats }),
    (e: unknown) => void cache.set(key, { state: "failed", error: String(e) }),
  );
  cache.set(key, { state: "pending", done });
  return done;
}

/** [`BuildStats`] plus whether a measurement is still running. */
export type LiveBuildStats = BuildStats & { pending: boolean };

/**
 * Stats of a build for a screen: `provided` (measured by the model's provider) if given, else
 * the measurer's result once it arrives, else the estimate. `null` if the car has no stats.
 */
export function useBuildStats(defId: string, mods: string[], mode: DamageMode, provided?: CarStats | null): LiveBuildStats | null {
  const key = buildKey(defId, mods, mode);
  const [, rerender] = useState(0);
  useEffect(() => {
    if (provided) return;
    let live = true;
    void requestMeasurement(defId, mods, mode).then(() => live && rerender((n) => n + 1));
    return () => {
      live = false;
    };
    // `key` covers defId, mods (sorted) and mode.
  }, [key, provided]);
  const e = cache.get(key);
  const measured = provided ?? (e?.state === "done" ? e.stats : null);
  const b = buildStats(defId, mods, measured);
  return b && { ...b, pending: !provided && e?.state === "pending" };
}

/** "measured" / "measuring…" / "estimated" for a build's stats. */
export function statSourceLabel(b: LiveBuildStats): string {
  return b.source === "measured" ? "measured" : b.pending ? "measuring…" : "estimated";
}
