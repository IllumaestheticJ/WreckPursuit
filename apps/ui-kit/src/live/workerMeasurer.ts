/**
 * A [`BuildMeasurer`](../data/measure.ts) backed by `statsWorker.ts` (physics-measured stat bars
 * for modded builds, off the main thread). Hosts: `setBuildMeasurer(createWorkerMeasurer())`.
 */
import type { CarStats } from "../data/catalog";
import type { BuildMeasurer } from "../data/measure";

/** Start the stats worker and return a measurer that talks to it. */
export function createWorkerMeasurer(): BuildMeasurer {
  const worker = new Worker(new URL("./statsWorker.ts", import.meta.url), { type: "module" });
  const waiting = new Map<number, { ok: (s: CarStats) => void; fail: (e: Error) => void }>();
  let next = 1;
  worker.onmessage = (e: MessageEvent<{ id: number; stats?: CarStats; error?: string }>) => {
    const w = waiting.get(e.data.id);
    if (!w) return;
    waiting.delete(e.data.id);
    if (e.data.stats) w.ok(e.data.stats);
    else w.fail(new Error(e.data.error ?? "measurement failed"));
  };
  return (defId, mods, mode) =>
    new Promise<CarStats>((ok, fail) => {
      const id = next++;
      waiting.set(id, { ok, fail });
      worker.postMessage({ id, defId, mods, mode });
    });
}
