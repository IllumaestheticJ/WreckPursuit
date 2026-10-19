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
import type { DamageMode, RepairEstimate, Role } from "../generated/contracts";
import { bind, unbind } from "../model/bindings";
import type { UiCommand } from "../model/commands";
import { Store, type GameModel } from "../model/GameModel";
import type { CaptainCommand } from "../model/proposed";
import type { GarageCar, MapBlip, RaceHud, Suspect, UiState } from "../model/types";
import { CARS, MODS, modBlocker } from "../data/catalog";
import { carValue, impoundFee, insuranceQuote, marketPrice, modsPrice, quoteRepair, salePrice } from "../data/economy";
import { FIXTURES, loadFixture } from "./fixtures";
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
  presetOf,
} from "./mockData";
import { heatSources, initialCharges, initialUnits, stepUnits } from "./police";
import { applyEvent, callsign, cooldownAt, emptyPlayback, entityPose, markersFor, type Playback, type ReduceContext } from "./reduce";
import type { Run } from "./runs";

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

/** Create a mock `GameModel`. */
export function createMockModel(opts: MockOptions = {}): MockModel {
  const map = cityMap();
  let run = loadFixture(opts.run ?? "synthetic_pursuit");
  let player = run.entities[0]?.entity ?? 1;
  let playback: Playback = emptyPlayback();
  let tickF = 0; // fractional tick
  let noticeId = 1;
  const loop = opts.loop ?? true;

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
      return e >= 10 && e < 50 ? callsign(e) : `traffic #${e}`;
    },
  });

  const account = mockAccount();
  const garage = mockGarage();
  const settings = defaultSettings();
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
      timeOfDayS: 23 * 3600 + 14 * 60,
    },
    scanner: [],
    settings,
    account,
    garage: { cars: garage, activeCar: 1, shops: mockShops(), selectedShop: "garage_1" },
    market: { listings: mockMarket() },
    lobby: mockLobby(),
    results: mockResults(),
    impound: mockImpound(garage, account.day),
    crew: mockCrews(map),
    police: mockPoliceCareer(account.copXp),
    captain: { units: initialUnits(map), suspects: [], incidents: [], tools: [], heat: [], charges: initialCharges() },
    map,
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

  function suspectsAt(tick: number, prev: Suspect[]): Suspect[] {
    return run.entities.flatMap((e) => {
      const pose = entityPose(run, e.entity, tick);
      if (!pose) return [];
      const old = prev.find((s) => s.id === e.entity);
      return [
        {
          id: e.entity,
          name: `${CARS[e.car]?.display_name ?? e.car}`,
          pos: pose.pos,
          headingRad: pose.heading,
          speedMps: pose.speed,
          level: playback.levels[e.entity] ?? "None",
          visible: playback.visible.includes(e.entity),
          priority: old?.priority ?? 1,
        },
      ];
    });
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
      const suspects = suspectsAt(tick, s.captain.suspects);
      // Units that the event stream dispatched join the captain's roster.
      let units = s.captain.units;
      for (const d of playback.dispatched) {
        if (!units.some((u) => u.id === d.unit)) {
          units = [...units, { id: d.unit, callsign: callsign(d.unit), kind: d.kind, status: "Responding", pos: [d.target[0] + 300, d.target[1] - 300], headingRad: 0, human: false, target: { kind: "racer", id: player }, health: 1 }];
        }
      }
      // AI units in pursuit follow the player unless the captain gave them another target.
      units = units.map((u) => (playback.pursuing.includes(u.id) && !u.target ? { ...u, target: { kind: "racer", id: player } } : u));
      units = stepUnits(units, suspects, playback.incidents, dtS);
      const blips: MapBlip[] = [
        ...suspects.filter((x) => x.id !== player).map((x) => ({ id: x.id, kind: "racer" as const, pos: x.pos, headingRad: x.headingRad })),
        ...units.map((u) => ({ id: u.id, kind: u.kind === "Helicopter" ? ("helicopter" as const) : ("police" as const), pos: u.pos, headingRad: u.headingRad })),
      ];
      const race = raceHud(tick);
      if (race) blips.push({ id: -1, kind: "checkpoint", pos: race.nextCheckpoint });
      const nowS = tick / run.ticksPerSecond;
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
          heat: { ...playback.heat, cooldown: cooldownAt(playback, tick), bounty: s.account.bounty + playback.heat.bounty },
          pos: pose?.pos ?? s.hud.pos,
          headingRad: pose?.heading ?? s.hud.headingRad,
          blips,
          race,
          timeOfDayS: (23 * 3600 + 14 * 60 + nowS) % 86400,
        },
        scanner,
        police: s.police.orders === orders ? s.police : { ...s.police, orders },
        captain: {
          ...s.captain,
          units,
          suspects,
          incidents: playback.incidents,
          tools: [...playback.tools, ...s.captain.tools.filter((t) => t.id < 0)],
          heat: heatSources(playback.incidents, suspects, nowS),
        },
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
    set((s) => ({ ...s, scanner: [], captain: { ...s.captain, units: initialUnits(map), tools: [], charges: initialCharges() } }));
  }

  function loadRun(id: string): void {
    run = loadFixture(id);
    player = run.entities[0]?.entity ?? 1;
    tickF = 0;
    resetRunState();
    set((s) => ({
      ...s,
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

  function captain(cmd: CaptainCommand): void {
    const s = store.get();
    if ("Assign" in cmd) {
      const { unit, target } = cmd.Assign;
      const t =
        "Pursue" in target
          ? ({ kind: "racer", id: target.Pursue.racer } as const)
          : "Incident" in target
            ? ({ kind: "incident", id: target.Incident.id } as const)
            : "Hold" in target
              ? ({ kind: "point", pos: target.Hold.pos } as const)
              : ({ kind: "point", pos: target.Patrol.center } as const);
      set((st) => ({
        ...st,
        captain: { ...st.captain, units: st.captain.units.map((u) => (u.id === unit ? { ...u, target: t, status: "Responding" } : u)) },
        police: unit === 10 ? { ...st.police, assignment: describeTarget(t) } : st.police,
      }));
    } else if ("Deploy" in cmd) {
      const { tool, pos, heading_rad } = cmd.Deploy;
      if ((s.captain.charges[tool] ?? 0) <= 0) return notice(`No ${tool} charges left`);
      set((st) => ({
        ...st,
        captain: {
          ...st.captain,
          charges: { ...st.captain.charges, [tool]: st.captain.charges[tool] - 1 },
          tools: [...st.captain.tools, { id: -(st.captain.tools.length + 1), tool, pos, headingRad: heading_rad }],
        },
      }));
    } else if ("Recall" in cmd) {
      set((st) => ({ ...st, captain: { ...st.captain, units: st.captain.units.map((u) => (u.id === cmd.Recall.unit ? { ...u, target: null, status: "Returning" } : u)) } }));
    } else if ("SetPriority" in cmd) {
      const { racer, priority } = cmd.SetPriority;
      set((st) => ({ ...st, captain: { ...st.captain, suspects: st.captain.suspects.map((x) => (x.id === racer ? { ...x, priority } : x)) } }));
    }
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
          next.assistPreset = presetOf(next.assists);
          return { ...st, settings: next, hud: { ...st.hud, assists: next.assists } };
        });
      }
      case "settings.assistPreset": {
        if (cmd.preset === "custom") return;
        const assists = { ...ASSIST_PRESETS[cmd.preset] };
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
          sustainedWhileWanted: remaining.length > 0 && car.sustainedWhileWanted,
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
          sustainedWhileWanted: false,
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

      case "notice.dismiss":
        return set((st) => ({ ...st, notice: null }));
    }
  }

  function setMode(mode: DamageMode): void {
    set((st) => ({ ...st, settings: { ...st.settings, damageMode: mode }, hud: { ...st.hud, damageMode: mode }, lobby: { ...st.lobby, damageMode: mode } }));
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

function describeTarget(t: { kind: string; id?: number; pos?: [number, number] }): string {
  if (t.kind === "racer") return `Pursue suspect #${t.id}`;
  if (t.kind === "incident") return `Respond to incident #${t.id}`;
  return `Hold position ${t.pos?.map((v) => v.toFixed(0)).join(", ")}`;
}
