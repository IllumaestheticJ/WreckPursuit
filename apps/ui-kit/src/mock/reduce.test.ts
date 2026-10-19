import { describe, expect, it } from "vitest";
import { CARS } from "../data/catalog";
import { loadFixture } from "./fixtures";
import { applyEvent, combineSeverity, contactSeverity, emptyPlayback, markersFor, regionAt, type Playback, type ReduceContext } from "./reduce";
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

describe("synthetic pursuit (wp_police event shapes)", () => {
  const run = loadFixture("synthetic_pursuit");

  it("raises the wanted level to L5, then clears on escape", () => {
    expect(foldAll(run, 1, 70 * 240).heat.level).toBe("L5");
    const end = foldAll(run, 1);
    expect(end.heat.level).toBe("None");
    expect(end.heat.state).toBe("clear");
  });

  it("tracks pursuit start / end and disabled units", () => {
    const mid = foldAll(run, 1, 35 * 240);
    expect(mid.heat.state).toBe("pursuit");
    expect(mid.units.find((u) => u.unit === 4)?.disabled).toBe(true);
    expect(foldAll(run, 1).pursued).toEqual([]);
  });

  it("uses RegionDamaged directly (no inferred damage) and reads typed radio + economy events", () => {
    const p = foldAll(run, 1);
    expect(p.regions.FrontBumper).toBeCloseTo(0.85, 5);
    expect(p.regions.LeftDoorF).toBeCloseTo(0.55, 5);
    // wp_police radio text is shown as-is.
    expect(p.scanner.some((s) => s.channel === "unit" && s.text === "Unit 5 deploying spikes on segment 120.")).toBe(true);
    expect(p.economy[0]?.bounty_delta).toBe(2500);
    // Tools were placed and removed again.
    expect(p.tools).toEqual([]);
    expect(foldAll(run, 1, 50 * 240).tools.map((t) => t.tool)).toEqual(["SpikeStrip"]);
    expect(p.units.find((u) => u.unit === 6)?.kind).toBe("Helicopter");
  });

  it("puts violations on the map at their contract position", () => {
    const p = foldAll(run, 1, 16 * 240);
    const red = p.incidents.find((i) => i.kind === "RedLight");
    expect(red).toBeDefined();
    const ev = run.events.find((l) => "TrafficViolation" in l.event && l.event.TrafficViolation.kind === "RedLight")!;
    const v = (ev.event as Extract<typeof ev.event, { TrafficViolation: unknown }>).TrafficViolation;
    expect(red!.pos).toEqual([v.pos[0], v.pos[1]]);
  });

  it("produces timeline markers", () => {
    const kinds = markersFor(run, 1).map((m) => m.kind);
    expect(kinds).toContain("contact");
    expect(kinds).toContain("police");
    expect(kinds).toContain("violation");
  });
});

describe("recorded wp_police run", () => {
  const run = loadFixture("police_pursuit");

  it("reads every typed police event as a known contract variant", () => {
    const known = ["Radio", "WantedLevelChanged", "PursuitStarted", "PursuitEnded", "UnitSpawned", "UnitDespawned", "UnitDisabled", "RoadblockPlaced", "SpikesPlaced", "ToolRemoved", "SpikeHit", "HeloLaunched", "PitAttempt"];
    const ctx = ctxFor(run, 1);
    let n = 0;
    for (const l of run.events) {
      if (!("Police" in l.event)) continue;
      n++;
      applyEvent(emptyPlayback(), l, ctx);
      expect(known).toContain(Object.keys(l.event.Police)[0]);
    }
    expect(n).toBeGreaterThan(20);
  });

  it("has a pursuit, spike strips and captain views", () => {
    const p = foldAll(run, 1);
    expect(p.scanner.some((s) => s.text.startsWith("Pursuit:"))).toBe(true);
    // Spike strips are deployed (whether the scripted racer drives over one depends on E's
    // placement; the synthetic run covers the hit).
    expect(run.events.some((l) => "Police" in l.event && "SpikesPlaced" in l.event.Police)).toBe(true);
    expect(run.captain.length).toBeGreaterThan(50);
    expect(run.captain[0].view.units.length).toBeGreaterThan(0);
    expect(run.map?.roads.length).toBeGreaterThan(50);
  });
});
