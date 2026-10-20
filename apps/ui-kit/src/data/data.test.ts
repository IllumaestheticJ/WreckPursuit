import { describe, expect, it } from "vitest";
import { mockGarage, mockShops } from "../mock/mockData";
import { CAR_STATS, CARS, MODS, modBlocker, modTree } from "./catalog";
import { marketPrice, quoteRepair, salePrice, scrapValue } from "./economy";
import { bracket, classIndex, estimate, previewBuild, TIRE_GRIP } from "./estimate";

describe("catalog", () => {
  it("loads the real roster and mod catalog", () => {
    expect(Object.keys(CARS).length).toBeGreaterThanOrEqual(13);
    expect(Object.keys(MODS).length).toBeGreaterThan(80);
    expect(modTree()[0].slot).toBe("Engine");
  });

  it("has physics-measured stat bars for every car", () => {
    for (const id of Object.keys(CARS)) {
      const s = CAR_STATS[id];
      expect(s, id).toBeDefined();
      for (const v of Object.values(s.bars)) {
        expect(v).toBeGreaterThanOrEqual(0);
        expect(v).toBeLessThanOrEqual(1);
      }
      expect(s.top_speed_kmh).toBeGreaterThan(100);
    }
  });

  it("enforces requirements, class and mode limits", () => {
    expect(modBlocker(MODS.engine_ecu_2, [], "Sport", "Arcade")).toMatch(/Requires/);
    expect(modBlocker(MODS.engine_ecu_2, ["engine_intake_1"], "Sport", "Arcade")).toBeNull();
    // A higher tier of the same line satisfies the requirement.
    expect(modBlocker(MODS.engine_ecu_2, ["engine_intake_2"], "Sport", "Arcade")).toBeNull();
    const arcadeOnly = Object.values(MODS).find((m) => m.modes?.length === 1 && m.modes[0] === "Arcade");
    if (arcadeOnly) expect(modBlocker(arcadeOnly, MODS[arcadeOnly.requires?.[0] ?? ""] ? arcadeOnly.requires! : [], "Sport", "Simulation")).toMatch(/mode/);
  });
});

describe("estimator port", () => {
  it("gives plausible numbers for every car", () => {
    for (const def of Object.values(CARS)) {
      const p = estimate(def, TIRE_GRIP[def.wheels[0].tire_id] ?? 1);
      expect(p.peakPowerKw).toBeGreaterThan(40);
      expect(p.est0to100S).toBeGreaterThan(2);
      expect(p.est0to100S).toBeLessThan(20);
      expect(p.estTopSpeedMps).toBeGreaterThan(30);
      const ci = classIndex(p);
      expect(ci).toBeGreaterThanOrEqual(100);
      expect(ci).toBeLessThanOrEqual(999);
    }
  });

  it("more power and grip raise the class index; brackets follow", () => {
    const base = previewBuild(CARS.shiro_turbo, []);
    const built = previewBuild(CARS.shiro_turbo, ["engine_intake_1", "engine_ecu_1", "tires_semi_slick"]);
    expect(built.perf.peakPowerKw).toBeGreaterThan(base.perf.peakPowerKw);
    expect(built.classIndex).toBeGreaterThan(base.classIndex);
    expect(bracket(100)).toBe("D");
    expect(bracket(455)).toBe("B");
    expect(bracket(999)).toBe("X");
  });

  it("protection adds mass and durability", () => {
    const p = previewBuild(CARS.shiro_turbo, ["prot_bumper_front"]);
    expect(p.def.mass_kg).toBeCloseTo(CARS.shiro_turbo.mass_kg + 20, 3);
    expect(p.durability.FrontBumper).toBeCloseTo(1.8);
  });
});

describe("pricing", () => {
  const [shiro, , ironside] = mockGarage();
  const [legit, crooked] = mockShops();

  it("Arcade bills half of Simulation", () => {
    const sim = quoteRepair({ ...shiro, insurance: null }, { kind: "RepairAll" }, legit, "Simulation", 1e9, false);
    const arc = quoteRepair({ ...shiro, insurance: null }, { kind: "RepairAll" }, legit, "Arcade", 1e9, false);
    expect(arc.total).toBeGreaterThan(0);
    expect(Math.abs(arc.total * 2 - sim.total)).toBeLessThanOrEqual(sim.lines.length);
  });

  it("denies insurance for damage sustained while wanted", () => {
    const q = quoteRepair(shiro, { kind: "RepairAll" }, legit, "Simulation", 1e9, false);
    expect(q.insuranceDenied).toBe(true);
    expect(q.insuranceCovered).toBe(0);
    const ok = quoteRepair({ ...shiro, sustainedWhileWanted: false }, { kind: "RepairAll" }, legit, "Simulation", 1e9, false);
    expect(ok.insuranceCovered).toBeGreaterThan(0);
    expect(ok.total).toBeLessThan(ok.subtotal);
  });

  it("crooked shops mark up a wanted car", () => {
    const calm = quoteRepair({ ...shiro, insurance: null }, { kind: "RepairAll" }, crooked, "Simulation", 1e9, false);
    const hot = quoteRepair({ ...shiro, insurance: null }, { kind: "RepairAll" }, crooked, "Simulation", 1e9, true);
    expect(hot.total).toBeGreaterThan(calm.total);
  });

  it("selected lines, drive damaged, scrap", () => {
    const sel = quoteRepair({ ...shiro, insurance: null }, { kind: "RepairSelected", lines: [0] }, legit, "Simulation", 1e9, false);
    expect(sel.lines.length).toBe(1);
    expect(quoteRepair(shiro, { kind: "DriveDamaged" }, legit, "Arcade", 0, false).total).toBe(0);
    const scrap = quoteRepair(ironside, { kind: "Scrap" }, legit, "Arcade", 0, false);
    expect(scrap.total).toBe(-scrapValue(ironside));
    expect(scrap.affordable).toBe(true);
  });

  it("market and sale prices follow market.toml", () => {
    expect(marketPrice(28000, "Sport", "Used")).toBe(25000); // 22 400 clamped to the Sport band
    expect(marketPrice(7500, "Compact", "New")).toBe(9000);
    expect(salePrice({ ...shiro, totalLoss: false })).toBeLessThan(shiro.basePrice);
  });
});
