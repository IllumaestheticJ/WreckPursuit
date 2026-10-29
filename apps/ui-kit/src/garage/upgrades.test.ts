import { describe, expect, it } from "vitest";
import type { CatalogEntry, InventoryView, ModOption, ModTreeView, SlotView, WorkshopScreen } from "./types";
import { boltOns, missingFor, onCar, ownedState, plan, planNames, upgradeGroup } from "./upgrades";

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

  it("does not block on the catalog's needs-fitted-first note for a prerequisite in the plan", () => {
    const t = tree([mod("kit", { requires: ["pads"] }), mod("pads")]);
    const s = screen([entry("kit", { reason: "needs pads fitted first" })]);
    expect(plan(s, t, "kit").blocked).toBeNull();
    const other = screen([entry("kit", { reason: "needs wing fitted first" })]);
    expect(plan(other, t, "kit").blocked).toContain("wing");
  });

  it("keeps cosmetics and part grades out of the bolt-on list", () => {
    const t = tree([mod("pads", { slot: "Brakes" }), mod("tint", { group: "cosmetic" }), mod("kit")]);
    const bolts = boltOns(t, [entry("kit")]);
    expect(bolts.map((m) => m.id)).toEqual(["pads"]);
    expect(upgradeGroup(bolts[0])).toBe("Brakes");
    expect(upgradeGroup(mod("w", { slot: "Weight" }))).toBe("Interior");
  });

  it("marks a part grade fitted where it would go as on the car (greyed, nothing to buy)", () => {
    const e = entry("rad_1", { kind: "radiator" });
    const s = screen([e], { slots: [slot("radiator", "radiator", "rad_1")] });
    expect(ownedState(s, e, mod("rad_1"), "radiator")?.state).toBe("fitted");
    expect(ownedState(s, e, mod("rad_1"), null)?.label).toBe("on the car");
    // A different grade on the car: for sale.
    const other = screen([e], { slots: [slot("radiator", "radiator", "rad_0")] });
    expect(ownedState(other, e, mod("rad_1"), "radiator")).toBeNull();
  });

  it("needs every slot of a set to carry the grade, and the opened slot for a single part", () => {
    const kit = entry("kit_2", { kind: "brake", count: 4 });
    const corners = ["fl", "fr", "rl", "rr"];
    const half = screen([kit], { slots: corners.map((c, i) => slot(c, "brake", i < 2 ? "kit_2" : null)) });
    expect(ownedState(half, kit, mod("kit_2"), "fl")).toBeNull();
    const all = screen([kit], { slots: corners.map((c) => slot(c, "brake", "kit_2")) });
    expect(ownedState(all, kit, mod("kit_2"), "fl")?.state).toBe("fitted");
    // Single parts of a kind with two slots: the slot the row is shown for decides.
    const lamp = entry("lamp_1", { kind: "lamp" });
    const lamps = screen([lamp], { slots: [slot("lamp_l", "lamp", "lamp_1"), slot("lamp_r", "lamp", null)] });
    expect(ownedState(lamps, lamp, null, "lamp_l")?.state).toBe("fitted");
    expect(ownedState(lamps, lamp, null, "lamp_r")).toBeNull();
  });

  it("only counts slots on the car (parts inside a removed assembly are not)", () => {
    const e = entry("cam_2", { kind: "cam" });
    const s = screen([e], { slots: [slot("engine", "engine", undefined), { ...slot("cam", "cam", "cam_2"), parent: "engine" }] });
    expect(onCar(s).map((x) => x.id)).toEqual(["engine"]);
    // No slot of the kind on the car: the mod tree decides.
    expect(ownedState(s, e, mod("cam_2"), null)).toBeNull();
    expect(ownedState(s, e, mod("cam_2", { installed: true }), null)?.state).toBe("fitted");
  });

  it("marks a part grade in the inventory as owned: no Buy, the mechanic fits that part", () => {
    const e = entry("rad_1", { kind: "radiator" });
    const s = screen([e], { slots: [slot("radiator", "radiator", null)], inventory: [loose(7, "radiator", "rad_1", ["radiator"])] });
    const o = ownedState(s, e, mod("rad_1"), "radiator");
    expect(o?.state).toBe("inventory");
    expect(o?.part?.part.uid).toBe(7);
    // Fitted beats in the inventory; a set is never "in the inventory" (one part at a time).
    const on = screen([e], { slots: [slot("radiator", "radiator", "rad_1")], inventory: [loose(7, "radiator", "rad_1", [])] });
    expect(ownedState(on, e, mod("rad_1"), "radiator")?.state).toBe("fitted");
    const set = entry("tyre_2", { kind: "tyre", count: 4 });
    const tyres = screen([set], { slots: [slot("t1", "tyre", null)], inventory: [loose(8, "tyre", "tyre_2", ["t1"])] });
    expect(ownedState(tyres, set, null, "t1")).toBeNull();
  });

  it("never marks stock parts as owned, and marks installed bolt-ons (Remove stays)", () => {
    const stock = entry("x", { kind: "radiator", grade: null });
    const s = screen([stock], { slots: [slot("radiator", "radiator", null)] });
    expect(ownedState(s, stock, null, "radiator")).toBeNull();
    expect(ownedState(s, null, mod("pads", { installed: true }))?.state).toBe("installed");
    expect(ownedState(s, null, mod("pads"))).toBeNull();
    expect(ownedState(s, null, null)).toBeNull();
  });
});

/** A slot on the car; `grade` undefined = empty, null = a stock part. */
function slot(id: string, kind: string, grade: string | null | undefined): SlotView {
  return {
    id,
    name: id,
    group: "Engine",
    kind,
    parent: null,
    fitted: grade === undefined ? null : { grade, name: grade ?? "stock", tier: 1, condition: 1 },
    required: false,
    blocked_by: [],
    remove_blocker: null,
    in_the_way: [],
    mechanic_fee: 100,
  };
}

function loose(uid: number, kind: string, grade: string, fits: string[]): InventoryView {
  return { part: { uid, kind, grade, model: null, condition: 1, damage: [], value: 0 }, name: grade, fits, stand_fits: [], reason: null, sale_price: 0 };
}
