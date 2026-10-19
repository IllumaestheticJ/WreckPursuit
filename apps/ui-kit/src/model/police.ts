/**
 * TypeScript mirror of `wp_police`'s public event / captain types (machine E, merged run 2).
 *
 * Source of truth: `crates/wp_police/src/events.rs` and `captain.rs`. The contract carries them
 * opaquely until v0.3.0 (`SimEvent::PoliceEvent { kind, payload }` with `payload` = JSON of
 * `PoliceEvent`; `ClientToServer::CaptainCommand(bytes)` with bytes = JSON of `CaptainCommand`).
 * When v0.3.0 types them in `wp_contracts`, `npm run gen:types` replaces this file.
 * Serde's externally tagged JSON is mirrored exactly (unit variants are plain strings).
 */
import type { DayPhase, EntityId, SimEvent, WantedLevel } from "../generated/contracts";

/** `wp_police::UnitId` (distinct from `EntityId`). */
export type UnitId = number;
/** `wp_police::ToolId`. */
export type ToolId = number;
/** `wp_police::IncidentId`. */
export type IncidentId = number;
/** `wp_police::graph::SegmentId`. */
export type SegmentId = number;

/** `wp_police::PursuitOutcome`. */
export type PursuitOutcome = "Escaped" | "Safehouse" | "Captured" | "CapturedTotalLoss" | "CalledOff";
/** `wp_police::DisableCause`. */
export type DisableCause = "SelfCrash" | "RacerContact" | "Damaged" | "BentWheel";
/** `wp_police::UnitKind`. */
export type UnitKind = "Car" | "Helicopter";
/** `wp_police::District`. */
export type District = "Core" | "Suburb" | "Industrial";
/** `wp_police::PursuitStatus`. */
export type PursuitStatus = "Clear" | "Wanted" | "Pursuit" | "Searching";

type V3 = [number, number, number];
type V2 = [number, number];

/** `wp_police::PoliceEvent`. */
export type PoliceEvent =
  | { Radio: { unit: UnitId | null; text: string } }
  | { WantedLevelChanged: { racer: EntityId; level: WantedLevel } }
  | { PursuitStarted: { racer: EntityId; unit: UnitId | null } }
  | { PursuitEnded: { racer: EntityId; outcome: PursuitOutcome } }
  | { UnitSpawned: { unit: UnitId; kind: UnitKind; pos: V3; heading: V2 } }
  | { UnitDespawned: { unit: UnitId } }
  | { UnitDisabled: { unit: UnitId; cause: DisableCause } }
  | { RoadblockPlaced: { tool: ToolId; racer: EntityId; pos: V3; sight_m: number } }
  | { SpikesPlaced: { tool: ToolId; racer: EntityId; a: V3; b: V3; sight_m: number } }
  | { ToolRemoved: { tool: ToolId } }
  | { SpikeHit: { vehicle: EntityId; wheels: [boolean, boolean, boolean, boolean] } }
  | { HeloLaunched: { unit: UnitId; target: EntityId } }
  | { PitAttempt: { unit: UnitId; target: EntityId } };

/** `wp_police::events::kind` discriminators. */
export const POLICE_KIND = {
  Radio: 1,
  WantedLevelChanged: 2,
  PursuitStarted: 3,
  PursuitEnded: 4,
  UnitSpawned: 5,
  UnitDespawned: 6,
  UnitDisabled: 7,
  RoadblockPlaced: 8,
  SpikesPlaced: 9,
  ToolRemoved: 10,
  SpikeHit: 11,
  HeloLaunched: 12,
  PitAttempt: 13,
} as const;

/** Decode `SimEvent::PoliceEvent` (null for other variants or a payload that does not parse). */
export function decodePoliceEvent(e: SimEvent): PoliceEvent | null {
  if (!("PoliceEvent" in e)) return null;
  try {
    return JSON.parse(new TextDecoder().decode(new Uint8Array(e.PoliceEvent.payload))) as PoliceEvent;
  } catch {
    return null;
  }
}

/** `wp_police::UnitOrder`. */
export type UnitOrder =
  | "Patrol"
  | { Pursue: { target: EntityId } }
  | { Intercept: { target: EntityId } }
  | { Respond: { incident: IncidentId } }
  | "Disengage";

/** `wp_police::CaptainCommand` (sent as JSON bytes in `ClientToServer::CaptainCommand`). */
export type CaptainCommand =
  | { AssignUnit: { unit: UnitId; order: UnitOrder } }
  | { RequestRoadblock: { target: EntityId; segment: SegmentId | null } }
  | { RequestSpikes: { target: EntityId; segment: SegmentId | null } }
  | { RequestHelicopter: { target: EntityId } }
  | { SetPriorityTarget: { target: EntityId | null } }
  | { CallOff: { target: EntityId } };

/** Encode for `ClientToServer::CaptainCommand` (mirrors `CaptainCommand::to_bytes`). */
export function captainCommandBytes(c: CaptainCommand): number[] {
  return [...new TextEncoder().encode(JSON.stringify(c))];
}

/** `wp_police::CaptainError`. */
export type CaptainError =
  | "UnknownUnit"
  | "UnknownTarget"
  | "UnknownIncident"
  | "NotPursued"
  | { Locked: { need_level: number } }
  | { Cooldown: { remaining_s: number } }
  | "Budget"
  | { Site: unknown }
  | "Unavailable"
  | "UnitUnavailable"
  | "Malformed";

/** Human text for a refused captain command. */
export function captainErrorText(e: CaptainError): string {
  if (typeof e === "string") {
    return {
      UnknownUnit: "No such unit",
      UnknownTarget: "No such racer",
      UnknownIncident: "No such incident",
      NotPursued: "That racer is not being pursued",
      Budget: "Too many tools / helicopters active",
      Unavailable: "Air unit unavailable tonight",
      UnitUnavailable: "That unit cannot take orders",
      Malformed: "Command did not decode",
    }[e];
  }
  if ("Locked" in e) return `Unlocks at wanted level ${e.Locked.need_level}`;
  if ("Cooldown" in e) return `Cooling down: ${e.Cooldown.remaining_s.toFixed(0)} s`;
  return "No valid site for that tool";
}

/** `wp_police::dispatch::IncidentKind`. */
export type IncidentKind = { Report: { racer: EntityId } } | "Crash";

/** `wp_police::Incident`. */
export interface PoliceIncident {
  id: IncidentId;
  kind: IncidentKind;
  pos: V2;
  district: District;
  created_s: number;
  dispatch_at_s: number;
  unit: UnitId | null;
  scene_until_s: number | null;
}

/** `wp_police::captain::UnitView`. */
export interface UnitView {
  unit: UnitId;
  kind: UnitKind;
  player_driven: boolean;
  pos: V2;
  heading: V2;
  speed_mps: number;
  health: number;
  risk: number;
  /** `Assignment::name()`: idle, patrol, respond, pursue, intercept, roadblock, spikes, helo-track, out-of-service. */
  assignment: string;
  target: EntityId | null;
  tactic: string | null;
  suggestion: UnitOrder | null;
}

/** `wp_police::captain::RacerSighting`. */
export interface RacerSighting {
  racer: EntityId;
  level: WantedLevel;
  /** Heat points 0..`max_heat` (100). */
  heat: number;
  status: PursuitStatus;
  last_known: V2 | null;
  staleness_s: number | null;
  bounty: number;
  units_assigned: number;
}

/** `wp_police::captain::ToolReadiness`. */
export interface ToolReadiness {
  roadblock_s: number;
  spikes_s: number;
  helo_s: number;
  roadblocks: [number, number];
  spikes: [number, number];
}

/** `wp_police::captain::ToolView`. */
export interface ToolView {
  tool: ToolId;
  roadblock: boolean;
  pos: V2;
  remaining_s: number;
}

/** `wp_police::CaptainView`. */
export interface CaptainView {
  time_s: number;
  phase: DayPhase;
  human_captain: boolean;
  priority_target: EntityId | null;
  units: UnitView[];
  incidents: PoliceIncident[];
  heat_by_district: [District, number][];
  racers: RacerSighting[];
  tools: ToolView[];
  readiness: ToolReadiness;
}

/** `data/police/heat.toml`: heat at which L1..L5 begin, and `max_heat`. */
export const LEVEL_THRESHOLDS = [10, 30, 50, 70, 90];
export const MAX_HEAT = 100;

/** `data/police/heat.toml` `[escape]`: seconds without line of sight to escape, L1..L5, × phase. */
export const ESCAPE_BASE_S = [12, 16, 22, 30, 40];
export const ESCAPE_PHASE_SCALE: Record<DayPhase, number> = { Dawn: 0.9, Day: 1.0, Dusk: 0.9, Night: 0.7 };

/** Escape time for a level and phase (mirror of `wp_police::heat::escape_time`). */
export function escapeTimeS(level: WantedLevel, phase: DayPhase): number {
  const i = Math.max(0, Math.min(4, (level === "None" ? 1 : Number(level.slice(1))) - 1));
  return ESCAPE_BASE_S[i] * ESCAPE_PHASE_SCALE[phase];
}

/** Heat as a 0..=1 fraction within the current level band. */
export function heatWithinLevel(heat: number, level: WantedLevel): number {
  const n = level === "None" ? 0 : Number(level.slice(1));
  const lo = n === 0 ? 0 : LEVEL_THRESHOLDS[n - 1];
  const hi = n >= 5 ? MAX_HEAT : LEVEL_THRESHOLDS[n];
  return Math.max(0, Math.min(1, (heat - lo) / Math.max(hi - lo, 1)));
}
