/**
 * Toy police for runs that have no recorded `wp_police` data (simcli crash runs, the synthetic
 * script). Units use `wp_police`'s assignment names and the captain commands follow its rules
 * (`captain.rs`: NotPursued, UnitUnavailable, Locked, Cooldown, Budget) with the numbers from
 * `data/police/{tools,presence}.toml`, so the captain screen behaves the same on both sources.
 * Deterministic: no randomness, no wall clock (time comes in as `dtS`).
 */
import type { DayPhase } from "../generated/contracts";
import type { CaptainCommand, CaptainError, ToolReadiness, UnitKind } from "../model/police";
import type { CityMap, DeployedTool, Incident, PoliceUnit, Suspect, Vec2 } from "../model/types";
import { callsign } from "./reduce";

/** Cruise speed per unit kind, m/s (helicopter = `tools.toml` `helicopter.speed_mps`). */
export const UNIT_SPEED: Record<UnitKind, number> = { Car: 28, Helicopter: 55 };

/** `presence.toml` `[unlocks]` and `tools.toml` numbers the toy enforces. */
export const TOOL_RULES = {
  spikesLevel: 2,
  roadblockLevel: 3,
  roadblockMaxActive: 1,
  spikesMaxActive: 2,
  roadblockCooldownS: { Dawn: 60, Day: 40, Dusk: 60, Night: 90 } as Record<DayPhase, number>,
  spikesCooldownS: { Dawn: 30, Day: 20, Dusk: 30, Night: 45 } as Record<DayPhase, number>,
  heloCooldownS: 60,
  roadblockLifetimeS: 90,
  spikesLifetimeS: 60,
  spikesLengthM: 7,
};

/** Starting units, parked around the police POIs of the map (or the map centre). */
export function initialUnits(map: CityMap): PoliceUnit[] {
  const stations = map.pois.filter((p) => p.kind === "PoliceStation");
  const centre: Vec2 = [(map.boundsMin[0] + map.boundsMax[0]) / 2, (map.boundsMin[1] + map.boundsMax[1]) / 2];
  // Park units around the stations, 55 m apart so they read as separate icons.
  const at = (i: number): Vec2 => {
    const s = stations[i % Math.max(stations.length, 1)];
    const k = Math.floor(i / Math.max(stations.length, 1));
    const base = s ? s.pos : centre;
    return [base[0] + 55 * (k + 1) * (i % 2 ? -1 : 1), base[1] + 55 * k];
  };
  const mk = (id: number, i: number, human = false): PoliceUnit => ({
    id,
    callsign: callsign(id),
    kind: "Car",
    assignment: "patrol",
    pos: at(i),
    headingRad: 0,
    speedMps: 0,
    human,
    target: null,
    incident: null,
    tactic: null,
    suggestion: null,
    health: 1,
    pendingOrder: null,
  });
  return [mk(1, 0, true), mk(2, 1), mk(3, 2), mk(4, 3)];
}

export { initialReadiness } from "../model/readiness";

function moveManhattan(pos: Vec2, target: Vec2, step: number): { pos: Vec2; heading: number } {
  const dx = target[0] - pos[0];
  const dy = target[1] - pos[1];
  // Drive the longer leg first: on a grid city this follows roads well enough for a mock.
  if (Math.abs(dx) > 1 && (Math.abs(dx) >= Math.abs(dy) || Math.abs(dy) <= 1)) {
    const s = Math.sign(dx) * Math.min(step, Math.abs(dx));
    return { pos: [pos[0] + s, pos[1]], heading: dx > 0 ? Math.PI / 2 : -Math.PI / 2 };
  }
  if (Math.abs(dy) > 1) {
    const s = Math.sign(dy) * Math.min(step, Math.abs(dy));
    return { pos: [pos[0], pos[1] + s], heading: dy > 0 ? 0 : Math.PI };
  }
  return { pos, heading: Math.atan2(dx, dy) };
}

function moveStraight(pos: Vec2, target: Vec2, step: number): { pos: Vec2; heading: number } {
  const dx = target[0] - pos[0];
  const dy = target[1] - pos[1];
  const d = Math.hypot(dx, dy);
  if (d < 1e-3) return { pos, heading: 0 };
  const k = Math.min(1, step / d);
  return { pos: [pos[0] + dx * k, pos[1] + dy * k], heading: Math.atan2(dx, dy) };
}

/** Where a unit is heading, or null when it has nowhere to go. */
export function targetPos(u: PoliceUnit, suspects: Suspect[], incidents: Incident[]): Vec2 | null {
  if (u.target !== null && ["pursue", "intercept", "helo-track"].includes(u.assignment)) {
    return suspects.find((s) => s.id === u.target)?.pos ?? null;
  }
  if (u.assignment === "respond" && u.incident !== null) return incidents.find((i) => i.id === u.incident)?.pos ?? null;
  if (u.assignment === "patrol" && u.patrolPoint) return u.patrolPoint;
  return null;
}

/** Advance every toy unit by `dtS` seconds. Returns new unit objects (inputs untouched). */
export function stepUnits(units: PoliceUnit[], suspects: Suspect[], incidents: Incident[], dtS: number): PoliceUnit[] {
  if (dtS <= 0) return units;
  return units.map((u) => {
    if (u.assignment === "out-of-service") return u;
    const tgt = targetPos(u, suspects, incidents);
    if (!tgt) return u.speedMps === 0 ? u : { ...u, speedMps: 0 };
    const step = UNIT_SPEED[u.kind] * dtS;
    const moved = u.kind === "Helicopter" ? moveStraight(u.pos, tgt, step) : moveManhattan(u.pos, tgt, step);
    const arrived = Math.hypot(tgt[0] - moved.pos[0], tgt[1] - moved.pos[1]) < 2;
    const assignment = arrived && u.assignment === "patrol" ? "idle" : u.assignment;
    return { ...u, pos: moved.pos, headingRad: moved.heading, speedMps: arrived ? 0 : UNIT_SPEED[u.kind], assignment };
  });
}

/** Heat sources for the heat layer: recent incidents (decay over 60 s) + suspects by level. */
export function heatSources(incidents: Incident[], suspects: Suspect[], nowS: number): { pos: Vec2; weight: number }[] {
  const out: { pos: Vec2; weight: number }[] = [];
  for (const i of incidents) {
    const age = nowS - i.tS;
    if (age < 0 || age > 60) continue;
    const base = i.kind === "Crash" ? 1 : i.kind === "Report" || i.kind === "Sighting" ? 0.6 : 0.3;
    out.push({ pos: i.pos, weight: base * (1 - age / 60) });
  }
  for (const s of suspects) {
    const lvl = s.level === "None" ? 0 : Number(s.level.slice(1));
    if (lvl > 0) out.push({ pos: s.pos, weight: Math.min(1, 0.3 + lvl * 0.15) });
  }
  return out;
}

/** Toy captain state the command handler reads and writes. */
export interface ToyCaptain {
  units: PoliceUnit[];
  tools: DeployedTool[];
  readiness: ToolReadiness;
  priorityTarget: number | null;
  nextToolId: number;
}

/** Context for validating a command. */
export interface CommandContext {
  suspects: Suspect[];
  incidents: Incident[];
  map: CityMap;
  phase: DayPhase;
  /** Racers currently in a pursuit. */
  pursued: number[];
}

/** Nearest point on a road polyline to `p`. */
function nearestOnRoad(map: CityMap, roadId: number | null, p: Vec2): Vec2 {
  const roads = roadId === null ? map.roads : map.roads.filter((r) => r.id === roadId);
  let best: Vec2 = p;
  let bestD = Infinity;
  for (const r of roads) {
    for (let i = 0; i + 1 < r.points.length; i++) {
      const [a, b] = [r.points[i], r.points[i + 1]];
      const abx = b[0] - a[0];
      const aby = b[1] - a[1];
      const t = Math.max(0, Math.min(1, ((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / Math.max(abx * abx + aby * aby, 1e-6)));
      const q: Vec2 = [a[0] + abx * t, a[1] + aby * t];
      const d = Math.hypot(q[0] - p[0], q[1] - p[1]);
      if (d < bestD) {
        bestD = d;
        best = q;
      }
    }
  }
  return best;
}

/**
 * Apply a captain command to the toy police, with `wp_police`'s validation rules.
 * Returns the new state or a `CaptainError` (mirrors `PoliceSystem::captain_command`).
 */
export function toyCaptainCommand(s: ToyCaptain, cmd: CaptainCommand, ctx: CommandContext): ToyCaptain | CaptainError {
  const racer = (id: number) => ctx.suspects.find((x) => x.id === id);
  const pursued = (id: number) => ctx.pursued.includes(id) || racer(id)?.status === "Pursuit" || racer(id)?.status === "Searching";
  const level = (id: number) => {
    const l = racer(id)?.level ?? "None";
    return l === "None" ? 0 : Number(l.slice(1));
  };
  if ("AssignUnit" in cmd) {
    const { unit, order } = cmd.AssignUnit;
    const u = s.units.find((x) => x.id === unit);
    if (!u) return "UnknownUnit";
    if (u.assignment === "out-of-service" || u.kind !== "Car" || u.assignment === "roadblock" || u.assignment === "spikes") return "UnitUnavailable";
    let next: PoliceUnit;
    if (order === "Patrol") next = { ...u, assignment: "patrol", target: null, incident: null, tactic: null };
    else if (order === "Disengage") next = { ...u, assignment: "out-of-service", target: null, incident: null, tactic: null };
    else if ("Respond" in order) {
      if (!ctx.incidents.some((i) => i.id === order.Respond.incident)) return "UnknownIncident";
      next = { ...u, assignment: "respond", incident: order.Respond.incident, target: null, tactic: null };
    } else {
      const target = "Pursue" in order ? order.Pursue.target : order.Intercept.target;
      if (!racer(target)) return "UnknownTarget";
      if (!pursued(target)) return "NotPursued";
      next = { ...u, assignment: "Pursue" in order ? "pursue" : "intercept", target, incident: null, tactic: "Pursue" in order ? "shadow" : null };
    }
    return { ...s, units: s.units.map((x) => (x.id === unit ? next : x)) };
  }
  if ("RequestRoadblock" in cmd || "RequestSpikes" in cmd) {
    const isBlock = "RequestRoadblock" in cmd;
    const { target, segment } = isBlock ? cmd.RequestRoadblock : cmd.RequestSpikes;
    const r = racer(target);
    if (!r) return "UnknownTarget";
    if (!pursued(target)) return "NotPursued";
    const need = isBlock ? TOOL_RULES.roadblockLevel : TOOL_RULES.spikesLevel;
    if (level(target) < need) return { Locked: { need_level: need } };
    const cd = isBlock ? s.readiness.roadblock_s : s.readiness.spikes_s;
    if (cd > 0) return { Cooldown: { remaining_s: cd } };
    const [active, max] = isBlock ? s.readiness.roadblocks : s.readiness.spikes;
    if (active >= max) return "Budget";
    // Site: on the requested segment, else ~300 m ahead of the racer's last known position.
    const ahead: Vec2 = [r.pos[0] + 300 * Math.sin(r.headingRad), r.pos[1] + 300 * Math.cos(r.headingRad)];
    const pos = nearestOnRoad(ctx.map, segment, segment === null ? ahead : r.pos);
    const half = TOOL_RULES.spikesLengthM / 2;
    const tool: DeployedTool = isBlock
      ? { id: s.nextToolId, tool: "Roadblock", pos, remainingS: TOOL_RULES.roadblockLifetimeS }
      : { id: s.nextToolId, tool: "SpikeStrip", pos, a: [pos[0] - half, pos[1]], b: [pos[0] + half, pos[1]], remainingS: TOOL_RULES.spikesLifetimeS };
    const readiness: ToolReadiness = isBlock
      ? { ...s.readiness, roadblock_s: TOOL_RULES.roadblockCooldownS[ctx.phase], roadblocks: [active + 1, max] }
      : { ...s.readiness, spikes_s: TOOL_RULES.spikesCooldownS[ctx.phase], spikes: [active + 1, max] };
    return { ...s, tools: [...s.tools, tool], readiness, nextToolId: s.nextToolId + 1 };
  }
  if ("RequestHelicopter" in cmd) {
    const { target } = cmd.RequestHelicopter;
    if (!racer(target)) return "UnknownTarget";
    if (!pursued(target)) return "NotPursued";
    if (s.readiness.helo_s > 0) return { Cooldown: { remaining_s: s.readiness.helo_s } };
    if (s.units.some((u) => u.kind === "Helicopter" && u.assignment !== "out-of-service")) return "Budget";
    const id = Math.max(0, ...s.units.map((u) => u.id)) + 1;
    const c: Vec2 = [(ctx.map.boundsMin[0] + ctx.map.boundsMax[0]) / 2, (ctx.map.boundsMin[1] + ctx.map.boundsMax[1]) / 2];
    const helo: PoliceUnit = {
      id,
      callsign: callsign(id, "Helicopter"),
      kind: "Helicopter",
      assignment: "helo-track",
      pos: c,
      headingRad: 0,
      speedMps: UNIT_SPEED.Helicopter,
      human: false,
      target,
      incident: null,
      tactic: null,
      suggestion: null,
      health: 1,
      pendingOrder: null,
    };
    return { ...s, units: [...s.units, helo], readiness: { ...s.readiness, helo_s: TOOL_RULES.heloCooldownS } };
  }
  if ("SetPriorityTarget" in cmd) {
    const { target } = cmd.SetPriorityTarget;
    if (target !== null && !racer(target)) return "UnknownTarget";
    return { ...s, priorityTarget: target };
  }
  // CallOff
  const { target } = cmd.CallOff;
  if (!racer(target)) return "UnknownTarget";
  if (!pursued(target)) return "NotPursued";
  return {
    ...s,
    units: s.units.map((u) => (u.target === target ? { ...u, assignment: u.kind === "Helicopter" ? "out-of-service" : "patrol", target: null, tactic: null } : u)),
  };
}

/** Tick the toy readiness / tool lifetimes by `dtS`. */
export function tickTools(s: ToyCaptain, dtS: number): ToyCaptain {
  if (dtS <= 0) return s;
  const tools = s.tools
    .map((t) => (t.remainingS === null ? t : { ...t, remainingS: t.remainingS - dtS }))
    .filter((t) => t.remainingS === null || t.remainingS > 0);
  const count = (k: DeployedTool["tool"]) => tools.filter((t) => t.tool === k).length;
  return {
    ...s,
    tools,
    readiness: {
      roadblock_s: Math.max(0, s.readiness.roadblock_s - dtS),
      spikes_s: Math.max(0, s.readiness.spikes_s - dtS),
      helo_s: Math.max(0, s.readiness.helo_s - dtS),
      roadblocks: [count("Roadblock"), s.readiness.roadblocks[1]],
      spikes: [count("SpikeStrip"), s.readiness.spikes[1]],
    },
  };
}
