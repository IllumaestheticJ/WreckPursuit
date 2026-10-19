/**
 * Types the UI needs that are NOT in `wp_contracts` v0.1.0 yet.
 *
 * Each one is either accepted in the v0.2.0 plan (`docs/CONTRACT_CHANGELOG.md`) or filed as a
 * request in `docs/integration/ui.md`. When the contract lands, `npm run gen:types` produces the
 * real type and the matching declaration here is deleted. Keep the shapes in sync with those
 * documents; do not invent fields the docs do not list.
 */
import type { BodyRegion, EntityId, VehicleClass } from "../generated/contracts";

/** v0.2.0 plan (B #1, #3): extra `SimEvent` variants. Not emitted by the sim yet. */
export type ProposedSimEvent =
  | { GlassShattered: { vehicle: EntityId; region: BodyRegion } }
  | { RegionDamaged: { vehicle: EntityId; region: BodyRegion; severity: number } };

/**
 * Wanted level, `wp_police::WantedLevel` (moves into contracts in v0.2.0, G #5).
 * Serialized as its variant name.
 */
export type WantedLevel = "None" | "L1" | "L2" | "L3" | "L4" | "L5";

/** Wanted level as a number 0..=5 (UI convenience). */
export function wantedLevelNumber(w: WantedLevel): number {
  return w === "None" ? 0 : Number(w.slice(1));
}

/**
 * Typed police events (v0.2.0 plan: "Typed `PoliceEvent`", designed with 06 — machine E has
 * not pushed yet). This is the UI's request (`docs/integration/ui.md` UI-1): the shapes below
 * are what the HUD, scanner feed and captain map need. Until the contract types it, the mock
 * provider decodes `SimEvent::PoliceEvent { kind, payload }` as `payload` = UTF-8 JSON of one of
 * these (the same convention `wp_economy` uses for `EconomyEvent`).
 */
export type PoliceEventPayload =
  | { HeatChanged: { racer: EntityId; heat: number; level: WantedLevel } }
  | { Spotted: { racer: EntityId; unit: EntityId; pos: [number, number, number] } }
  | { PursuitStarted: { racer: EntityId; unit: EntityId } }
  | { LostVisual: { racer: EntityId; unit: EntityId } }
  | { CooldownStarted: { racer: EntityId; seconds: number } }
  | { Escaped: { racer: EntityId } }
  | { Busted: { racer: EntityId; unit: EntityId } }
  | { UnitDispatched: { unit: EntityId; kind: PoliceUnitKind; target: [number, number, number] } }
  | { UnitStatus: { unit: EntityId; status: PoliceUnitStatus; pos: [number, number, number]; heading_rad: number } }
  | { ToolDeployed: { unit: EntityId; tool: PoliceTool; pos: [number, number, number] } }
  | { Radio: { unit: EntityId | null; text: string } };

/** Stable kind codes the mock uses for `SimEvent::PoliceEvent.kind` (proposal, see UI-1). */
export const POLICE_KIND: Record<keyof UnionToRecord<PoliceEventPayload>, number> = {
  HeatChanged: 1,
  Spotted: 2,
  PursuitStarted: 3,
  LostVisual: 4,
  CooldownStarted: 5,
  Escaped: 6,
  Busted: 7,
  UnitDispatched: 8,
  UnitStatus: 9,
  ToolDeployed: 10,
  Radio: 11,
};

/** Helper type: `{A: x} | {B: y}` → `{A: x, B: y}`. */
type UnionToRecord<U> = { [K in U extends unknown ? keyof U : never]: unknown };

/** Police unit kind (by `VehicleClass` plus the helicopter, which has no vehicle def yet). */
export type PoliceUnitKind = "Cruiser" | "Interceptor" | "Suv" | "Helicopter";

/** What a police unit is doing. */
export type PoliceUnitStatus = "Patrol" | "Responding" | "Pursuit" | "Roadblock" | "Disabled" | "Returning";

/** Captain / chase-unit tools. */
export type PoliceTool = "SpikeStrip" | "Roadblock" | "Helicopter" | "Spotlight" | "Pit";

/**
 * Captain command (UI-2). Replaces the opaque `ClientToServer::CaptainCommand(Vec<u8>)`.
 * Positions are world metres (x, y); `unit` is the police unit's `EntityId`.
 */
export type CaptainCommand =
  | { Assign: { unit: EntityId; target: CaptainTarget } }
  | { Deploy: { tool: PoliceTool; pos: [number, number]; heading_rad: number } }
  | { Recall: { unit: EntityId } }
  | { SetPriority: { racer: EntityId; priority: number } };

/** Target of a captain `Assign`. */
export type CaptainTarget =
  | { Pursue: { racer: EntityId } }
  | { Patrol: { center: [number, number]; radius_m: number } }
  | { Hold: { pos: [number, number] } }
  | { Incident: { id: number } };

/**
 * Mirror of `wp_economy::EconomyEvent` (as it is JSON-encoded inside the contract's opaque
 * `SimEvent::EconomyEvent.payload`). Typed in v0.2.0 (G #1); then this is replaced.
 * `reason` is the externally tagged `EventReason` (e.g. `"CleanRaceBonus"` or
 * `{ "RacePurse": { "race": "sprint", "placement": 1 } }`).
 */
export interface EconomyEventMirror {
  player: number;
  seq: number;
  day: number;
  reason: string | Record<string, Record<string, unknown>>;
  car: number | null;
  cash_delta: number;
  rep_delta: number;
  bounty_delta: number;
  cop_xp_delta: number;
  cash_after: number;
}

/** Police vehicle classes (subset of `VehicleClass`). */
export const POLICE_CLASSES: VehicleClass[] = ["PoliceCruiser", "PoliceInterceptor", "PoliceSuv"];
