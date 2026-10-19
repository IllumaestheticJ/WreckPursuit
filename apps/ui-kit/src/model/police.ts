/**
 * Police types for the UI.
 *
 * Since contract v0.3.0 the event and captain types are typed in `wp_contracts::police`
 * (`SimEvent::Police(PoliceEvent)`, `ClientToServer::CaptainCommand(CaptainCommand)`) and come from
 * `../generated/contracts.ts` (`npm run gen:types`); they are re-exported here. What remains
 * hand-written mirrors `wp_police`'s read-only captain view (`CaptainView` and friends), which is
 * not in the contract (UI-3), plus the heat / escape tables from `data/police/heat.toml`.
 * Serde's externally tagged JSON is mirrored exactly (unit variants are plain strings).
 */
import type {
  CaptainCommand,
  CaptainError,
  ClientToServer,
  DayPhase,
  DisableCause,
  EntityId,
  IncidentId,
  PoliceEvent,
  PursuitOutcome,
  RoadEdgeId,
  SimEvent,
  SiteError,
  ToolId,
  UnitId,
  UnitKind,
  UnitOrder,
  WantedLevel,
} from "../generated/contracts";

export type { CaptainCommand, CaptainError, DisableCause, IncidentId, PoliceEvent, PursuitOutcome, RoadEdgeId, SiteError, ToolId, UnitId, UnitKind, UnitOrder };

/** `wp_police::District`. */
export type District = "Core" | "Suburb" | "Industrial";
/** `wp_police::PursuitStatus`. */
export type PursuitStatus = "Clear" | "Wanted" | "Pursuit" | "Searching";

type V2 = [number, number];

/** `wp_contracts::police_kind` discriminators (`PoliceEvent::kind()`). */
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

/** The police event inside `SimEvent::Police`, or null for other variants. */
export function decodePoliceEvent(e: SimEvent): PoliceEvent | null {
  return "Police" in e ? e.Police : null;
}

/** Wrap a captain command as the client message (`ClientToServer::CaptainCommand`). */
export function captainMessage(c: CaptainCommand): ClientToServer {
  return { CaptainCommand: c };
}

const SITE_TEXT: Record<SiteError, string> = {
  NoSite: "No valid site for that tool",
  NotOnPath: "That road is not on the racer's path",
  BlindCorner: "Too close to a blind corner",
  Highway: "Not allowed on the highway",
};

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
  return SITE_TEXT[e.Site];
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
