import { describe, expect, it } from "vitest";
import { CARS } from "../data/catalog";
import { loadFixture } from "./fixtures";
import { applyEvent, combineSeverity, contactSeverity, cooldownAt, emptyPlayback, markersFor, regionAt, type Playback, type ReduceContext } from "./reduce";
import type { Run } from "./runs";

function ctxFor(run: Run, player: number): ReduceContext {
  return {
    run,
    player,
    bodySize: (e) => (CARS[run.entities.find((x) => x.entity === e)?.car ?? ""]?.body_size_m as [number, number, number]) ?? [1.8, 4.5, 1.4],
    nameOf: (e) => `#${e}`,
  };
}

function foldAll(run: Run, player: number, upTo = Infinity): Playback {
  const ctx = ctxFor(run, player);
  return run.events.filter((l) => l.tick <= upTo).reduce((p, l) => applyEvent(p, l, ctx), emptyPlayback());
}

describe("contact → region", () => {
  it("maps chassis-local points to regions", () => {
    expect(regionAt(0, 2.2, 1.8, 4.5)).toBe("FrontBumper");
    expect(regionAt(0.85, 2.0, 1.8, 4.5)).toBe("FrontRightFender");
    expect(regionAt(0, -2.2, 1.8, 4.5)).toBe("RearBumper");
    expect(regionAt(-0.9, 0.4, 1.8, 4.5)).toBe("LeftDoorF");
    expect(regionAt(-0.9, -0.4, 1.8, 4.5)).toBe("LeftDoorR");
    expect(regionAt(0.9, -2.0, 1.8, 4.5)).toBe("RearRightQuarter");
  });

  it("combines severities without exceeding 1", () => {
    expect(combineSeverity(0.5, 0.5)).toBeCloseTo(0.75);
    expect(combineSeverity(1, 0.3)).toBe(1);
    expect(contactSeverity(1e9, 100)).toBe(1);
  });
});

describe("replaying real simcli runs", () => {
  it("frontal: front of the car crushed, components damaged, lights failed", () => {
    const p = foldAll(loadFixture("golden_frontal_56kmh"), 1);
    expect(p.regions.FrontBumper ?? 0).toBeGreaterThan(0.5);
    expect(p.regions.RearBumper ?? 0).toBe(0);
    expect(p.components.Radiator).toBeLessThan(1);
    expect(p.failed).toContain("Lights");
    expect(p.detached.length).toBe(1);
    expect(p.scanner.some((s) => s.text.startsWith("Impact"))).toBe(true);
  });

  it("t-bone: the victim (entity 1) is hit on its left side", () => {
    const p = foldAll(loadFixture("golden_tbone"), 1);
    const left = (p.regions.LeftDoorF ?? 0) + (p.regions.LeftDoorR ?? 0) + (p.regions.FrontLeftFender ?? 0) + (p.regions.RearLeftQuarter ?? 0);
    const right = (p.regions.RightDoorF ?? 0) + (p.regions.RightDoorR ?? 0);
    expect(left).toBeGreaterThan(0.3);
    expect(right).toBeLessThan(left / 5); // a light second contact after the spin
  });

  it("ignores damage events for other vehicles", () => {
    const p = foldAll(loadFixture("golden_frontal_56kmh"), 99);
    expect(p.components).toEqual({});
    expect(p.regions).toEqual({});
  });
});

describe("synthetic pursuit", () => {
  const run = loadFixture("synthetic_pursuit");

  it("raises heat to level 4, then clears on escape", () => {
    expect(foldAll(run, 1, 70 * 240).heat.level).toBe("L4");
    const end = foldAll(run, 1);
    expect(end.heat.level).toBe("None");
    expect(end.heat.state).toBe("clear");
  });

  it("tracks pursuit and cooldown", () => {
    expect(foldAll(run, 1, 12 * 240).heat.state).toBe("pursuit");
    const cd = foldAll(run, 1, 50 * 240);
    expect(cd.heat.state).toBe("cooldown");
    // 20 s cooldown started at 47 s: 3 s in = 15 %.
    expect(cooldownAt(cd, 50 * 240)).toBeCloseTo(0.15, 2);
  });

  it("uses RegionDamaged directly (no inferred damage) and decodes radio + economy payloads", () => {
    const p = foldAll(run, 1);
    expect(p.regions.FrontBumper).toBeGreaterThan(0.85);
    expect(p.regions.LeftDoorF).toBeCloseTo(0.55, 5);
    expect(p.scanner.some((s) => s.channel === "unit" && s.text.includes("Spikes down"))).toBe(true);
    expect(p.economy[0]?.bounty_delta).toBe(2500);
    expect(p.tools.map((t) => t.tool)).toEqual(["SpikeStrip", "Spotlight", "Roadblock"]);
    expect(p.dispatched.map((d) => d.kind)).toContain("Helicopter");
  });

  it("produces timeline markers", () => {
    const kinds = markersFor(run, 1).map((m) => m.kind);
    expect(kinds).toContain("contact");
    expect(kinds).toContain("police");
    expect(kinds).toContain("violation");
  });
});
