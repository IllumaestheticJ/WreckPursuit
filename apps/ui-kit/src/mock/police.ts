/**
 * A toy police "sim" for the captain map: units drive (Manhattan-style, along the grid) toward
 * their target; helicopters fly straight. Stand-in for `wp_police` until machine E lands.
 * Deterministic: no randomness, no wall clock (time comes in as `dtS`).
 */
import type { PoliceTool, PoliceUnitKind } from "../model/proposed";
import type { CityMap, Incident, PoliceUnit, Suspect, Vec2 } from "../model/types";
import { callsign } from "./reduce";

/** Cruise speed per unit kind, m/s. */
export const UNIT_SPEED: Record<PoliceUnitKind, number> = {
  Cruiser: 26,
  Interceptor: 34,
  Suv: 23,
  Helicopter: 48,
};

/** Starting units, parked around the police POIs of the map. */
export function initialUnits(map: CityMap): PoliceUnit[] {
  const stations = map.pois.filter((p) => p.kind === "PoliceStation");
  const at = (i: number): Vec2 => {
    const s = stations[i % Math.max(stations.length, 1)];
    return s ? [s.pos[0] + 12 * (i + 1), s.pos[1]] : [0, 0];
  };
  const mk = (id: number, kind: PoliceUnitKind, i: number, human = false): PoliceUnit => ({
    id,
    callsign: callsign(id),
    kind,
    status: "Patrol",
    pos: at(i),
    headingRad: 0,
    human,
    target: null,
    health: 1,
  });
  return [mk(10, "Cruiser", 0, true), mk(11, "Cruiser", 1), mk(12, "Interceptor", 2), mk(14, "Suv", 3)];
}

/** Starting tool charges for the captain. */
export function initialCharges(): Record<PoliceTool, number> {
  return { SpikeStrip: 3, Roadblock: 2, Helicopter: 1, Spotlight: 4, Pit: 2 };
}

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

/** Where a unit is heading, or null when it has no target. */
export function targetPos(u: PoliceUnit, suspects: Suspect[], incidents: Incident[]): Vec2 | null {
  const t = u.target;
  if (!t) return null;
  if (t.kind === "point") return t.pos;
  if (t.kind === "racer") return suspects.find((s) => s.id === t.id)?.pos ?? null;
  return incidents.find((i) => i.id === t.id)?.pos ?? null;
}

/** Advance every unit by `dtS` seconds. Returns new unit objects (inputs untouched). */
export function stepUnits(units: PoliceUnit[], suspects: Suspect[], incidents: Incident[], dtS: number): PoliceUnit[] {
  return units.map((u) => {
    if (u.status === "Disabled") return u;
    const tgt = targetPos(u, suspects, incidents);
    if (!tgt) return u.status === "Returning" ? { ...u, status: "Patrol" } : u;
    const step = UNIT_SPEED[u.kind] * dtS;
    const moved = u.kind === "Helicopter" ? moveStraight(u.pos, tgt, step) : moveManhattan(u.pos, tgt, step);
    const dist = Math.hypot(tgt[0] - moved.pos[0], tgt[1] - moved.pos[1]);
    let status = u.status;
    if (u.target?.kind === "racer") status = dist < 80 ? "Pursuit" : "Responding";
    else if (dist < 2) status = u.status === "Responding" ? "Roadblock" : u.status;
    else status = "Responding";
    return { ...u, pos: moved.pos, headingRad: moved.heading, status };
  });
}

/** Heat sources for the heat-map layer: recent incidents (decay over 60 s) + suspects by level. */
export function heatSources(incidents: Incident[], suspects: Suspect[], nowS: number): { pos: Vec2; weight: number }[] {
  const out: { pos: Vec2; weight: number }[] = [];
  for (const i of incidents) {
    const age = nowS - i.tS;
    if (age < 0 || age > 60) continue;
    const base = i.kind === "Crash" ? 1 : i.kind === "Sighting" ? 0.6 : 0.45;
    out.push({ pos: i.pos, weight: base * (1 - age / 60) });
  }
  for (const s of suspects) {
    const lvl = s.level === "None" ? 0 : Number(s.level.slice(1));
    if (lvl > 0) out.push({ pos: s.pos, weight: Math.min(1, 0.3 + lvl * 0.15) });
  }
  return out;
}
