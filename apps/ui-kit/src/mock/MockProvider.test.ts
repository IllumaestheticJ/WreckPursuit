import { describe, expect, it } from "vitest";
import { createMockModel } from "./MockProvider";

const manual = (run = "synthetic_pursuit") => createMockModel({ run, clock: "manual", autoplay: true, loop: false });

describe("mock provider", () => {
  it("replays telemetry into the HUD", () => {
    const m = manual();
    m.advance(10);
    const hud = m.get().hud;
    expect(m.get().replay.tick).toBe(2400);
    expect(hud.speedMps).toBeGreaterThan(5);
    expect(hud.gear).toBeGreaterThan(0);
    expect(hud.heat.state).toBe("pursuit");
    expect(hud.race?.of).toBe(2);
    expect(m.get().scanner.length).toBeGreaterThan(3);
  });

  it("seeks backward and forward deterministically", () => {
    const m = manual();
    m.send({ type: "replay.seek", tick: 65 * 240 });
    const a = m.get().hud;
    m.send({ type: "replay.seek", tick: 5 * 240 });
    expect(m.get().hud.regions).toEqual({});
    m.send({ type: "replay.seek", tick: 65 * 240 });
    expect(m.get().hud.regions).toEqual(a.regions);
    expect(m.get().hud.heat.level).toBe(a.heat.level);
  });

  it("stops at the end without loop", () => {
    const m = manual("golden_tbone");
    m.advance(100);
    expect(m.get().replay.playing).toBe(false);
    expect(m.get().replay.tick).toBe(m.get().replay.totalTicks);
  });

  it("loads another run", () => {
    const m = manual();
    m.send({ type: "replay.load", source: "golden_frontal_56kmh" });
    expect(m.get().replay.source).toBe("golden_frontal_56kmh");
    m.advance(7);
    expect(m.get().hud.regions.FrontBumper ?? 0).toBeGreaterThan(0.5);
    m.send({ type: "replay.load", source: "nope" });
    expect(m.get().notice?.kind).toBe("error");
  });

  it("refuses an unaffordable purchase and accepts an affordable one", () => {
    const m = manual();
    const cash = m.get().account.cash;
    m.send({ type: "market.buy", defId: "lucciola_350", condition: "New" });
    expect(m.get().notice?.text).toMatch(/Not enough cash/);
    expect(m.get().account.cash).toBe(cash);
    m.send({ type: "market.buy", defId: "vireo_xs", condition: "Worn" });
    expect(m.get().account.cash).toBe(cash - 9000);
    expect(m.get().garage.cars.some((c) => c.defId === "vireo_xs" && c.location.kind === "Garage")).toBe(true);
  });

  it("repairs a car and charges for it", () => {
    const m = manual();
    m.send({ type: "settings.damageMode", mode: "Arcade" });
    const before = m.get().account.cash;
    m.send({ type: "garage.repair", car: 1, choice: { kind: "RepairSelected", lines: [0] } });
    const car = m.get().garage.cars.find((c) => c.uid === 1)!;
    expect(m.get().account.cash).toBeLessThan(before);
    expect(car.outstanding.lines.length).toBe(7);
    m.send({ type: "garage.repair", car: 1, choice: { kind: "RepairAll" } });
    expect(m.get().garage.cars.find((c) => c.uid === 1)!.health).toBe(1);
  });

  it("installs mods only when requirements are met, replacing the same line", () => {
    const m = manual();
    m.send({ type: "mods.install", car: 2, mod: "engine_ecu_2" });
    expect(m.get().notice?.text).toMatch(/Requires/);
    m.send({ type: "mods.install", car: 2, mod: "engine_intake_1" });
    m.send({ type: "mods.install", car: 2, mod: "engine_intake_2" });
    const mods = m.get().garage.cars.find((c) => c.uid === 2)!.mods;
    expect(mods).toContain("engine_intake_2");
    expect(mods).not.toContain("engine_intake_1");
  });

  it("toy captain follows wp_police rules: orders, cooldowns, locks, budget", () => {
    const m = manual();
    // Before any pursuit, ordering a unit onto the racer is refused (NotPursued).
    m.advance(5);
    const unit = m.get().captain.units[1];
    m.send({ type: "captain", command: { AssignUnit: { unit: unit.id, order: { Pursue: { target: 1 } } } } });
    expect(m.get().notice?.text).toMatch(/not being pursued/);
    // During the pursuit at L3 it works, and the unit drives toward the racer.
    m.send({ type: "replay.seek", tick: 35 * 240 });
    m.send({ type: "captain", command: { AssignUnit: { unit: unit.id, order: { Pursue: { target: 1 } } } } });
    const before = m.get().captain.units.find((u) => u.id === unit.id)!;
    expect(before.assignment).toBe("pursue");
    m.advance(2);
    const after = m.get().captain.units.find((u) => u.id === unit.id)!;
    expect(after.pos).not.toEqual(before.pos);
    // Spikes: allowed at L3, then cooling down.
    m.send({ type: "captain", command: { RequestSpikes: { target: 1, segment: null } } });
    expect(m.get().captain.readiness.spikes[0]).toBeGreaterThanOrEqual(1);
    m.send({ type: "captain", command: { RequestSpikes: { target: 1, segment: null } } });
    expect(m.get().notice?.text).toMatch(/Cooling down/);
    // Roadblocks unlock at L3: fine now; a second one hits the budget / cooldown.
    m.send({ type: "captain", command: { RequestRoadblock: { target: 1, segment: null } } });
    expect(m.get().captain.tools.some((t) => t.tool === "Roadblock")).toBe(true);
    m.send({ type: "captain", command: { SetPriorityTarget: { target: 1 } } });
    expect(m.get().captain.priorityTarget).toBe(1);
  });

  it("recorded wp_police run drives the captain map and HUD heat", () => {
    const m = manual("police_pursuit");
    expect(m.get().captain.source).toBe("wp_police");
    expect(m.get().map.id).toBe("police_procedural_10x10");
    m.send({ type: "replay.seek", tick: 100 * 240 });
    const s = m.get();
    expect(s.captain.units.length).toBeGreaterThan(3);
    expect(s.captain.suspects[0]?.id).toBe(1);
    expect(s.hud.heat.level).not.toBe("None");
    expect(["pursuit", "cooldown", "noticed"]).toContain(s.hud.heat.state);
    // Orders are validated and queued (the recording itself cannot change).
    const car = s.captain.units.find((u) => u.kind === "Car" && !["out-of-service", "roadblock", "spikes"].includes(u.assignment))!;
    m.send({ type: "captain", command: { AssignUnit: { unit: car.id, order: "Disengage" } } });
    expect(m.get().notice?.text).toMatch(/Sent to wp_police/);
    expect(m.get().captain.units.find((u) => u.id === car.id)?.pendingOrder).toBe("Disengage");
  });

  it("assist presets and bindings", () => {
    const m = manual();
    m.send({ type: "settings.assistPreset", preset: "sim" });
    expect(m.get().settings.assists.tcs).toBe(false);
    m.send({ type: "settings.patch", patch: { assists: { ...m.get().settings.assists, tcs: true } } });
    expect(m.get().settings.assistPreset).toBe("custom");
    m.send({ type: "settings.unbind", action: "pause", device: "keyboard" });
    expect(m.get().notice?.kind).toBe("error");
  });

  it("navigation: pause stops playback, resume restarts it", () => {
    const m = manual();
    m.send({ type: "pause" });
    expect(m.get().screen).toBe("pause");
    expect(m.get().replay.playing).toBe(false);
    m.send({ type: "navigate", screen: "settings" });
    m.send({ type: "back" });
    expect(m.get().screen).toBe("pause");
    m.send({ type: "resume" });
    expect(m.get().screen).toBe("hud");
    expect(m.get().replay.playing).toBe(true);
  });
});
