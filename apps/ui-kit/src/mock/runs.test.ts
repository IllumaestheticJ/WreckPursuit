import { describe, expect, it } from "vitest";
import { FIXTURES, loadFixture } from "./fixtures";
import { headingAt, parseEventsJsonl, parseMetricsCsv, rowAt } from "./runs";

describe("simcli run parsing", () => {
  it("bundles the real simcli runs and the synthetic script", () => {
    const ids = FIXTURES.map((f) => f.id);
    expect(ids).toContain("golden_frontal_56kmh");
    expect(ids).toContain("golden_tbone");
    expect(ids[0]).toBe("synthetic_pursuit");
    expect(FIXTURES.find((f) => f.id === "synthetic_pursuit")?.synthetic).toBe(true);
    expect(FIXTURES.find((f) => f.id === "golden_tbone")?.synthetic).toBe(false);
  });

  it("parses the frontal run: entities, events, metrics", () => {
    const run = loadFixture("golden_frontal_56kmh");
    expect(run.entities.map((e) => [e.id, e.entity])).toEqual([["car", 1], ["barrier", 2]]);
    expect(run.events.length).toBeGreaterThan(5);
    expect(Object.keys(run.events[0].event)[0]).toBe("Contact");
    expect(run.metrics.car.length).toBeGreaterThan(100);
    expect(run.totalTicks).toBe(1920);
  });

  it("reports the line number of a bad events line", () => {
    expect(() => parseEventsJsonl('{"tick":1,"t_s":0,"event":{"TotalLoss":{"vehicle":1}}}\nnot json')).toThrow(/events.jsonl:2/);
  });

  it("rejects an unexpected metrics header", () => {
    expect(() => parseMetricsCsv("a,b\n1,2")).toThrow(/header/);
  });

  it("finds rows by tick and derives heading from motion", () => {
    const rows = loadFixture("golden_frontal_56kmh").metrics.car;
    const i = rowAt(rows, 600);
    expect(rows[i].tick).toBeLessThanOrEqual(600);
    expect(rows[i + 1].tick).toBeGreaterThan(600);
    // The frontal car drives along +Y: heading ~0.
    expect(Math.abs(headingAt(rows, i))).toBeLessThan(0.05);
    expect(rowAt(rows, -5)).toBe(-1);
  });
});
