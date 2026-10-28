import { describe, expect, it } from "vitest";
import { canWalkLot, garageAccess, OPEN } from "./access";

const runs = { drivable: true };
const wrecked = { drivable: false };

describe("garageAccess", () => {
  it("keeps the old menu open for hosts that do not track the place", () => {
    expect(garageAccess(undefined, runs)).toEqual(OPEN);
  });

  it("closes garage work out on the road, but tows a wreck", () => {
    const a = garageAccess(null, runs);
    expect([a.workshop, a.mods, a.tune, a.tires, a.sell, a.repair]).toEqual([false, false, false, false, false, false]);
    const w = garageAccess(null, wrecked);
    expect(w.repair).toBe(true);
    expect(w.tow).toBe(true);
    expect(w.repairShop).toBeNull();
  });

  it("repairs only at the shop pulled into", () => {
    const a = garageAccess({ kind: "Garage", id: "Gantry Garage", name: "Gantry Garage" }, wrecked);
    expect(a.repair && a.mods && a.workshop && a.tune && a.tires).toBe(true);
    expect(a.repairShop).toBe("Gantry Garage");
    expect(a.tow).toBe(false);
    expect(a.sell).toBe(false);
  });

  it("has no mechanic at home, and the dealer buys at a lot", () => {
    const home = garageAccess({ kind: "Home", id: "vesper_lockup", name: "Dockside Lock-up" }, runs);
    expect(home.mods && home.workshop && home.tune).toBe(true);
    expect(home.repair).toBe(false);
    const lot = garageAccess({ kind: "Lot", id: "budget_motors", name: "Budget Motors" }, runs);
    expect(lot.sell).toBe(true);
    expect(lot.workshop || lot.mods).toBe(false);
  });
});

describe("canWalkLot", () => {
  it("is the lot pulled into, or a taxi without a car", () => {
    const at = { kind: "Lot" as const, id: "budget_motors", name: "Budget Motors" };
    expect(canWalkLot(at, "budget_motors", true)).toBe(true);
    expect(canWalkLot(at, "dockside_salvage", true)).toBe(false);
    expect(canWalkLot(null, "dockside_salvage", true)).toBe(false);
    expect(canWalkLot(null, "dockside_salvage", false)).toBe(true);
    expect(canWalkLot(undefined, "dockside_salvage", true)).toBe(true);
  });
});
