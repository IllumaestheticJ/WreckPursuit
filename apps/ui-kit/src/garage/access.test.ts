import { describe, expect, it } from "vitest";
import { garageAccess, lotActions, OPEN } from "./access";

const runs = { drivable: true };
const wrecked = { drivable: false };

describe("garageAccess", () => {
  it("keeps the old menu open for hosts that do not track the place", () => {
    expect(garageAccess(undefined, runs)).toEqual(OPEN);
  });

  it("closes garage work out on the road; old hosts (no car spot) tow a wreck to any shop", () => {
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

describe("garageAccess: cars that cannot be driven (tow service)", () => {
  const road = { drivable: false, spot: { kind: "Road" as const, id: "road", name: "out on the road" } };
  const atShop = { drivable: false, spot: { kind: "Garage" as const, id: "Gantry Garage", name: "Gantry Garage" } };
  const atHome = { drivable: false, spot: { kind: "Home" as const, id: "vesper_lockup", name: "Dockside Lock-up" } };
  const shop = { kind: "Garage" as const, id: "Meridian Motorworks", name: "Meridian Motorworks" };

  it("a wreck on the road gets a tow, and nothing else, wherever the player is", () => {
    for (const place of [null, shop, { kind: "Home" as const, id: "vesper_lockup", name: "Dockside Lock-up" }]) {
      const a = garageAccess(place, road);
      expect(a.tow).toBe(true);
      expect([a.repair, a.workshop, a.mods, a.tune, a.tires, a.sell]).toEqual([false, false, false, false, false, false]);
    }
  });

  it("towed to a shop, the car is repaired and worked on at that shop, not the player's", () => {
    for (const place of [null, shop]) {
      const a = garageAccess(place, atShop);
      expect(a.tow).toBe(false);
      expect(a.repair && a.workshop && a.mods && a.tune && a.tires).toBe(true);
      expect(a.repairShop).toBe("Gantry Garage");
    }
  });

  it("a dead car at home has the workshop but needs a tow for repairs", () => {
    const a = garageAccess(null, atHome);
    expect(a.workshop && a.mods && a.tune).toBe(true);
    expect(a.repair).toBe(false);
    expect(a.tow).toBe(true);
  });

  it("a car in the pound is no tow (spot null)", () => {
    const a = garageAccess(null, { drivable: false, spot: null });
    expect(a.tow).toBe(false);
    expect(a.repair).toBe(false);
  });

  it("a car that drives follows the player (drive-to), never a tow", () => {
    const a = garageAccess(null, { drivable: true, spot: { kind: "Road", id: "road", name: "" } });
    expect(a.tow).toBe(false);
    expect(a.repair).toBe(false);
    const b = garageAccess(shop, { drivable: true, spot: atHome.spot });
    expect(b.repair).toBe(true);
    expect(b.repairShop).toBe("Meridian Motorworks");
  });
});

describe("lotActions", () => {
  const at = { kind: "Lot" as const, id: "budget_motors", name: "Budget Motors" };
  it("walks the lot pulled into", () => {
    expect(lotActions(at, "budget_motors", true)).toEqual({ walk: true, taxi: false, waypoint: false });
    expect(lotActions(undefined, "dockside_salvage", true)).toEqual({ walk: true, taxi: false, waypoint: false });
  });
  it("always offers the taxi elsewhere, car or no car (owner 2026-10-29)", () => {
    expect(lotActions(null, "dockside_salvage", true)).toEqual({ walk: false, taxi: true, waypoint: true });
    expect(lotActions(at, "dockside_salvage", true)).toEqual({ walk: false, taxi: true, waypoint: true });
    expect(lotActions({ kind: "Garage", id: "Meridian Motorworks", name: "Meridian Motorworks" }, "dockside_salvage", true).taxi).toBe(true);
    expect(lotActions(null, "dockside_salvage", false)).toEqual({ walk: false, taxi: true, waypoint: false });
  });
});
