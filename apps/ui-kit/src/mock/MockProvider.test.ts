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

  it("captain commands move units and spend charges", () => {
    const m = manual();
    const unit = m.get().captain.units[1];
    m.send({ type: "captain", command: { Assign: { unit: unit.id, target: { Hold: { pos: [unit.pos[0] + 200, unit.pos[1]] } } } } });
    m.advance(2);
    const moved = m.get().captain.units.find((u) => u.id === unit.id)!;
    expect(moved.pos[0]).toBeGreaterThan(unit.pos[0] + 20);
    const spikes = m.get().captain.charges.SpikeStrip;
    m.send({ type: "captain", command: { Deploy: { tool: "SpikeStrip", pos: [0, 0], heading_rad: 0 } } });
    expect(m.get().captain.charges.SpikeStrip).toBe(spikes - 1);
    expect(m.get().captain.tools.some((t) => t.tool === "SpikeStrip" && t.id < 0)).toBe(true);
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
