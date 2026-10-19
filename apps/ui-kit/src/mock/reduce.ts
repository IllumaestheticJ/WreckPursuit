/**
 * Pure event → UI-state reduction used by the mock provider (and reusable by the real one).
 *
 * `applyEvent` folds one `events.jsonl` line into a [`Playback`] state. Everything here is
 * deterministic and side-effect free so it can be unit-tested and so seeking is just
 * "reset, then fold every line up to the tick".
 */
import type { BodyRegion, Component, SimEvent } from "../generated/contracts";
import type {
  EconomyEventMirror,
  PoliceEventPayload,
  PoliceTool,
  PoliceUnitKind,
  ProposedSimEvent,
  WantedLevel,
} from "../model/proposed";
import type { DeployedTool, HeatHud, Incident, ReplayMarker, ScannerEntry, Vec2 } from "../model/types";
import { headingAt, rowAt, type EventLine, type Run } from "./runs";

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
  heat: HeatHud;
  /** Cooldown bookkeeping (start tick, length in ticks). */
  cooldownStart: number | null;
  cooldownTicks: number;
  pursuing: number[];
  /** Units known to exist (dispatched), with kind. */
  dispatched: { unit: number; kind: PoliceUnitKind; target: Vec2 }[];
  incidents: Incident[];
  tools: DeployedTool[];
  /** Suspect entities currently visible to police. */
  visible: number[];
  /** Per-entity wanted level. */
  levels: Record<number, WantedLevel>;
  scanner: ScannerEntry[];
  economy: EconomyEventMirror[];
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
    cooldownStart: null,
    cooldownTicks: 0,
    pursuing: [],
    dispatched: [],
    incidents: [],
    tools: [],
    visible: [],
    levels: {},
    scanner: [],
    economy: [],
    nextId: 1,
  };
}

/** The variant name of an externally tagged event. */
export function eventKind(e: SimEvent | ProposedSimEvent): string {
  return Object.keys(e)[0];
}

/** Decode a `PoliceEvent`/`EconomyEvent` payload (UTF-8 JSON bytes, see proposed.ts). */
export function decodePayload<T>(payload: number[]): T | null {
  try {
    return JSON.parse(new TextDecoder().decode(new Uint8Array(payload))) as T;
  } catch {
    return null;
  }
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
 * length. Mock stand-in for `SimEvent::RegionDamaged` (v0.2.0).
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
  p.scanner = [...p.scanner, { id: p.nextId++, tick: line.tick, tS: line.t_s, channel, severity, text, entity }];
}

function incident(p: Playback, line: EventLine, kind: Incident["kind"], pos: Vec2, suspect: number | null): void {
  p.incidents = [...p.incidents, { id: p.nextId++, kind, pos, tS: line.t_s, suspect }];
}

const WANTED_ORDER: WantedLevel[] = ["None", "L1", "L2", "L3", "L4", "L5"];

const TOOL_LABEL: Record<PoliceTool, string> = {
  SpikeStrip: "spike strip",
  Roadblock: "roadblock",
  Helicopter: "air unit",
  Spotlight: "spotlight",
  Pit: "PIT manoeuvre",
};

/** Fold one line into the playback (returns a new object; inputs are not mutated). */
export function applyEvent(prev: Playback, line: EventLine, ctx: ReduceContext): Playback {
  const p: Playback = { ...prev, cursor: prev.cursor + 1, tick: Math.max(prev.tick, line.tick) };
  const e = line.event as Record<string, Record<string, unknown>>;
  const kind = eventKind(line.event);
  const body = e[kind] as never;
  const isPlayer = (v: number) => v === ctx.player;

  switch (kind) {
    case "Contact": {
      const c = body as Extract<SimEvent, { Contact: unknown }>["Contact"];
      const severity = contactSeverity(c.impulse_ns, c.rel_speed);
      for (const v of [c.a, c.b]) {
        if (!isPlayer(v)) continue;
        const pose = entityPose(ctx.run, v, line.tick);
        if (pose) {
          const dx = c.point[0] - pose.pos[0];
          const dy = c.point[1] - pose.pos[1];
          const fwd = dx * Math.sin(pose.heading) + dy * Math.cos(pose.heading);
          const right = dx * Math.cos(pose.heading) - dy * Math.sin(pose.heading);
          const [w, l] = ctx.bodySize(v);
          // The sim does not emit RegionDamaged yet; infer it unless the stream carries it.
          if (!ctx.run.events.some((x) => eventKind(x.event) === "RegionDamaged")) {
            hitRegion(p, regionAt(right, fwd, w, l), severity);
          }
        }
        const other = v === c.a ? c.b : c.a;
        say(p, line, "system", severity > 0.5 ? "alert" : "warn", `Impact ${(0.5 * c.impulse_ns * c.rel_speed / 1000).toFixed(0)} kJ — ${ctx.nameOf(other)}`, v);
      }
      if (severity > 0.3) incident(p, line, "Crash", [c.point[0], c.point[1]], c.a < 1000 ? c.a : null);
      break;
    }
    case "RegionDamaged": {
      const r = body as Extract<ProposedSimEvent, { RegionDamaged: unknown }>["RegionDamaged"];
      if (isPlayer(r.vehicle)) hitRegion(p, r.region, r.severity, 0);
      break;
    }
    case "GlassShattered": {
      const g = body as Extract<ProposedSimEvent, { GlassShattered: unknown }>["GlassShattered"];
      if (isPlayer(g.vehicle)) say(p, line, "system", "warn", `Glass shattered — ${split(g.region)}`, g.vehicle);
      break;
    }
    case "ComponentDamaged": {
      const c = body as Extract<SimEvent, { ComponentDamaged: unknown }>["ComponentDamaged"];
      if (!isPlayer(c.vehicle)) break;
      p.components = { ...p.components, [c.component]: c.health };
      if (c.health < 0.5) say(p, line, "system", "warn", `${split(c.component)} at ${Math.round(c.health * 100)}%`, c.vehicle);
      break;
    }
    case "ComponentFailed": {
      const c = body as Extract<SimEvent, { ComponentFailed: unknown }>["ComponentFailed"];
      if (!isPlayer(c.vehicle)) break;
      p.components = { ...p.components, [c.component]: 0 };
      if (!p.failed.includes(c.component)) p.failed = [...p.failed, c.component];
      say(p, line, "system", "alert", `${split(c.component)} FAILED`, c.vehicle);
      break;
    }
    case "PartDetached": {
      const d = body as Extract<SimEvent, { PartDetached: unknown }>["PartDetached"];
      if (!isPlayer(d.vehicle)) break;
      // Group ids are lattice-specific; the debug lattices use 1 = front bumper, 2 = rear.
      const region: BodyRegion = d.group === 2 ? "RearBumper" : d.group === 1 ? "FrontBumper" : "Hood";
      if (!p.detached.includes(region)) p.detached = [...p.detached, region];
      say(p, line, "system", "warn", `Part torn off (group ${d.group})`, d.vehicle);
      break;
    }
    case "WheelDetached": {
      const w = body as Extract<SimEvent, { WheelDetached: unknown }>["WheelDetached"];
      if (!isPlayer(w.vehicle)) break;
      if (!p.wheelsDetached.includes(w.wheel)) p.wheelsDetached = [...p.wheelsDetached, w.wheel];
      say(p, line, "system", "alert", `Wheel lost — ${["FL", "FR", "RL", "RR"][w.wheel] ?? w.wheel}`, w.vehicle);
      break;
    }
    case "TotalLoss": {
      const t = body as Extract<SimEvent, { TotalLoss: unknown }>["TotalLoss"];
      if (isPlayer(t.vehicle)) p.totalLoss = true;
      say(p, line, "system", "alert", `TOTAL LOSS — ${ctx.nameOf(t.vehicle)}`, t.vehicle);
      break;
    }
    case "TrafficViolation": {
      const v = body as Extract<SimEvent, { TrafficViolation: unknown }>["TrafficViolation"];
      const vk = typeof v.kind === "string" ? v.kind : Object.keys(v.kind)[0];
      const over = typeof v.kind === "object" && "Speeding" in v.kind ? ` +${(v.kind.Speeding.over_mps * 3.6).toFixed(0)} km/h` : "";
      say(p, line, "civil", "info", `${split(vk)}${over} — ${ctx.nameOf(v.vehicle)}`, v.vehicle);
      const pose = entityPose(ctx.run, v.vehicle, line.tick);
      const ik: Incident["kind"] = vk === "Collision" ? "Crash" : (vk as Incident["kind"]);
      if (pose) incident(p, line, ik, pose.pos, v.vehicle);
      break;
    }
    case "PoliceEvent": {
      const pe = body as { kind: number; payload: number[] };
      const decoded = decodePayload<PoliceEventPayload>(pe.payload);
      if (decoded) applyPolice(p, line, decoded, ctx);
      else say(p, line, "dispatch", "info", `Police event #${pe.kind}`);
      break;
    }
    case "EconomyEvent": {
      const ee = body as { kind: number; payload: number[] };
      const decoded = decodePayload<EconomyEventMirror>(ee.payload);
      if (!decoded) break;
      p.economy = [...p.economy, decoded];
      p.heat = { ...p.heat, bounty: p.heat.bounty + decoded.bounty_delta };
      const reason = typeof decoded.reason === "string" ? decoded.reason : Object.keys(decoded.reason)[0];
      const parts = [
        decoded.cash_delta && `cash ${decoded.cash_delta > 0 ? "+" : ""}${decoded.cash_delta}`,
        decoded.rep_delta && `rep ${decoded.rep_delta > 0 ? "+" : ""}${decoded.rep_delta}`,
        decoded.bounty_delta && `bounty ${decoded.bounty_delta > 0 ? "+" : ""}${decoded.bounty_delta}`,
      ].filter(Boolean);
      say(p, line, "system", "info", `${split(reason)}: ${parts.join(", ")}`);
      break;
    }
    default:
      // LatticeDeformed and future variants: nothing to show.
      break;
  }
  return p;
}

function applyPolice(p: Playback, line: EventLine, ev: PoliceEventPayload, ctx: ReduceContext): void {
  const kind = Object.keys(ev)[0] as keyof typeof ev;
  const unitName = (u: number | null) => (u === null ? "Dispatch" : callsign(u));
  switch (kind) {
    case "HeatChanged": {
      const h = (ev as Extract<PoliceEventPayload, { HeatChanged: unknown }>).HeatChanged;
      const old = p.levels[h.racer] ?? "None";
      p.levels = { ...p.levels, [h.racer]: h.level };
      if (h.racer === ctx.player) {
        p.heat = { ...p.heat, level: h.level, heat: h.heat };
        if (h.level === "None") p.heat.state = "clear";
        else if (p.heat.state === "clear") p.heat.state = "noticed";
      }
      if (WANTED_ORDER.indexOf(h.level) > WANTED_ORDER.indexOf(old)) {
        say(p, line, "dispatch", "alert", `${ctx.nameOf(h.racer)} now wanted level ${h.level.slice(1)}`, h.racer);
      }
      break;
    }
    case "Spotted": {
      const s = (ev as Extract<PoliceEventPayload, { Spotted: unknown }>).Spotted;
      if (!p.visible.includes(s.racer)) p.visible = [...p.visible, s.racer];
      if (s.racer === ctx.player) p.heat = { ...p.heat, state: p.heat.state === "clear" ? "noticed" : p.heat.state === "cooldown" ? "pursuit" : p.heat.state };
      incident(p, line, "Sighting", [s.pos[0], s.pos[1]], s.racer);
      say(p, line, "unit", "warn", `${unitName(s.unit)}: visual on ${ctx.nameOf(s.racer)}`, s.racer);
      break;
    }
    case "PursuitStarted": {
      const s = (ev as Extract<PoliceEventPayload, { PursuitStarted: unknown }>).PursuitStarted;
      if (!p.pursuing.includes(s.unit)) p.pursuing = [...p.pursuing, s.unit];
      if (!p.visible.includes(s.racer)) p.visible = [...p.visible, s.racer];
      if (s.racer === ctx.player) {
        p.heat = { ...p.heat, state: "pursuit", cooldown: 0, unitsInPursuit: p.pursuing.length };
        p.cooldownStart = null;
      }
      say(p, line, "dispatch", "alert", `Pursuit: ${unitName(s.unit)} on ${ctx.nameOf(s.racer)}`, s.racer);
      break;
    }
    case "LostVisual": {
      const s = (ev as Extract<PoliceEventPayload, { LostVisual: unknown }>).LostVisual;
      p.pursuing = p.pursuing.filter((u) => u !== s.unit);
      p.visible = p.visible.filter((r) => r !== s.racer);
      if (s.racer === ctx.player) p.heat = { ...p.heat, unitsInPursuit: p.pursuing.length };
      say(p, line, "unit", "info", `${unitName(s.unit)}: lost visual`, s.racer);
      break;
    }
    case "CooldownStarted": {
      const c = (ev as Extract<PoliceEventPayload, { CooldownStarted: unknown }>).CooldownStarted;
      p.pursuing = [];
      if (c.racer === ctx.player) {
        p.heat = { ...p.heat, state: "cooldown", cooldown: 0, unitsInPursuit: 0 };
        p.cooldownStart = line.tick;
        p.cooldownTicks = c.seconds * ctx.run.ticksPerSecond;
      }
      say(p, line, "dispatch", "info", `Searching for ${ctx.nameOf(c.racer)} (${c.seconds}s)`, c.racer);
      break;
    }
    case "Escaped": {
      const s = (ev as Extract<PoliceEventPayload, { Escaped: unknown }>).Escaped;
      p.visible = p.visible.filter((r) => r !== s.racer);
      if (s.racer === ctx.player) {
        p.heat = { ...p.heat, state: "clear", cooldown: 1, unitsInPursuit: 0 };
        p.cooldownStart = null;
      }
      say(p, line, "system", "info", `${ctx.nameOf(s.racer)} escaped`, s.racer);
      break;
    }
    case "Busted": {
      const s = (ev as Extract<PoliceEventPayload, { Busted: unknown }>).Busted;
      say(p, line, "dispatch", "alert", `${ctx.nameOf(s.racer)} BUSTED by ${unitName(s.unit)}`, s.racer);
      break;
    }
    case "UnitDispatched": {
      const d = (ev as Extract<PoliceEventPayload, { UnitDispatched: unknown }>).UnitDispatched;
      p.dispatched = [...p.dispatched.filter((x) => x.unit !== d.unit), { unit: d.unit, kind: d.kind, target: [d.target[0], d.target[1]] }];
      say(p, line, "dispatch", "info", `${callsign(d.unit, d.kind)} (${d.kind.toLowerCase()}) dispatched`);
      break;
    }
    case "UnitStatus":
      // Unit positions: the mock police sim moves units itself (see mock/police.ts).
      break;
    case "ToolDeployed": {
      const t = (ev as Extract<PoliceEventPayload, { ToolDeployed: unknown }>).ToolDeployed;
      p.tools = [...p.tools, { id: p.nextId++, tool: t.tool, pos: [t.pos[0], t.pos[1]], headingRad: 0 }];
      say(p, line, "unit", "warn", `${unitName(t.unit)}: ${TOOL_LABEL[t.tool]} deployed`);
      break;
    }
    case "Radio": {
      const r = (ev as Extract<PoliceEventPayload, { Radio: unknown }>).Radio;
      // Radio text is the unit speaking: prefix the callsign unless the line already starts with it.
      const name = unitName(r.unit);
      say(p, line, r.unit === null ? "dispatch" : "unit", "info", r.text.startsWith(name) ? r.text : `${name}: ${r.text}`);
      break;
    }
  }
}

const CALLSIGN_WORDS: Record<number, string> = { 10: "Adam", 11: "Mary", 12: "Lincoln", 13: "Air", 14: "Boy" };
const FALLBACK_WORDS = ["Charles", "David", "Edward", "Frank", "George", "Henry"];

/** Radio callsign for a police unit entity (mock naming; helicopters are always "Air"). */
export function callsign(unit: number, kind?: PoliceUnitKind): string {
  if (kind === "Helicopter") return `Air-${unit}`;
  return `${CALLSIGN_WORDS[unit] ?? FALLBACK_WORDS[unit % FALLBACK_WORDS.length]}-${unit}`;
}

/** Cooldown progress at `tick` (0..=1). */
export function cooldownAt(p: Playback, tick: number): number {
  if (p.heat.state !== "cooldown" || p.cooldownStart === null || p.cooldownTicks <= 0) return p.heat.cooldown;
  return Math.min(1, Math.max(0, (tick - p.cooldownStart) / p.cooldownTicks));
}

/** Timeline markers for a whole run. */
export function markersFor(run: Run, player: number): ReplayMarker[] {
  const out: ReplayMarker[] = [];
  for (const line of run.events) {
    const k = eventKind(line.event);
    const body = (line.event as Record<string, Record<string, unknown>>)[k];
    const concernsPlayer = body && (body.vehicle === player || body.a === player || body.b === player);
    if (k === "Contact" && concernsPlayer) out.push({ tick: line.tick, kind: "contact", label: "Impact" });
    else if (k === "PartDetached" && concernsPlayer) out.push({ tick: line.tick, kind: "detach", label: "Part detached" });
    else if (k === "ComponentFailed" && concernsPlayer) out.push({ tick: line.tick, kind: "damage", label: `${split(String(body.component))} failed` });
    else if (k === "TotalLoss") out.push({ tick: line.tick, kind: "totalLoss", label: "Total loss" });
    else if (k === "TrafficViolation" && concernsPlayer) out.push({ tick: line.tick, kind: "violation", label: "Violation" });
    else if (k === "PoliceEvent") {
      const d = decodePayload<PoliceEventPayload>((body as { payload: number[] }).payload);
      const pk = d ? Object.keys(d)[0] : "";
      if (["PursuitStarted", "Escaped", "Busted", "CooldownStarted"].includes(pk)) out.push({ tick: line.tick, kind: "police", label: split(pk) });
    }
  }
  // Collapse markers closer than 0.1 s of the same kind.
  return out.filter((m, i) => i === 0 || !(out[i - 1].kind === m.kind && m.tick - out[i - 1].tick < 24));
}
