/**
 * Real game data the kit reads at build time: car definitions (`data/cars/*.json`, typed by the
 * generated `VehicleDef`) and the mod catalog (`data/mods/*.json`, shaped like
 * `wp_economy::ModDef`). Vite inlines them, so the demo shows the actual roster and mod tree.
 */
import type { BodyRegion, Component, DamageMode, VehicleClass, VehicleDef } from "../generated/contracts";

const carFiles = import.meta.glob("../../../../data/cars/*.json", { eager: true, import: "default" });
const modFiles = import.meta.glob("../../../../data/mods/*.json", { eager: true, import: "default" });
const statFiles = import.meta.glob("../../../../data/stats/cars.json", { eager: true, import: "default" });

/** Every car definition, by id (debug cars included; filter with `isDebugCar`). */
export const CARS: Record<string, VehicleDef> = Object.fromEntries(
  Object.values(carFiles).map((d) => {
    const def = d as VehicleDef;
    return [def.id, def];
  }),
);

/**
 * A car's menu stats (`simcli::car_stats::CarStatsEntry`): measured on the physics rig
 * (`simcli car-stats` → `data/stats/cars.json`), stock car, flat dry asphalt. Bars are 0–1 on
 * fixed scales (0–100 16 → 3.5 s, 140 → 310 km/h, 0.55 → 1.05 g).
 */
export interface CarStats {
  display_name: string;
  zero_to_100_s: number | null;
  top_speed_kmh: number;
  cornering_g: number;
  bars: { acceleration: number; top_speed: number; cornering: number };
}

/** Stock stats by car id (empty if the table is missing). */
export const CAR_STATS: Record<string, CarStats> =
  (Object.values(statFiles)[0] as { cars?: Record<string, CarStats> } | undefined)?.cars ?? {};

/** The lead's debug cars, hidden from the market. */
export function isDebugCar(id: string): boolean {
  return id.startsWith("debug_");
}

/** `ModOp` from `wp_economy::mods`. */
export type ModOp = "Add" | "Mul" | "Set";

/** `ModEffect` from `wp_economy::mods` (externally tagged, as in the JSON). */
export type ModEffect =
  | { Vehicle: { param: string; op: ModOp; value: number | string } }
  | {
      Durability: {
        target: { Region: BodyRegion } | { Component: Component } | "CabinCage";
        scale: number;
        mass_kg: number;
        material?: string | null;
      };
    }
  | { Cosmetic: { kind: string; value: string } }
  | { Tool: Record<string, unknown> }
  | Record<string, unknown>;

/** `ModDef` from `wp_economy::mods`. */
export interface ModDef {
  id: string;
  name: string;
  slot: string;
  line: string;
  tier: number;
  price: number;
  requires?: string[];
  class_whitelist?: VehicleClass[];
  modes?: DamageMode[];
  effects: ModEffect[];
  tradeoff: string;
}

/** Every mod, by id. */
export const MODS: Record<string, ModDef> = {};
/** Slot display order (matches `wp_economy::ModSlot`). */
export const MOD_SLOTS: string[] = [
  "Engine",
  "ForcedInduction",
  "Drivetrain",
  "Suspension",
  "Brakes",
  "Tires",
  "Weight",
  "Aero",
  "Cooling",
  "Protection",
  "BodyFront",
  "BodySides",
  "BodyRear",
  "Hood",
  "Roof",
  "Wheels",
  "Paint",
  "Vinyl",
  "Interior",
  "Electronics",
  "Nitrous",
  "Tools",
];
for (const file of Object.values(modFiles)) {
  for (const m of (file as { mods: ModDef[] }).mods) MODS[m.id] = m;
}
for (const m of Object.values(MODS)) if (!MOD_SLOTS.includes(m.slot)) MOD_SLOTS.push(m.slot);

/** Human label for a slot id. */
export function slotLabel(slot: string): string {
  return slot.replace(/([a-z])([A-Z])/g, "$1 $2");
}

/** Mods grouped as slot → line → mods sorted by tier. */
export function modTree(): { slot: string; lines: { line: string; mods: ModDef[] }[] }[] {
  const bySlot = new Map<string, Map<string, ModDef[]>>();
  for (const m of Object.values(MODS)) {
    if (!bySlot.has(m.slot)) bySlot.set(m.slot, new Map());
    const lines = bySlot.get(m.slot)!;
    if (!lines.has(m.line)) lines.set(m.line, []);
    lines.get(m.line)!.push(m);
  }
  return MOD_SLOTS.filter((s) => bySlot.has(s)).map((slot) => ({
    slot,
    lines: [...bySlot.get(slot)!.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([line, mods]) => ({ line, mods: mods.sort((a, b) => a.tier - b.tier) })),
  }));
}

/** Why a mod cannot be installed on a car right now (null = it can). */
export function modBlocker(
  mod: ModDef,
  installed: string[],
  carClass: VehicleClass,
  mode: DamageMode,
): string | null {
  if (mod.class_whitelist?.length && !mod.class_whitelist.includes(carClass)) {
    return `Not for ${carClass} cars`;
  }
  if (mod.modes?.length && !mod.modes.includes(mode)) return `${mode} mode only: ${mod.modes.join(", ")}`;
  for (const req of mod.requires ?? []) {
    const r = MODS[req];
    // Satisfied by the same line at an equal or higher tier (wp_economy rule).
    const ok = installed.some((id) => {
      const m = MODS[id];
      return id === req || (r && m && m.line === r.line && m.slot === r.slot && m.tier >= r.tier);
    });
    if (!ok) return `Requires ${r ? r.name : req}`;
  }
  return null;
}
