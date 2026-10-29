/**
 * Upgrade buying in the garage workshop: what can be bought for a slot, what has to go on
 * first, and the steps that buy (or buy and fit) a part together with everything it needs.
 *
 * Two kinds of mods are sold in the workshop:
 * - part grades (`CatalogEntry` with a grade): bought as parts into the inventory, fitted by
 *   hand or by the mechanic;
 * - bolt-on upgrades: mods that are not parts (brake pads, ABS, radar, final drive...), put
 *   straight on the car. Cosmetic mods are the paint shop's.
 *
 * Pure functions over the screens wp_wasm returns, so they are unit tested (upgrades.test.ts).
 */
import type { CatalogEntry, InventoryView, ModOption, ModTreeView, PartGroup, SlotView, WorkshopScreen } from "./types";

/** Part group a bolt-on upgrade is listed under, by its `ModSlot`. */
const UPGRADE_GROUP: Record<string, PartGroup> = {
  Engine: "Engine",
  ForcedInduction: "Engine",
  Drivetrain: "Drivetrain",
  Suspension: "Suspension",
  Brakes: "Brakes",
  Tires: "Wheels",
  Wheels: "Wheels",
  Weight: "Interior",
  Interior: "Interior",
  Aero: "Aero",
  BodyFront: "Body",
  BodySides: "Body",
  BodyRear: "Body",
  Hood: "Body",
  Roof: "Body",
  Protection: "Protection",
  Cooling: "Cooling",
  Electronics: "Electronics",
};

/** Part group of a bolt-on upgrade (`Body` for a slot this table does not know). */
export function upgradeGroup(m: ModOption): PartGroup {
  return UPGRADE_GROUP[m.slot] ?? "Body";
}

/** Bolt-on upgrades of the mod tree: not a part grade, not cosmetic. */
export function boltOns(tree: ModTreeView, catalog: CatalogEntry[]): ModOption[] {
  const parts = new Set(catalog.map((c) => c.grade).filter((g): g is string => !!g));
  return tree.mods.filter((m) => m.group !== "cosmetic" && !parts.has(m.id));
}

/** `req` is met by an installed mod of its line at its tier or above (wp_economy rule). */
function met(tree: ModTreeView, req: ModOption): boolean {
  return tree.mods.some((m) => m.installed && m.line === req.line && m.tier >= req.tier);
}

/**
 * Prerequisites of mod `id` that are not on the car, in the order they must go on (what a
 * requirement needs comes before it). Unknown ids are left out.
 */
export function missingFor(tree: ModTreeView, id: string): ModOption[] {
  const out: ModOption[] = [];
  const seen = new Set<string>();
  const visit = (mid: string) => {
    const m = tree.mods.find((x) => x.id === mid);
    if (!m) return;
    for (const r of m.requires) {
      if (seen.has(r)) continue;
      seen.add(r);
      const req = tree.mods.find((x) => x.id === r);
      if (!req || met(tree, req)) continue;
      visit(r);
      out.push(req);
    }
  };
  visit(id);
  return out;
}

/** One step of a combined purchase. */
export interface Step {
  mod: ModOption;
  /** The catalog line when the mod is a part grade (null = bolt-on upgrade). */
  entry: CatalogEntry | null;
  /** A part of this grade already in the inventory that fits the car (nothing to buy). */
  owned: InventoryView | null;
}

/** A purchase of a mod and everything it needs first. */
export interface Plan {
  /** Prerequisites first, the mod asked for last. */
  steps: Step[];
  /** Money for the parts and upgrades bought (owned parts cost nothing). */
  price: number;
  /** Mechanic labour when every part is fitted for you. */
  labour: number;
  /** Why the plan cannot be bought at all (a prerequisite this workshop does not sell...). */
  blocked: string | null;
}

/** An inventory part of mod `m`'s grade (one part, not a set) that fits the car now or later. */
function ownedPart(screen: WorkshopScreen, m: ModOption, entry: CatalogEntry | null): InventoryView | null {
  if (!entry || entry.count > 1) return null;
  return screen.inventory.find((p) => p.part.grade === m.id) ?? null;
}

/** Plan buying mod `id` with every prerequisite it is missing. */
export function plan(screen: WorkshopScreen, tree: ModTreeView, id: string): Plan {
  const target = tree.mods.find((m) => m.id === id);
  if (!target) return { steps: [], price: 0, labour: 0, blocked: `unknown mod ${id}` };
  let blocked: string | null = null;
  const chain = [...missingFor(tree, id), target];
  // The catalog says "needs <part> fitted first" for a part whose prerequisite is not on the
  // car (wp_economy `parts_catalog`); the plan fits that prerequisite first, so it does not block.
  const planned = new Set(chain.map((m) => `needs ${m.name} fitted first`));
  const steps = chain.map((mod): Step => {
    const entry = screen.catalog.find((c) => c.grade === mod.id) ?? null;
    if (entry?.reason && !planned.has(entry.reason)) blocked ??= `${mod.name}: ${entry.reason}`;
    if (!entry && mod.group === "cosmetic") blocked ??= `${mod.name} is sold at the paint shop`;
    return { mod, entry, owned: ownedPart(screen, mod, entry) };
  });
  let price = 0;
  let labour = 0;
  for (const s of steps) {
    if (!s.owned) price += s.entry ? s.entry.price : s.mod.price;
    if (s.entry) labour += s.entry.install_fee;
  }
  return { steps, price, labour, blocked };
}

/** "Brake pads + ABS": the names of a plan's steps. */
export function planNames(p: Plan): string {
  return p.steps.map((s) => s.mod.name).join(" + ");
}

/** Slots that are on the car now: parts inside an assembly only while the assembly is fitted. */
export function onCar(screen: WorkshopScreen): SlotView[] {
  const fitted = new Set(screen.slots.filter((s) => s.fitted).map((s) => s.id));
  return screen.slots.filter((s) => !s.parent || fitted.has(s.parent));
}

/**
 * The slots on the car a catalog line of `entry` would go in: the slot the row is shown for
 * (`prefer`) when it takes a single part of that kind, else every slot of the kind (a set,
 * four brake corners, goes in all of them).
 */
function slotsFor(screen: WorkshopScreen, entry: CatalogEntry, prefer?: string | null): SlotView[] {
  const kind = onCar(screen).filter((s) => s.kind === entry.kind);
  const here = entry.count <= 1 ? kind.find((s) => s.id === prefer) : undefined;
  return here ? [here] : kind;
}

/** What the player already has of an upgrade row (null = nothing: it is for sale as usual). */
export interface Owned {
  /**
   * - `fitted`: the part grade is on the car where it would go (every slot of a set);
   * - `installed`: the bolt-on upgrade is on the car;
   * - `inventory`: a part of the grade sits in the inventory. Buying another makes no sense,
   *   but the mechanic can still fit the one the player has (labour only).
   */
  state: "fitted" | "installed" | "inventory";
  /** Short tag next to the name ("on the car", "in the inventory"). */
  label: string;
  /** Tooltip of the tag: what it means for the buttons. */
  title: string;
  /** The inventory part (state `inventory`), to fit instead of buying one. */
  part: InventoryView | null;
}

/**
 * Whether the player already has what an upgrade row sells: a part grade (`entry`, with its
 * mod-tree line `mod`) or a bolt-on upgrade (`entry` null). Rows the player has are greyed out
 * and their buy buttons disabled; an installed bolt-on keeps its Remove button.
 *
 * Stock parts (grade null) are never "owned": buying a new stock part replaces a worn one.
 * A set (`count` > 1) is never `inventory`: the mechanic fits one inventory part at a time.
 */
export function ownedState(screen: WorkshopScreen, entry: CatalogEntry | null, mod: ModOption | null, prefer?: string | null): Owned | null {
  if (!entry) {
    return mod?.installed
      ? { state: "installed", label: "on the car", title: "already installed on this car; Remove takes it off (40 % back)", part: null }
      : null;
  }
  if (entry.grade === null) return null;
  const slots = slotsFor(screen, entry, prefer);
  const fitted = slots.length > 0 ? slots.every((s) => s.fitted?.grade === entry.grade) : !!mod?.installed;
  if (fitted) {
    return { state: "fitted", label: "on the car", title: `${entry.name} is already fitted${entry.count > 1 ? " on every corner" : ""}: nothing to buy`, part: null };
  }
  const loose = screen.inventory.filter((p) => p.part.kind === entry.kind && p.part.grade === entry.grade);
  if (entry.count <= 1 && loose.length > 0) {
    // The one that fits where the row would fit it, if any.
    const part = loose.find((p) => slots.some((s) => p.fits.includes(s.id))) ?? loose[0];
    return {
      state: "inventory",
      label: "owned",
      title: "you have one in the inventory: fit it yourself, or have the mechanic fit it (labour only)",
      part,
    };
  }
  return null;
}
