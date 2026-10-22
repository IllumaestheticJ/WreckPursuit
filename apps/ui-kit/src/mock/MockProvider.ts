/**
 * `createMockModel`: a complete `GameModel` with no game behind it.
 *
 * - Replays a simcli run (`events.jsonl` + `metrics.csv` + `summary.json`) to drive the HUD,
 *   scanner feed, captain map and replay controls. The replay transport *is* the replay screen.
 * - Applies every `UiCommand` against local mock economy / police state, refusing what the real
 *   economy would refuse (unaffordable, in debt, requirement missing...).
 *
 * Time: `clock: "raf"` advances with requestAnimationFrame (browser); `clock: "manual"` only
 * moves on `advance(seconds)` (tests, or a host that owns the frame loop). The sim tick is
 * derived from the replay position, never from the wall clock.
 */
import type { DamageMode, DayPhase, RepairEstimate, Role } from "../generated/contracts";
import { bind, unbind } from "../model/bindings";
import type { UiCommand } from "../model/commands";
import { Store, type GameModel } from "../model/GameModel";
import {
  captainErrorText,
  escapeTimeS,
  heatWithinLevel,
  type CaptainCommand,
  type CaptainView,
  type RacerSighting,
  type UnitOrder,
} from "../model/police";
import type { CaptainState, CityMap, DeployedTool, GarageCar, HeatHud, Incident, MapBlip, PoliceUnit, RaceHud, Suspect, UiState, Vec2 } from "../model/types";
import { CARS, MODS, modBlocker } from "../data/catalog";
import { carValue, impoundFee, insuranceQuote, marketPrice, modsPrice, quoteRepair, salePrice } from "../data/economy";
import { FIXTURES, loadFixture } from "./fixtures";
import { DEFAULT_BILL_REQUEST, mockBill } from "../live/bill";
import {
  ASSIST_PRESETS,
  cityMap,
  defaultSettings,
  mockAccount,
  mockCrews,
  mockGarage,
  mockImpound,
  mockLobby,
  mockMarket,
  mockPoliceCareer,
  mockResults,
  mockShops,
  PRESET_MODE,
  presetOf,
} from "./mockData";
import { heatSources, initialReadiness, initialUnits, stepUnits, tickTools, toyCaptainCommand, type ToyCaptain } from "./police";
import { applyEvent, callsign, emptyPlayback, entityPose, markersFor, type Playback, type ReduceContext } from "./reduce";
import { captainAt, type Run } from "./runs";

/** Options for [`createMockModel`]. */
export interface MockOptions {
  /** Fixture id (see `FIXTURES`), default `"synthetic_pursuit"`. */
  run?: string;
  /** Start playing immediately (default true). */
  autoplay?: boolean;
  /** Loop the run when it ends (default true). */
  loop?: boolean;
  /** `"raf"` (default in browsers) or `"manual"`. */
  clock?: "raf" | "manual";
  /** First screen (default `"hud"`). */
  screen?: UiState["screen"];
  /** Local role (default `"Racer"`). */
  role?: Role;
}

/** The mock model, with extra controls for demos and tests. */
export interface MockModel extends GameModel {
  /** Advance playback by `seconds` of wall time (scaled by replay speed). */
  advance(seconds: number): void;
  /** Stop the animation loop. */
  dispose(): void;
  /** The run being replayed. */
  run(): Run;
}

const SCANNER_KEEP = 200;
const START_CLOCK_S = 23 * 3600 + 14 * 60;

const STATUS_TO_STATE: Record<RacerSighting["status"], HeatHud["state"]> = {
  Clear: "clear",
  Wanted: "noticed",
  Pursuit: "pursuit",
  Searching: "cooldown",
};

/** Same unit order? (to clear a pending order once the recording shows it). */
function orderMatches(u: PoliceUnit, o: UnitOrder): boolean {
  if (o === "Patrol") return u.assignment === "patrol" || u.assignment === "idle";
  if (o === "Disengage") return u.assignment === "out-of-service";
  if ("Respond" in o) return u.assignment === "respond" && u.incident === o.Respond.incident;
  if ("Pursue" in o) return u.assignment === "pursue" && u.target === o.Pursue.target;
  return u.assignment === "intercept" && u.target === o.Intercept.target;
}

/** Centroid of each district's road midpoints (heat layer anchors on police maps). */
function districtCentroids(map: CityMap): Map<string, Vec2> {
  const acc = new Map<string, [number, number, number]>();
  for (const r of map.roads) {
    if (!r.district || r.points.length < 2) continue;
    const [a, b] = [r.points[0], r.points[r.points.length - 1]];
    const v = acc.get(r.district) ?? [0, 0, 0];
    acc.set(r.district, [v[0] + (a[0] + b[0]) / 2, v[1] + (a[1] + b[1]) / 2, v[2] + 1]);
  }
  return new Map([...acc].map(([k, [x, y, n]]) => [k, [x / n, y / n] as Vec2]));
}

/** Create a mock `GameModel`. */
export function createMockModel(opts: MockOptions = {}): MockModel {
  const defaultMap = cityMap();
  let run = loadFixture(opts.run ?? "synthetic_pursuit");
  let player = run.entities[0]?.entity ?? 1;
  let playback: Playback = emptyPlayback();
  let tickF = 0; // fractional tick
  let noticeId = 1;
  const loop = opts.loop ?? true;
  const mapOf = (r: Run) => r.map ?? defaultMap;
  let centroids = districtCentroids(mapOf(run));
  // Toy police (runs without recorded wp_police data).
  const freshToy = (): ToyCaptain => ({ units: initialUnits(mapOf(run)), tools: [], readiness: initialReadiness(), priorityTarget: null, nextToolId: 1000 });
  let toy = freshToy();
  // Recorded runs: orders the captain sent that the recording cannot reflect, and a local priority.
  let pending = new Map<number, UnitOrder>();
  let localPriority: number | null | undefined;

  const ctx = (): ReduceContext => ({
    run,
    player,
    bodySize: (e) => {
      const car = run.entities.find((x) => x.entity === e)?.car;
      const def = car ? CARS[car] : undefined;
      return (def?.body_size_m as [number, number, number] | undefined) ?? [1.8, 4.5, 1.4];
    },
    nameOf: (e) => {
      if (e === 4294967295) return "barrier";
      const ent = run.entities.find((x) => x.entity === e);
      if (ent) return `${CARS[ent.car]?.display_name ?? ent.car} (${ent.id})`;
      return `traffic #${e}`;
    },
  });

  const account = mockAccount();
  const garage = mockGarage();
  const settings = defaultSettings();
  const emptyCaptain = (): CaptainState => ({
    source: run.captain.length ? "wp_police" : "mock",
    units: run.captain.length ? [] : toy.units,
    suspects: [],
    incidents: [],
    tools: [],
    heat: [],
    heatByDistrict: [],
    readiness: initialReadiness(),
    priorityTarget: null,
  });
  const store = new Store<UiState>({
    screen: opts.screen ?? "hud",
    previousScreen: null,
    role: opts.role ?? "Racer",
    hud: {
      entity: player,
      speedMps: 0,
      engineRpm: 900,
      redlineRpm: CARS[run.entities[0]?.car]?.engine.redline_rpm ?? 7000,
      gear: 1,
      health: 1,
      regions: {},
      detached: [],
      components: {},
      failed: [],
      wheelsDetached: [],
      totalLoss: false,
      heat: emptyPlayback().heat,
      pos: [0, 0],
      headingRad: 0,
      blips: [],
      race: null,
      assists: settings.assists,
      damageMode: settings.damageMode,
      timeOfDayS: START_CLOCK_S,
    },
    scanner: [],
    settings,
    account,
    garage: { cars: garage, activeCar: 1, shops: mockShops(), selectedShop: "garage_1" },
    market: { listings: mockMarket() },
    lobby: mockLobby(),
    results: mockResults(),
    impound: mockImpound(garage, account.day),
    crew: mockCrews(defaultMap),
    police: mockPoliceCareer(account.copXp),
    captain: emptyCaptain(),
    map: mapOf(run),
    replay: {
      source: run.id,
      playing: opts.autoplay ?? true,
      speed: 1,
      tick: 0,
      totalTicks: run.totalTicks,
      ticksPerSecond: run.ticksPerSecond,
      markers: markersFor(run, player),
      camera: "chase",
    },
    bill: mockBill(garage[0], DEFAULT_BILL_REQUEST, settings.damageMode, account.cash),
    notice: null,
  });

  const set = (fn: (s: UiState) => UiState) => store.update(fn);
  const notice = (text: string, kind: "info" | "error" = "error") =>
    set((s) => ({ ...s, notice: { id: noticeId++, text, kind } }));

  // ------------------------------------------------------------ playback

  /** Fold events up to `tick` (resets on backward seeks). */
  function foldTo(tick: number): void {
    if (tick < playback.tick) playback = emptyPlayback();
    const c = ctx();
    while (playback.cursor < run.events.length && run.events[playback.cursor].tick <= tick) {
      playback = applyEvent(playback, run.events[playback.cursor], c);
    }
    playback = { ...playback, tick };
  }

  const carName = (entity: number) => {
    const e = run.entities.find((x) => x.entity === entity);
    return e ? (CARS[e.car]?.display_name ?? e.car) : `Racer #${entity}`;
  };

  /** Captain state from a recorded `CaptainView`, interpolated toward the next sample. */
  function recordedCaptain(tick: number, prev: CaptainState): { captain: CaptainState; sighting: RacerSighting | null; phase: DayPhase } {
    const i = Math.max(0, captainAt(run.captain, tick));
    const a = run.captain[i];
    const b = run.captain[i + 1];
    const f = b ? Math.max(0, Math.min(1, (tick - a.tick) / (b.tick - a.tick))) : 0;
    const view: CaptainView = a.view;
    const lerp = (p: Vec2, q: Vec2 | undefined): Vec2 => (q ? [p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f] : p);
    const units: PoliceUnit[] = view.units.map((u) => {
      const next = b?.view.units.find((x) => x.unit === u.unit);
      const unit: PoliceUnit = {
        id: u.unit,
        callsign: callsign(u.unit, u.kind),
        kind: u.kind,
        assignment: u.assignment,
        pos: lerp(u.pos, next?.pos),
        headingRad: Math.atan2(u.heading[0], u.heading[1]),
        speedMps: u.speed_mps,
        human: u.player_driven,
        target: u.target,
        incident: null,
        tactic: u.tactic,
        suggestion: u.suggestion,
        health: u.health,
        pendingOrder: null,
      };
      const p = pending.get(u.unit);
      if (p && orderMatches(unit, p)) pending.delete(u.unit);
      unit.pendingOrder = pending.get(u.unit) ?? null;
      return unit;
    });
    // Responding units: the incident list names the unit.
    for (const inc of view.incidents) {
      const u = units.find((x) => x.id === inc.unit);
      if (u && u.assignment === "respond") u.incident = inc.id;
    }
    const suspects: Suspect[] = view.racers.map((r) => {
      const next = b?.view.racers.find((x) => x.racer === r.racer);
      const pose = entityPose(run, r.racer, tick);
      const pos = r.last_known ? lerp(r.last_known, next?.last_known ?? undefined) : (pose?.pos ?? [0, 0]);
      return {
        id: r.racer,
        name: carName(r.racer),
        pos,
        headingRad: pose?.heading ?? 0,
        speedMps: pose?.speed ?? 0,
        level: r.level,
        heat: r.heat,
        status: r.status,
        stalenessS: r.staleness_s,
        bounty: r.bounty,
        unitsAssigned: r.units_assigned,
      };
    });
    const incidents: Incident[] = view.incidents.map((inc) => ({
      id: inc.id,
      kind: inc.kind === "Crash" ? "Crash" : "Report",
      pos: inc.pos,
      tS: inc.created_s,
      suspect: inc.kind === "Crash" ? null : inc.kind.Report.racer,
      unit: inc.unit,
    }));
    const tools: DeployedTool[] = view.tools.map((t) => {
      const known = playback.tools.find((x) => x.id === t.tool);
      return { id: t.tool, tool: t.roadblock ? "Roadblock" : "SpikeStrip", pos: t.pos, a: known?.a, b: known?.b, remainingS: t.remaining_s };
    });
    const heat = [
      ...view.heat_by_district.flatMap(([d, h]) => {
        const c = centroids.get(d);
        return c && h > 0 ? [{ pos: c, weight: Math.min(1, h / 100) }] : [];
      }),
      ...heatSources(incidents, [], view.time_s),
    ];
    const captain: CaptainState = {
      ...prev,
      source: "wp_police",
      units,
      suspects,
      incidents,
      tools,
      heat,
      heatByDistrict: view.heat_by_district,
      readiness: view.readiness,
      priorityTarget: localPriority !== undefined ? localPriority : view.priority_target,
    };
    return { captain, sighting: view.racers.find((r) => r.racer === player) ?? null, phase: view.phase };
  }

  /** Captain state from the toy police (non-police runs). */
  function toyCaptainState(tick: number, dtS: number, prev: CaptainState): CaptainState {
    const suspects: Suspect[] = run.entities.flatMap((e) => {
      const pose = entityPose(run, e.entity, tick);
      if (!pose) return [];
      const level = playback.levels[e.entity] ?? "None";
      const status = playback.pursued.includes(e.entity) ? "Pursuit" : level !== "None" ? "Wanted" : "Clear";
      return [{ id: e.entity, name: carName(e.entity), pos: pose.pos, headingRad: pose.heading, speedMps: pose.speed, level, heat: 0, status, stalenessS: 0, bounty: 0, unitsAssigned: 0 }];
    });
    // Units the event stream spawned join the roster; units chasing the player follow it.
    let units = toy.units;
    for (const u of playback.units) {
      if (!units.some((x) => x.id === u.unit)) {
        units = [...units, { id: u.unit, callsign: callsign(u.unit, u.kind), kind: u.kind, assignment: "patrol", pos: u.pos, headingRad: u.headingRad, speedMps: 0, human: false, target: null, incident: null, tactic: null, suggestion: null, health: 1, pendingOrder: null }];
      }
    }
    units = units
      .filter((u) => playback.units.some((x) => x.unit === u.id) || toy.units.some((x) => x.id === u.id))
      .map((u) => {
        const ev = playback.units.find((x) => x.unit === u.id);
        if (ev?.disabled) return { ...u, assignment: "out-of-service", target: null };
        if (playback.pursuing.includes(u.id) && u.assignment === "patrol") return { ...u, assignment: "pursue", target: player, tactic: "shadow" };
        return u;
      });
    toy = tickTools({ ...toy, units: stepUnits(units, suspects, playback.incidents, dtS) }, dtS);
    for (const s of suspects) s.unitsAssigned = toy.units.filter((u) => u.target === s.id).length;
    const nowS = tick / run.ticksPerSecond;
    return {
      ...prev,
      source: "mock",
      units: toy.units,
      suspects,
      incidents: playback.incidents.filter((i) => i.kind !== "Speeding" || nowS - i.tS < 30),
      tools: [...playback.tools, ...toy.tools],
      heat: heatSources(playback.incidents, suspects, nowS),
      heatByDistrict: [],
      readiness: toy.readiness,
      priorityTarget: toy.priorityTarget,
    };
  }

  function raceHud(tick: number): RaceHud | null {
    const racers = run.entities.filter((e) => run.metrics[e.id]?.length);
    if (!run.synthetic || racers.length < 2) return null;
    // Progress = distance driven (sum of speed × dt over samples up to tick).
    const progress = racers.map((e) => {
      const rows = run.metrics[e.id];
      let d = 0;
      for (let i = 1; i < rows.length && rows[i].tick <= tick; i++) d += rows[i].speedMps * (rows[i].tS - rows[i - 1].tS);
      return { e, d, speed: rows[Math.max(0, rows.findIndex((r) => r.tick > tick) - 1)]?.speedMps ?? 0 };
    });
    const sorted = [...progress].sort((a, b) => b.d - a.d);
    const me = progress.find((p) => p.e.entity === player)!;
    const pos = sorted.indexOf(me);
    const ahead = sorted[pos - 1];
    const behind = sorted[pos + 1];
    const cpLen = 400;
    const pose = entityPose(run, player, tick);
    return {
      position: pos + 1,
      of: sorted.length,
      lap: 1,
      laps: 1,
      checkpoint: Math.min(10, Math.floor(me.d / cpLen)),
      checkpoints: 10,
      gapAheadS: ahead ? (ahead.d - me.d) / Math.max(me.speed, 5) : null,
      gapBehindS: behind ? (me.d - behind.d) / Math.max(behind.speed, 5) : null,
      elapsedS: tick / run.ticksPerSecond,
      nextCheckpoint: pose ? [pose.pos[0] + 120 * Math.sin(pose.heading), pose.pos[1] + 120 * Math.cos(pose.heading)] : [0, 0],
    };
  }

  /** Recompute everything playback-driven for the current tick. */
  function render(dtS: number): void {
    const tick = Math.floor(tickF);
    foldTo(tick);
    set((s) => {
      const pose = entityPose(run, player, tick);
      const ent = run.entities.find((e) => e.entity === player);
      const rows = ent ? run.metrics[ent.id] : undefined;
      let row = rows?.[0];
      if (rows) for (const r of rows) if (r.tick <= tick) row = r; else break;
      const nowS = tick / run.ticksPerSecond;

      let captain: CaptainState;
      let heat: HeatHud = { ...playback.heat, bounty: s.account.bounty + playback.heat.bounty };
      if (run.captain.length) {
        const rec = recordedCaptain(tick, s.captain);
        captain = rec.captain;
        const sg = rec.sighting;
        if (sg) {
          const state = STATUS_TO_STATE[sg.status];
          heat = {
            level: sg.level,
            heat: heatWithinLevel(sg.heat, sg.level),
            state,
            cooldown: state === "cooldown" ? Math.min(1, (sg.staleness_s ?? 0) / escapeTimeS(sg.level, rec.phase)) : 0,
            unitsInPursuit: sg.units_assigned,
            bounty: s.account.bounty + Math.round(sg.bounty),
          };
        }
      } else {
        captain = toyCaptainState(tick, dtS, s.captain);
      }

      const blips: MapBlip[] = [
        ...captain.suspects.filter((x) => x.id !== player).map((x) => ({ id: x.id, kind: "racer" as const, pos: x.pos, headingRad: x.headingRad })),
        ...captain.units
          .filter((u) => u.assignment !== "out-of-service")
          .map((u) => ({ id: u.id, kind: u.kind === "Helicopter" ? ("helicopter" as const) : ("police" as const), pos: u.pos, headingRad: u.headingRad })),
      ];
      const race = raceHud(tick);
      if (race) blips.push({ id: -1, kind: "checkpoint", pos: race.nextCheckpoint });
      // playback.scanner only changes when events are folded or the run resets.
      const pb = playback.scanner;
      const same = s.scanner.length === Math.min(pb.length, SCANNER_KEEP) && s.scanner[s.scanner.length - 1]?.id === pb[pb.length - 1]?.id;
      const scanner = same ? s.scanner : pb.slice(-SCANNER_KEEP);
      const orders = scanner.filter((e) => e.channel === "dispatch" || e.channel === "unit").slice(-30);
      return {
        ...s,
        hud: {
          ...s.hud,
          entity: player,
          speedMps: row?.speedMps ?? 0,
          engineRpm: row?.engineRpm ?? 900,
          gear: row?.gear ?? 1,
          health: Math.min(row?.health ?? 1, 1),
          regions: playback.regions,
          detached: playback.detached,
          components: playback.components,
          failed: playback.failed,
          wheelsDetached: playback.wheelsDetached,
          totalLoss: playback.totalLoss,
          heat,
          pos: pose?.pos ?? s.hud.pos,
          headingRad: pose?.heading ?? s.hud.headingRad,
          blips,
          race,
          timeOfDayS: (START_CLOCK_S + nowS) % 86400,
        },
        scanner,
        police: s.police.orders === orders ? s.police : { ...s.police, orders },
        captain,
        replay: { ...s.replay, tick },
      };
    });
  }

  function advance(seconds: number): void {
    const r = store.get().replay;
    if (r.playing) {
      tickF += seconds * r.speed * run.ticksPerSecond;
      if (tickF >= run.totalTicks) {
        if (loop) {
          tickF = 0;
          resetRunState();
        } else {
          tickF = run.totalTicks;
          set((s) => ({ ...s, replay: { ...s.replay, playing: false } }));
        }
      }
    }
    render(r.playing ? seconds * r.speed : 0);
  }

  function resetRunState(): void {
    playback = emptyPlayback();
    toy = freshToy();
    pending = new Map();
    localPriority = undefined;
    set((s) => ({ ...s, scanner: [], captain: emptyCaptain() }));
  }

  function loadRun(id: string): void {
    run = loadFixture(id);
    player = run.entities[0]?.entity ?? 1;
    centroids = districtCentroids(mapOf(run));
    tickF = 0;
    resetRunState();
    set((s) => ({
      ...s,
      map: mapOf(run),
      hud: { ...s.hud, redlineRpm: CARS[run.entities[0]?.car]?.engine.redline_rpm ?? 7000 },
      replay: { ...s.replay, source: id, tick: 0, totalTicks: run.totalTicks, ticksPerSecond: run.ticksPerSecond, markers: markersFor(run, player) },
    }));
    render(0);
  }

  // ------------------------------------------------------------ commands

  const carOf = (s: UiState, uid: number) => s.garage.cars.find((c) => c.uid === uid);
  const wanted = (s: UiState) => s.hud.heat.level !== "None";
  const withCar = (s: UiState, car: GarageCar): UiState => ({
    ...s,
    garage: { ...s.garage, cars: s.garage.cars.map((c) => (c.uid === car.uid ? { ...car, value: carValue(car) } : c)) },
  });
  const spend = (s: UiState, amount: number): UiState => ({ ...s, account: { ...s.account, cash: s.account.cash - amount } });
  const canSpend = (s: UiState, amount: number): string | null =>
    s.account.cash < 0 ? "In debt: only free street meets until it is paid off" : amount > s.account.cash ? `Not enough cash (need $${amount.toLocaleString("en-US")})` : null;

  function healthAfter(remaining: RepairEstimate["lines"]): number {
    return Math.max(0.05, 1 - remaining.reduce((a, l) => a + l.severity * 0.12, 0));
  }

  /** Execute a captain command with wp_police's rules (toy runs) or validate + queue it (recorded runs). */
  function captain(cmd: CaptainCommand): void {
    const s = store.get();
    const phase: DayPhase = run.captain.length ? run.captain[Math.max(0, captainAt(run.captain, s.replay.tick))].view.phase : "Night";
    const cctx = { suspects: s.captain.suspects, incidents: s.captain.incidents, map: s.map, phase, pursued: playback.pursued };
    if (run.captain.length) {
      // Same validation against the recorded state; the recording itself cannot change.
      const probe: ToyCaptain = { units: s.captain.units, tools: s.captain.tools, readiness: s.captain.readiness, priorityTarget: s.captain.priorityTarget, nextToolId: 0 };
      const r = toyCaptainCommand(probe, cmd, cctx);
      if (typeof r === "string" || !("units" in r)) return notice(captainErrorText(r));
      if ("AssignUnit" in cmd) pending.set(cmd.AssignUnit.unit, cmd.AssignUnit.order);
      if ("SetPriorityTarget" in cmd) localPriority = cmd.SetPriorityTarget.target;
      notice(`Sent to wp_police: ${describeCommand(cmd)} (recorded run: units keep their recorded paths)`, "info");
      return render(0);
    }
    const r = toyCaptainCommand(toy, cmd, cctx);
    if (typeof r === "string" || !("units" in r)) return notice(captainErrorText(r));
    toy = r;
    if ("AssignUnit" in cmd && s.captain.units.find((u) => u.id === cmd.AssignUnit.unit)?.human) {
      set((st) => ({ ...st, police: { ...st.police, assignment: describeCommand(cmd) } }));
    }
    render(0);
  }

  function send(cmd: UiCommand): void {
    const s = store.get();
    switch (cmd.type) {
      case "navigate":
        if (cmd.screen === s.screen) return;
        return set((st) => ({ ...st, previousScreen: st.screen, screen: cmd.screen }));
      case "back":
        return set((st) => ({ ...st, screen: st.previousScreen ?? "mainMenu", previousScreen: null }));
      case "pause":
        return set((st) => ({ ...st, previousScreen: st.screen, screen: "pause", replay: { ...st.replay, playing: false } }));
      case "resume":
        return set((st) => ({ ...st, screen: "hud", previousScreen: null, replay: { ...st.replay, playing: true } }));
      case "quitToMenu":
        return set((st) => ({ ...st, screen: "mainMenu", previousScreen: null, replay: { ...st.replay, playing: false } }));

      case "settings.patch": {
        const p = cmd.patch;
        return set((st) => {
          const next = { ...st.settings } as UiState["settings"];
          for (const k of Object.keys(p) as (keyof typeof p)[]) {
            const v = p[k];
            const cur = next[k];
            (next as unknown as Record<string, unknown>)[k] = typeof cur === "object" && cur !== null ? { ...cur, ...(v as object) } : v;
          }
          next.assistPreset = presetOf(next.assists, next.damageMode);
          return { ...st, settings: next, hud: { ...st.hud, assists: next.assists } };
        });
      }
      case "settings.assistPreset": {
        if (cmd.preset === "custom") return;
        // One click: damage mode + assists together (GDD §6).
        const assists = { ...ASSIST_PRESETS[cmd.preset] };
        setMode(PRESET_MODE[cmd.preset]);
        return set((st) => ({ ...st, settings: { ...st.settings, assistPreset: cmd.preset, assists }, hud: { ...st.hud, assists } }));
      }
      case "settings.damageMode":
        return setMode(cmd.mode);
      case "settings.bind":
        return set((st) => ({ ...st, settings: { ...st.settings, bindings: bind(st.settings.bindings, cmd.action, cmd.device, cmd.code) } }));
      case "settings.unbind": {
        const next = unbind(s.settings.bindings, cmd.action, cmd.device);
        if (next === s.settings.bindings) return notice("That action must stay bound on the keyboard");
        return set((st) => ({ ...st, settings: { ...st.settings, bindings: next } }));
      }
      case "settings.resetBindings":
        return set((st) => ({ ...st, settings: { ...st.settings, bindings: defaultSettings().bindings } }));

      case "garage.setActive": {
        const car = carOf(s, cmd.car);
        if (!car) return;
        if (car.location.kind !== "Garage") return notice(`${car.name} is in the impound`);
        if (!car.drivable) return notice(`${car.name} is not drivable: repair or scrap it`);
        return set((st) => ({ ...st, garage: { ...st.garage, activeCar: cmd.car } }));
      }
      case "garage.selectShop":
        return set((st) => ({ ...st, garage: { ...st.garage, selectedShop: cmd.shop } }));
      case "garage.repair": {
        const car = carOf(s, cmd.car);
        const shop = s.garage.shops.find((x) => x.id === s.garage.selectedShop) ?? s.garage.shops[0];
        if (!car) return;
        const q = quoteRepair(car, cmd.choice, shop, s.settings.damageMode, s.account.cash, wanted(s));
        if (cmd.choice.kind === "DriveDamaged") {
          if (!car.drivable) return notice("Total loss: it cannot be driven");
          return notice(`${car.name} goes out as-is. It will drive like it looks.`, "info");
        }
        if (cmd.choice.kind === "Scrap") {
          return set((st) => ({
            ...st,
            account: { ...st.account, cash: st.account.cash + q.scrapValue },
            garage: { ...st.garage, cars: st.garage.cars.filter((c) => c.uid !== car.uid), activeCar: st.garage.activeCar === car.uid ? null : st.garage.activeCar },
            notice: { id: noticeId++, text: `Scrapped ${car.name} for $${q.scrapValue.toLocaleString("en-US")}`, kind: "info" },
          }));
        }
        const blocked = canSpend(s, q.total);
        if (blocked) return notice(blocked);
        const picked = cmd.choice.kind === "RepairAll" ? car.outstanding.lines.map((_, i) => i) : cmd.choice.lines;
        const remaining = car.outstanding.lines.filter((_, i) => !picked.includes(i));
        const repaired: GarageCar = {
          ...car,
          outstanding: { lines: remaining, total: remaining.reduce((a, l) => a + l.total, 0) },
          health: remaining.length ? healthAfter(remaining) : 1,
          totalLoss: car.totalLoss && remaining.some((l) => "Region" in l.item && (l.item.Region === "FrameFront" || l.item.Region === "Cabin")),
          sustainedBeforeCapture: remaining.length > 0 && car.sustainedBeforeCapture,
        };
        repaired.drivable = !repaired.totalLoss;
        const note = shop.kind === "legit" && wanted(s) && ["L2", "L3", "L4", "L5"].includes(s.hud.heat.level) ? " The shop reported you to the police." : "";
        return set((st) => ({ ...withCar(spend(st, q.total), repaired), notice: { id: noticeId++, text: `Repaired for $${q.total.toLocaleString("en-US")}.${note}`, kind: "info" } }));
      }
      case "garage.buyInsurance": {
        const car = carOf(s, cmd.car);
        if (!car) return;
        const ins = insuranceQuote(car, cmd.tier);
        const blocked = canSpend(s, ins.premiumPerDay);
        if (blocked) return notice(blocked);
        return set((st) => withCar(spend(st, ins.premiumPerDay), { ...car, insurance: ins }));
      }
      case "mods.install": {
        const car = carOf(s, cmd.car);
        const mod = MODS[cmd.mod];
        if (!car || !mod) return;
        if (car.mods.includes(mod.id)) return;
        const blocker = modBlocker(mod, car.mods, car.class, s.settings.damageMode);
        if (blocker) return notice(blocker);
        const replaced = car.mods.filter((id) => MODS[id]?.line === mod.line && MODS[id]?.slot === mod.slot);
        const refund = Math.round(0.4 * modsPrice(replaced));
        const cost = mod.price - refund;
        const blocked = canSpend(s, cost);
        if (blocked) return notice(blocked);
        const mods = [...car.mods.filter((id) => !replaced.includes(id)), mod.id].sort();
        return set((st) => withCar(spend(st, cost), { ...car, mods }));
      }
      case "mods.uninstall": {
        const car = carOf(s, cmd.car);
        const mod = MODS[cmd.mod];
        if (!car || !mod || !car.mods.includes(mod.id)) return;
        const rest = car.mods.filter((id) => id !== mod.id);
        const dependent = rest.find((id) => modBlocker(MODS[id], rest, car.class, s.settings.damageMode));
        if (dependent) return notice(`${MODS[dependent].name} needs it`);
        return set((st) => withCar(spend(st, -Math.round(0.4 * mod.price)), { ...car, mods: rest }));
      }
      case "market.buy": {
        const def = CARS[cmd.defId];
        if (!def) return;
        const price = marketPrice(def.base_price, def.class, cmd.condition);
        const blocked = canSpend(s, price);
        if (blocked) return notice(blocked);
        const uid = Math.max(0, ...s.garage.cars.map((c) => c.uid)) + 1;
        const health = { New: 1, Used: 0.9, Worn: 0.75 }[cmd.condition];
        const car: GarageCar = {
          uid,
          defId: def.id,
          name: def.display_name,
          class: def.class,
          basePrice: def.base_price,
          mods: [],
          health,
          totalLoss: false,
          drivable: true,
          location: { kind: "Garage" },
          insurance: null,
          outstanding: { lines: [], total: 0 },
          sustainedBeforeCapture: false,
          value: 0,
        };
        car.value = carValue(car);
        return set((st) => ({
          ...spend(st, price),
          garage: { ...st.garage, cars: [...st.garage.cars, car] },
          notice: { id: noticeId++, text: `Bought ${def.display_name} (${cmd.condition})`, kind: "info" },
        }));
      }
      case "market.sell": {
        const car = carOf(s, cmd.car);
        if (!car) return;
        if (car.location.kind !== "Garage") return notice("Retrieve it from the impound first");
        if (s.garage.cars.length === 1) return notice("You cannot sell your last car");
        const price = salePrice(car);
        return set((st) => ({
          ...spend(st, -price),
          garage: { ...st.garage, cars: st.garage.cars.filter((c) => c.uid !== car.uid), activeCar: st.garage.activeCar === car.uid ? null : st.garage.activeCar },
          notice: { id: noticeId++, text: `Sold ${car.name} for $${price.toLocaleString("en-US")}`, kind: "info" },
        }));
      }

      case "lobby.setRole": {
        const used = s.lobby.entrants.filter((e) => e.role === cmd.role && !e.isLocal).length;
        if (used >= s.lobby.roleSlots[cmd.role]) return notice(`No ${cmd.role} slot free`);
        return set((st) => ({
          ...st,
          role: cmd.role,
          lobby: {
            ...st.lobby,
            entrants: st.lobby.entrants.map((e) =>
              e.isLocal ? { ...e, role: cmd.role, car: cmd.role === "Racer" ? "shiro_turbo" : cmd.role === "PoliceChase" ? "sentinel_cruiser" : null, ready: false } : e,
            ),
          },
        }));
      }
      case "lobby.setReady": {
        if (cmd.ready && s.lobby.buyIn > 0 && s.role === "Racer") {
          const blocked = canSpend(s, s.lobby.buyIn);
          if (blocked) return notice(blocked);
        }
        return set((st) => {
          const entrants = st.lobby.entrants.map((e) => (e.isLocal ? { ...e, ready: cmd.ready } : e.role === "Racer" ? { ...e, ready: true } : e));
          return { ...st, lobby: { ...st.lobby, entrants, countdownS: entrants.every((e) => e.ready) ? 10 : null } };
        });
      }
      case "lobby.leave":
        return set((st) => ({ ...st, screen: "mainMenu" }));
      case "results.continue":
        return set((st) => ({ ...st, screen: "garage", previousScreen: null }));

      case "impound.retrieve": {
        const car = carOf(s, cmd.car);
        if (!car || car.location.kind !== "Impound") return;
        const fee = impoundFee(car.class, s.account.day - car.location.sinceDay);
        const blocked = canSpend(s, fee);
        if (blocked) return notice(blocked);
        const freed = { ...car, location: { kind: "Garage" as const } };
        return set((st) => {
          const next = withCar(spend(st, fee), freed);
          return { ...next, impound: mockImpound(next.garage.cars, st.account.day), notice: { id: noticeId++, text: `${car.name} released for $${fee}`, kind: "info" } };
        });
      }
      case "impound.payFines": {
        const owed = s.impound.finesOwed;
        if (owed <= 0) return;
        return set((st) => ({ ...spend(st, owed), impound: { ...st.impound, finesOwed: 0 } }));
      }

      case "captain":
        return captain(cmd.command);

      case "replay.play":
        if (tickF >= run.totalTicks) tickF = 0;
        return set((st) => ({ ...st, replay: { ...st.replay, playing: true } }));
      case "replay.pause":
        return set((st) => ({ ...st, replay: { ...st.replay, playing: false } }));
      case "replay.seek":
        tickF = Math.max(0, Math.min(cmd.tick, run.totalTicks));
        return render(0);
      case "replay.step":
        tickF = Math.max(0, Math.min(Math.floor(tickF) + cmd.ticks, run.totalTicks));
        set((st) => ({ ...st, replay: { ...st.replay, playing: false } }));
        return render(0);
      case "replay.speed":
        return set((st) => ({ ...st, replay: { ...st.replay, speed: cmd.speed } }));
      case "replay.camera":
        return set((st) => ({ ...st, replay: { ...st.replay, camera: cmd.camera } }));
      case "replay.load":
        if (!FIXTURES.some((f) => f.id === cmd.source)) return notice(`Unknown replay ${cmd.source}`);
        return loadRun(cmd.source);

      case "bill.request": {
        const car = s.garage.cars.find((c) => c.uid === s.garage.activeCar) ?? s.garage.cars[0];
        if (!car) return notice("No car to price");
        const request = { ...(s.bill?.request ?? DEFAULT_BILL_REQUEST), ...cmd.request };
        return set((st) => ({ ...st, bill: mockBill(car, request, st.settings.damageMode, st.account.cash) }));
      }
      case "notice.dismiss":
        return set((st) => ({ ...st, notice: null }));
    }
  }

  function setMode(mode: DamageMode): void {
    set((st) => ({
      ...st,
      settings: { ...st.settings, damageMode: mode, assistPreset: presetOf(st.settings.assists, mode) },
      hud: { ...st.hud, damageMode: mode },
      lobby: { ...st.lobby, damageMode: mode },
    }));
  }

  // ------------------------------------------------------------ clock

  let raf = 0;
  let last = 0;
  const useRaf = (opts.clock ?? (typeof requestAnimationFrame === "function" ? "raf" : "manual")) === "raf";
  if (useRaf) {
    const frame = (t: number) => {
      const dt = last ? Math.min((t - last) / 1000, 0.1) : 0;
      last = t;
      advance(dt);
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
  }
  render(0);

  return {
    get: () => store.get(),
    subscribe: (l) => store.subscribe(l),
    send,
    advance,
    dispose: () => cancelAnimationFrame?.(raf),
    run: () => run,
  };
}

/** Short text for a captain command (notices, police orders). */
export function describeCommand(c: CaptainCommand): string {
  if ("AssignUnit" in c) {
    const o = c.AssignUnit.order;
    const what = typeof o === "string" ? o.toLowerCase() : "Pursue" in o ? `pursue racer #${o.Pursue.target}` : "Intercept" in o ? `intercept racer #${o.Intercept.target}` : `respond to incident #${o.Respond.incident}`;
    return `Unit ${c.AssignUnit.unit} → ${what}`;
  }
  if ("RequestRoadblock" in c) return `roadblock on racer #${c.RequestRoadblock.target}${c.RequestRoadblock.road != null ? ` (road ${c.RequestRoadblock.road})` : ""}`;
  if ("RequestSpikes" in c) return `spikes on racer #${c.RequestSpikes.target}${c.RequestSpikes.road != null ? ` (road ${c.RequestSpikes.road})` : ""}`;
  if ("RequestHelicopter" in c) return `air unit on racer #${c.RequestHelicopter.target}`;
  if ("SetPriorityTarget" in c) return c.SetPriorityTarget.target === null ? "priority cleared" : `priority racer #${c.SetPriorityTarget.target}`;
  return `call off racer #${c.CallOff.target}`;
}
