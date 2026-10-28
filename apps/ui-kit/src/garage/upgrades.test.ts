import { describe, expect, it } from "vitest";
import type { CatalogEntry, ModOption, ModTreeView, WorkshopScreen } from "./types";
import { boltOns, missingFor, plan, planNames, upgradeGroup } from "./upgrades";

const mod = (id: string, over: Partial<ModOption> = {}): ModOption => ({
  id,
  name: id,
  slot: "Engine",
  line: id,
  tier: 1,
  price: 1000,
  tradeoff: "",
  group: "performance",
  installed: false,
  allowed: true,
  reason: null,
  replaces: null,
  stats_after: null,
  requires: [],
  removes: [],
  ...over,
});

const entry = (grade: string, over: Partial<CatalogEntry> = {}): CatalogEntry => ({
  kind: grade,
  slot_name: grade,
  group: "Engine",
  grade,
  name: grade,
  tier: 1,
  price: 1000,
  count: 1,
  reason: null,
  install_fee: 100,
  ...over,
});

const tree = (mods: ModOption[]): ModTreeView => ({ uid: 1, stats: {} as ModTreeView["stats"], mods });

const screen = (catalog: CatalogEntry[], over: Partial<WorkshopScreen> = {}): WorkshopScreen => ({
  car: 1,
  slots: [],
  missing: [],
  upgrades: [],
  inventory: [],
  stands: [],
  torque_factor: 1,
  power_hp: 150,
  stock_power_hp: 150,
  catalog,
  cash: 100000,
  def_id: "x",
  ...over,
});

describe("workshop upgrades", () => {
  it("lists what a mod needs first, deepest first, skipping what is on the car", () => {
    const t = tree([
      mod("turbo", { requires: ["fuel", "internals"] }),
      mod("fuel"),
      mod("internals", { requires: ["rods"] }),
      mod("rods", { installed: true }),
    ]);
    expect(missingFor(t, "turbo").map((m) => m.id)).toEqual(["fuel", "internals"]);
    expect(missingFor(t, "fuel")).toEqual([]);
  });

  it("counts a higher tier of the same line as meeting a requirement", () => {
    const t = tree([
      mod("ecu_2", { requires: ["intake_1"] }),
      mod("intake_1", { line: "intake", tier: 1 }),
      mod("intake_2", { line: "intake", tier: 2, installed: true }),
    ]);
    expect(missingFor(t, "ecu_2")).toEqual([]);
  });

  it("orders a chain so each requirement goes on before what needs it", () => {
    const t = tree([mod("c", { requires: ["b"] }), mod("b", { requires: ["a"] }), mod("a")]);
    expect(missingFor(t, "c").map((m) => m.id)).toEqual(["a", "b"]);
  });

  it("prices a plan: parts plus labour, bolt-ons at their price, owned parts free", () => {
    const t = tree([mod("abs", { slot: "Brakes", price: 900, requires: ["kit"] }), mod("kit", { slot: "Brakes" })]);
    const s = screen([entry("kit", { price: 2000, install_fee: 150 })]);
    const p = plan(s, t, "abs");
    expect(p.steps.map((x) => x.mod.id)).toEqual(["kit", "abs"]);
    expect(p.price).toBe(2900);
    expect(p.labour).toBe(150);
    expect(p.blocked).toBeNull();
    expect(planNames(p)).toBe("kit + abs");

    const owned = screen([entry("kit", { price: 2000, install_fee: 150 })], {
      inventory: [
        {
          part: { uid: 7, kind: "kit", grade: "kit", model: null, condition: 1, damage: [], value: 0 },
          name: "kit",
          fits: [],
          stand_fits: [],
          reason: null,
          sale_price: 0,
        },
      ],
    });
    const q = plan(owned, t, "abs");
    expect(q.steps[0].owned?.part.uid).toBe(7);
    expect(q.price).toBe(900);
  });

  it("blocks a plan with a prerequisite the car cannot take", () => {
    const t = tree([mod("wing", { requires: ["splitter"] }), mod("splitter")]);
    const s = screen([entry("wing"), entry("splitter", { reason: "not for class Bus" })]);
    expect(plan(s, t, "wing").blocked).toContain("splitter");
  });

  it("keeps cosmetics and part grades out of the bolt-on list", () => {
    const t = tree([mod("pads", { slot: "Brakes" }), mod("tint", { group: "cosmetic" }), mod("kit")]);
    const bolts = boltOns(t, [entry("kit")]);
    expect(bolts.map((m) => m.id)).toEqual(["pads"]);
    expect(upgradeGroup(bolts[0])).toBe("Brakes");
    expect(upgradeGroup(mod("w", { slot: "Weight" }))).toBe("Interior");
  });
});
