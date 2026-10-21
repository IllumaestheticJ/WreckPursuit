import { afterEach, describe, expect, it } from "vitest";
import { CAR_STATS } from "./catalog";
import { buildKey, measuredEntry, requestMeasurement, setBuildMeasurer } from "./measure";
import { buildStats } from "./stats";

afterEach(() => setBuildMeasurer(null));

describe("build measurer", () => {
  it("measures once per build, any mod order, and the result replaces the estimate", async () => {
    const calls: string[] = [];
    setBuildMeasurer(async (defId, mods, mode) => {
      calls.push(buildKey(defId, mods, mode));
      return { ...CAR_STATS.kestrel_ts, top_speed_kmh: 299 };
    });
    const mods = ["engine_intake_1", "tires_semi_slick"];
    const first = requestMeasurement("kestrel_ts", mods, "Arcade");
    expect(measuredEntry("kestrel_ts", mods, "Arcade")?.state).toBe("pending");
    await Promise.all([first, requestMeasurement("kestrel_ts", [...mods].reverse(), "Arcade")]);
    expect(calls).toHaveLength(1);
    const e = measuredEntry("kestrel_ts", mods, "Arcade");
    expect(e?.state).toBe("done");
    const b = buildStats("kestrel_ts", mods, e?.state === "done" ? e.stats : null)!;
    expect(b.source).toBe("measured");
    expect(b.stats.top_speed_kmh).toBe(299);
    // Another mode is another build.
    await requestMeasurement("kestrel_ts", mods, "Simulation");
    expect(calls).toHaveLength(2);
  });

  it("keeps the estimate when measuring fails, and skips stock builds", async () => {
    let n = 0;
    setBuildMeasurer(async () => {
      n++;
      throw new Error("no wasm");
    });
    await requestMeasurement("kestrel_ts", ["tires_semi_slick"], "Arcade");
    expect(measuredEntry("kestrel_ts", ["tires_semi_slick"], "Arcade")?.state).toBe("failed");
    await requestMeasurement("kestrel_ts", [], "Arcade");
    expect(n).toBe(1);
  });
});
