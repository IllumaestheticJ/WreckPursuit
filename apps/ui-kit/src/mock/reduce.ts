/**
 * Pure event → UI-state reduction used by the mock provider (and reusable by the real one).
 *
 * `applyEvent` folds one `events.jsonl` line into a [`Playback`] state. Everything here is
 * deterministic and side-effect free so it can be unit-tested and so seeking is just
 * "reset, then fold every line up to the tick".
 *
 * Police, economy and race events are the contract's typed variants (v0.3.0):
 * `SimEvent::Police(PoliceEvent)`, `SimEvent::Economy(EconomyEvent)`, `SimEvent::Race(RaceEvent)`.
 */
import type { BodyRegion, Component, EconomyEvent, SimEvent, WantedLevel } from "../generated/contracts";
import { type PoliceEvent, type UnitKind } from "../model/police";
import type { DeployedTool, HeatHud, Incident, ReplayMarker, ScannerEntry, Vec2 } from "../model/types";
import { headingAt, rowAt, type EventLine, type Run } from "./runs";

/** A police unit as the event stream describes it. */
export interface EventUnit {
  unit: number;
  kind: UnitKind;
  /** Position when spawned (the stream has no unit positions after that). */
  pos: Vec2;
  headingRad: number;
  disabled: boolean;
}

/** Everything the event stream drives. */
export interface Playback {
  /** Last applied line index + 1. */
  cursor: number;
  /** Tick the state was last advanced to. */
  tick: number;
  regions: Partial<Record<BodyRegion, number>>;
  detached: BodyRegion[];
  components: Partial<Record<Component, number>>;
  failed: Component[];
  wheelsDetached: number[];
  totalLoss: boolean;
  /** Player's heat as far as events tell (level + state; heat/cooldown come from captain views). */
  heat: HeatHud;
  /** Units currently pursuing the player (from `PursuitStarted`; cleared by `PursuitEnded`). */
  pursuing: number[];
  /** Units that exist (spawned and not despawned). */
  units: EventUnit[];
  incidents: Incident[];
  tools: DeployedTool[];
  /** Per-entity wanted level. */
  levels: Record<number, WantedLevel>;
  /** Racers currently in a pursuit. */
  pursued: number[];
  scanner: ScannerEntry[];
  economy: EconomyEvent[];
  nextId: number;
}

/** Context the reducer needs from outside the event. */
export interface ReduceContext {
  run: Run;
  /** `EntityId` of the local player's car. */
  player: number;
  /** Body size `[w, l, h]` per entity (for contact → region), m. */
  bodySize: (entity: number) => [number, number, number];
  /** Display name per entity. */
  nameOf: (entity: number) => string;
  /**
   * Feed every racer's police events (a police captain's view). Default (unset): only the local
   * player's — violations, pursuits, spikes, PITs and wanted levels of AI racers elsewhere on the
   * map are not the driver's business (owner, 2026-10-23). State (levels, pursued list, units,
   * tools) is tracked for everyone either way.
   */
  allRacers?: boolean;
}

/** Empty playback (pristine car, no heat). */
export function emptyPlayback(): Playback {
  return {
    cursor: 0,
    tick: 0,
    regions: {},
    detached: [],
    components: {},
    failed: [],
    wheelsDetached: [],
    totalLoss: false,
    heat: { level: "None", heat: 0, state: "clear", cooldown: 0, unitsInPursuit: 0, bounty: 0 },
    pursuing: [],
    units: [],
    incidents: [],
    tools: [],
    levels: {},
    pursued: [],
    scanner: [],
    economy: [],
    nextId: 1,
  };
}

/** The variant name of an externally tagged event. */
export function eventKind(e: SimEvent): string {
  return Object.keys(e)[0];
}

/** The economy event inside `SimEvent::Economy`, or null for other variants. */
export function decodeEconomy(e: SimEvent): EconomyEvent | null {
  return "Economy" in e ? e.Economy : null;
}

/** World position of an entity at a tick, from metrics (null if the entity has no metrics). */
export function entityPose(run: Run, entity: number, tick: number): { pos: Vec2; heading: number; speed: number } | null {
  const ent = run.entities.find((e) => e.entity === entity);
  const rows = ent ? run.metrics[ent.id] : undefined;
  if (!rows || rows.length === 0) return null;
  const i = Math.max(0, rowAt(rows, tick));
  return { pos: [rows[i].x, rows[i].y], heading: headingAt(rows, i), speed: rows[i].speedMps };
}

/**
 * Body region hit by a contact at chassis-local `(x right, y forward)`, given body width and
 * length. Used only for runs whose stream has no `RegionDamaged` (the sim does not emit it yet).
 */
export function regionAt(localX: number, localY: number, width: number, length: number): BodyRegion {
  const u = localY / (length / 2);
  const s = localX / (width / 2);
  const left = s < 0;
  if (u > 0.55) return Math.abs(s) < 0.55 ? "FrontBumper" : left ? "FrontLeftFender" : "FrontRightFender";
  if (u < -0.55) return Math.abs(s) < 0.55 ? "RearBumper" : left ? "RearLeftQuarter" : "RearRightQuarter";
  if (left) return u >= 0 ? "LeftDoorF" : "LeftDoorR";
  return u >= 0 ? "RightDoorF" : "RightDoorR";
}

/** Regions that take a share of a hit on `r` (structure behind panels). */
export const NEIGHBOURS: Partial<Record<BodyRegion, BodyRegion[]>> = {
  FrontBumper: ["Hood", "FrameFront"],
  RearBumper: ["Trunk", "FrameRear"],
  FrontLeftFender: ["FrontBumper", "FrameLeft"],
  FrontRightFender: ["FrontBumper", "FrameRight"],
  RearLeftQuarter: ["RearBumper", "FrameLeft"],
  RearRightQuarter: ["RearBumper", "FrameRight"],
  LeftDoorF: ["FrameLeft", "Cabin"],
  LeftDoorR: ["FrameLeft", "Cabin"],
  RightDoorF: ["FrameRight", "Cabin"],
  RightDoorR: ["FrameRight", "Cabin"],
};

/** Contact energy (J) → severity 0..=1 (mock scale: 120 kJ = destroyed panel). */
export function contactSeverity(impulseNs: number, relSpeed: number): number {
  return Math.min(1, (0.5 * impulseNs * relSpeed) / 120_000);
}

/** Combine severities: `1 - (1-a)(1-b)` (two half-crushes do not make a whole). */
export function combineSeverity(a: number, b: number): number {
  return 1 - (1 - a) * (1 - b);
}

function hitRegion(p: Playback, region: BodyRegion, severity: number, spill = 0.4): void {
  p.regions = { ...p.regions, [region]: combineSeverity(p.regions[region] ?? 0, severity) };
  for (const n of NEIGHBOURS[region] ?? []) {
    p.regions[n] = combineSeverity(p.regions[n] ?? 0, severity * spill);
  }
}

const split = (s: string) => s.replace(/([a-z])([A-Z])/g, "$1 $2");

function say(p: Playback, line: EventLine, channel: ScannerEntry["channel"], severity: ScannerEntry["severity"], text: string, entity?: number): void {
  const e: ScannerEntry = { id: p.nextId++, tick: line.tick, tS: line.t_s, channel, severity, text, entity };
  if (channel === "unit" && isAirLine(text)) e.air = true;
  p.scanner = [...p.scanner, e];
}

/** A line spoken by a helicopter pilot: `wp_police` starts each with its call sign "Air <n>". */
export function isAirLine(text: string): boolean {
  return /^Air \d+\b/.test(text);
}

function incident(p: Playback, line: EventLine, kind: Incident["kind"], pos: Vec2, suspect: number | null): void {
  p.incidents = [...p.incidents, { id: p.nextId++, kind, pos, tS: line.t_s, suspect, unit: null }];
}

const WANTED_ORDER: WantedLevel[] = ["None", "L1", "L2", "L3", "L4", "L5"];

/** Radio callsign for a police unit, matching `wp_police`'s radio text ("Unit 7"). */
export function callsign(unit: number, kind?: UnitKind): string {
  return kind === "Helicopter" ? `Air ${unit}` : `Unit ${unit}`;
}

/** True when the stream reports region damage itself (then contacts are not turned into damage). */
const hasRegionEvents = new WeakMap<Run, boolean>();
function streamHasRegions(run: Run): boolean {
  let v = hasRegionEvents.get(run);
  if (v === undefined) {
    v = run.events.some((x) => eventKind(x.event) === "RegionDamaged");
    hasRegionEvents.set(run, v);
  }
  return v;
}

/** Fold one line into the playback (returns a new object; inputs are not mutated). */
export function applyEvent(prev: Playback, line: EventLine, ctx: ReduceContext): Playback {
  const p: Playback = { ...prev, cursor: prev.cursor + 1, tick: Math.max(prev.tick, line.tick) };
  const e = line.event;
  const isPlayer = (v: number) => v === ctx.player;
  /** Whether the feed shows this racer's violations / police events. */
  const feeds = (racer: number) => ctx.allRacers === true || racer === ctx.player;

  if ("Contact" in e) {
    const c = e.Contact;
    const severity = contactSeverity(c.impulse_ns, c.rel_speed);
    for (const v of [c.a, c.b]) {
      if (!isPlayer(v)) continue;
      const pose = entityPose(ctx.run, v, line.tick);
      if (pose && !streamHasRegions(ctx.run)) {
        const dx = c.point[0] - pose.pos[0];
        const dy = c.point[1] - pose.pos[1];
        const fwd = dx * Math.sin(pose.heading) + dy * Math.cos(pose.heading);
        const right = dx * Math.cos(pose.heading) - dy * Math.sin(pose.heading);
        const [w, l] = ctx.bodySize(v);
        hitRegion(p, regionAt(right, fwd, w, l), severity);
      }
      const other = v === c.a ? c.b : c.a;
      say(p, line, "system", severity > 0.5 ? "alert" : "warn", `Impact ${((0.5 * c.impulse_ns * c.rel_speed) / 1000).toFixed(0)} kJ — ${ctx.nameOf(other)}`, v);
    }
    if (severity > 0.3) incident(p, line, "Crash", [c.point[0], c.point[1]], c.a < 1000 ? c.a : null);
  } else if ("RegionDamaged" in e) {
    // Contract: severity is the region's current level 0..=1 (thresholded), so keep the max.
    const r = e.RegionDamaged;
    if (isPlayer(r.vehicle)) p.regions = { ...p.regions, [r.region]: Math.max(p.regions[r.region] ?? 0, r.severity) };
  } else if ("GlassShattered" in e) {
    const g = e.GlassShattered;
    if (isPlayer(g.vehicle)) say(p, line, "system", "warn", `Glass shattered — ${split(g.region)}`, g.vehicle);
  } else if ("ComponentDamaged" in e) {
    const c = e.ComponentDamaged;
    if (isPlayer(c.vehicle)) {
      p.components = { ...p.components, [c.component]: c.health };
      if (c.health < 0.5) say(p, line, "system", "warn", `${split(c.component)} at ${Math.round(c.health * 100)}%`, c.vehicle);
    }
  } else if ("ComponentFailed" in e) {
    const c = e.ComponentFailed;
    if (isPlayer(c.vehicle)) {
      p.components = { ...p.components, [c.component]: 0 };
      if (!p.failed.includes(c.component)) p.failed = [...p.failed, c.component];
      say(p, line, "system", "alert", `${split(c.component)} FAILED`, c.vehicle);
    }
  } else if ("PartDetached" in e) {
    const d = e.PartDetached;
    if (isPlayer(d.vehicle)) {
      // Group ids are lattice-specific; the debug lattices use 1 = front bumper, 2 = rear (UI-7).
      const region: BodyRegion = d.group === 2 ? "RearBumper" : d.group === 1 ? "FrontBumper" : "Hood";
      if (!p.detached.includes(region)) p.detached = [...p.detached, region];
      say(p, line, "system", "warn", `Part torn off (group ${d.group})`, d.vehicle);
    }
  } else if ("WheelDetached" in e) {
    const w = e.WheelDetached;
    if (isPlayer(w.vehicle)) {
      if (!p.wheelsDetached.includes(w.wheel)) p.wheelsDetached = [...p.wheelsDetached, w.wheel];
      say(p, line, "system", "alert", `Wheel lost — ${["FL", "FR", "RL", "RR"][w.wheel] ?? w.wheel}`, w.vehicle);
    }
  } else if ("TotalLoss" in e) {
    const t = e.TotalLoss;
    if (isPlayer(t.vehicle)) p.totalLoss = true;
    say(p, line, "system", "alert", `TOTAL LOSS — ${ctx.nameOf(t.vehicle)}`, t.vehicle);
  } else if ("TrafficViolation" in e) {
    const v = e.TrafficViolation;
    const vk = typeof v.kind === "string" ? v.kind : Object.keys(v.kind)[0];
    const over = typeof v.kind === "object" && "Speeding" in v.kind ? ` +${(v.kind.Speeding.over_mps * 3.6).toFixed(0)} km/h` : "";
    // Speeding is continuous: keep the feed readable by logging it at most every 5 s per racer.
    const lastSpeeding = [...p.scanner].reverse().find((s) => s.entity === v.vehicle && s.channel === "civil" && s.text.startsWith("Speeding"));
    if (feeds(v.vehicle) && (vk !== "Speeding" || !lastSpeeding || line.t_s - lastSpeeding.tS >= 5)) {
      say(p, line, "civil", "info", `${split(vk)}${over} — ${ctx.nameOf(v.vehicle)}`, v.vehicle);
      incident(p, line, vk as Incident["kind"], [v.pos[0], v.pos[1]], v.vehicle);
    }
  } else if ("Police" in e) {
    applyPolice(p, line, e.Police, ctx, feeds);
  } else if ("Economy" in e) {
    const d = e.Economy;
    {
      p.economy = [...p.economy, d];
      p.heat = { ...p.heat, bounty: p.heat.bounty + d.bounty_delta };
      const reason = typeof d.reason === "string" ? d.reason : Object.keys(d.reason)[0];
      const parts = [
        d.cash_delta && `cash ${d.cash_delta > 0 ? "+" : ""}${d.cash_delta}`,
        d.rep_delta && `rep ${d.rep_delta > 0 ? "+" : ""}${d.rep_delta}`,
        d.bounty_delta && `bounty ${d.bounty_delta > 0 ? "+" : ""}${d.bounty_delta}`,
      ].filter(Boolean);
      say(p, line, "system", "info", `${split(reason)}: ${parts.join(", ")}`);
    }
  }
  // LatticeDeformed and future variants: nothing to show.
  return p;
}

const OUTCOME_TEXT = {
  Escaped: "escaped",
  Safehouse: "made it to a safehouse",
  Captured: "BUSTED",
  CapturedTotalLoss: "BUSTED (car wrecked)",
  CalledOff: "pursuit called off",
} as const;

function applyPolice(p: Playback, line: EventLine, ev: PoliceEvent, ctx: ReduceContext, feeds: (racer: number) => boolean): void {
  const unitKind = (u: number) => p.units.find((x) => x.unit === u)?.kind;
  const name = (u: number) => callsign(u, unitKind(u));
  if ("Radio" in ev) {
    // wp_police radio text already names the unit ("Unit 3 responding …").
    say(p, line, ev.Radio.unit == null ? "dispatch" : "unit", "info", ev.Radio.text);
  } else if ("WantedLevelChanged" in ev) {
    const { racer, level } = ev.WantedLevelChanged;
    const old = p.levels[racer] ?? "None";
    p.levels = { ...p.levels, [racer]: level };
    if (racer === ctx.player) {
      p.heat = { ...p.heat, level, state: level === "None" ? "clear" : p.heat.state === "clear" ? "noticed" : p.heat.state };
    }
    // Only announce new highs: the level can flap at a threshold.
    const peak = p.scanner.some((s) => s.entity === racer && s.text.includes(`wanted level ${level.slice(1)}`));
    if (feeds(racer) && WANTED_ORDER.indexOf(level) > WANTED_ORDER.indexOf(old) && !peak) {
      say(p, line, "dispatch", "alert", `${ctx.nameOf(racer)} now wanted level ${level.slice(1)}`, racer);
    }
  } else if ("PursuitStarted" in ev) {
    const { racer } = ev.PursuitStarted;
    const unit = ev.PursuitStarted.unit ?? null;
    if (!p.pursued.includes(racer)) p.pursued = [...p.pursued, racer];
    if (unit !== null && !p.pursuing.includes(unit) && racer === ctx.player) p.pursuing = [...p.pursuing, unit];
    if (racer === ctx.player) p.heat = { ...p.heat, state: "pursuit", cooldown: 0, unitsInPursuit: Math.max(1, p.pursuing.length) };
    if (feeds(racer)) say(p, line, "dispatch", "alert", `Pursuit: ${unit === null ? "air unit" : name(unit)} on ${ctx.nameOf(racer)}`, racer);
  } else if ("PursuitEnded" in ev) {
    const { racer, outcome } = ev.PursuitEnded;
    p.pursued = p.pursued.filter((r) => r !== racer);
    if (racer === ctx.player) {
      p.pursuing = [];
      const caught = outcome === "Captured" || outcome === "CapturedTotalLoss";
      p.heat = { ...p.heat, state: caught ? "pursuit" : "clear", cooldown: caught ? 0 : 1, unitsInPursuit: 0 };
    }
    const caught = outcome === "Captured" || outcome === "CapturedTotalLoss";
    if (feeds(racer)) say(p, line, caught ? "dispatch" : "system", caught ? "alert" : "info", `${ctx.nameOf(racer)}: ${OUTCOME_TEXT[outcome]}`, racer);
  } else if ("UnitSpawned" in ev) {
    const u = ev.UnitSpawned;
    p.units = [
      ...p.units.filter((x) => x.unit !== u.unit),
      { unit: u.unit, kind: u.kind, pos: [u.pos[0], u.pos[1]], headingRad: Math.atan2(u.heading[0], u.heading[1]), disabled: false },
    ];
  } else if ("UnitDespawned" in ev) {
    const { unit } = ev.UnitDespawned;
    p.units = p.units.filter((x) => x.unit !== unit);
    p.pursuing = p.pursuing.filter((x) => x !== unit);
  } else if ("UnitDisabled" in ev) {
    const { unit, cause } = ev.UnitDisabled;
    p.units = p.units.map((x) => (x.unit === unit ? { ...x, disabled: true } : x));
    p.pursuing = p.pursuing.filter((x) => x !== unit);
    if (p.heat.state === "pursuit") p.heat = { ...p.heat, unitsInPursuit: p.pursuing.length };
    say(p, line, "unit", cause === "RacerContact" ? "warn" : "info", `${name(unit)} out of service (${split(cause).toLowerCase()})`);
  } else if ("RoadblockPlaced" in ev) {
    const t = ev.RoadblockPlaced;
    p.tools = [...p.tools, { id: t.tool, tool: "Roadblock", pos: [t.pos[0], t.pos[1]], remainingS: null }];
    if (t.racer === ctx.player) say(p, line, "dispatch", "warn", `Roadblock reported ahead (${t.sight_m.toFixed(0)} m sight line)`, t.racer);
  } else if ("SpikesPlaced" in ev) {
    const t = ev.SpikesPlaced;
    const a: Vec2 = [t.a[0], t.a[1]];
    const b: Vec2 = [t.b[0], t.b[1]];
    p.tools = [...p.tools, { id: t.tool, tool: "SpikeStrip", pos: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2], a, b, remainingS: null }];
  } else if ("ToolRemoved" in ev) {
    const { tool } = ev.ToolRemoved;
    p.tools = p.tools.filter((t) => t.id !== tool);
  } else if ("SpikeHit" in ev) {
    const { vehicle, wheels } = ev.SpikeHit;
    const n = wheels.filter(Boolean).length;
    if (feeds(vehicle)) say(p, line, "system", "alert", `Spikes! ${n} tyre${n === 1 ? "" : "s"} deflating — ${ctx.nameOf(vehicle)}`, vehicle);
  } else if ("HeloLaunched" in ev) {
    const { unit, target } = ev.HeloLaunched;
    p.units = p.units.map((x) => (x.unit === unit ? { ...x, kind: "Helicopter" } : x));
    if (feeds(target)) say(p, line, "dispatch", "alert", `Air unit ${unit} launched on ${ctx.nameOf(target)}`, target);
  } else if ("PitAttempt" in ev) {
    const { unit, target } = ev.PitAttempt;
    if (feeds(target)) say(p, line, "unit", "warn", `${name(unit)} attempting PIT on ${ctx.nameOf(target)}`, target);
  }
}

/** Timeline markers for a whole run. */
export function markersFor(run: Run, player: number): ReplayMarker[] {
  const out: ReplayMarker[] = [];
  for (const line of run.events) {
    const e = line.event;
    if ("Contact" in e && (e.Contact.a === player || e.Contact.b === player)) out.push({ tick: line.tick, kind: "contact", label: "Impact" });
    else if ("PartDetached" in e && e.PartDetached.vehicle === player) out.push({ tick: line.tick, kind: "detach", label: "Part detached" });
    else if ("ComponentFailed" in e && e.ComponentFailed.vehicle === player) out.push({ tick: line.tick, kind: "damage", label: `${split(e.ComponentFailed.component)} failed` });
    else if ("TotalLoss" in e) out.push({ tick: line.tick, kind: "totalLoss", label: "Total loss" });
    else if ("TrafficViolation" in e && e.TrafficViolation.vehicle === player && e.TrafficViolation.kind !== undefined && typeof e.TrafficViolation.kind === "string") {
      out.push({ tick: line.tick, kind: "violation", label: split(e.TrafficViolation.kind) });
    } else if ("Police" in e) {
      const d = e.Police;
      if ("PursuitStarted" in d) out.push({ tick: line.tick, kind: "police", label: "Pursuit started" });
      else if ("PursuitEnded" in d) out.push({ tick: line.tick, kind: "police", label: `Pursuit ended: ${d.PursuitEnded.outcome}` });
      else if ("SpikeHit" in d && d.SpikeHit.vehicle === player) out.push({ tick: line.tick, kind: "police", label: "Spike hit" });
      else if ("HeloLaunched" in d) out.push({ tick: line.tick, kind: "police", label: "Air unit" });
    }
  }
  // Collapse markers closer than 0.1 s of the same kind.
  return out.filter((m, i) => i === 0 || !(out[i - 1].kind === m.kind && m.tick - out[i - 1].tick < 24));
}
