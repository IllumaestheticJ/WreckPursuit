/**
 * `createLiveModel`: a `GameModel` fed by a running simulation instead of fixtures.
 *
 * The host (the browser testbed) pushes one [`LiveFrame`] per rendered frame with plain data it
 * already has: the drained `SimEvent`s, the player's vehicle readout and, when the sim exposes it,
 * the police readout for the player (`Sim::police()` → `RacerRecord`). The model folds events with
 * the same reducer the mock uses (`applyEvent`), so the HUD heat panel, pursuit status and police
 * radio behave identically on both sources. The repair bill is priced through a host callback
 * (`wp_wasm::repair_quote_json`).
 *
 * No wasm import here: the kit stays independent of `wp_wasm`; the host adapts.
 */
import type { DamageMode, DayPhase, SimEvent, WantedLevel } from "../generated/contracts";
import type { UiCommand } from "../model/commands";
import { emptyUiState } from "../model/defaults";
import { Store, type GameModel } from "../model/GameModel";
import { escapeTimeS, heatWithinLevel, type PursuitStatus } from "../model/police";
import type { BillRequest, HeatHud, MapBlip, UiState, Vec2 } from "../model/types";
import { applyEvent, emptyPlayback, type Playback, type ReduceContext } from "../mock/reduce";
import type { Run } from "../mock/runs";
import { billFromWasm, DEFAULT_BILL_REQUEST, type LiveBillJson } from "./bill";

/** The player's vehicle as the host reads it (`VehicleState` / telemetry). */
export interface LiveVehicle {
  speedMps: number;
  engineRpm: number;
  redlineRpm?: number;
  gear: number;
  health: number;
  pos: Vec2;
  headingRad: number;
}

/**
 * The player's police record (`wp_police::RacerRecord` via `Sim::police()`), when available.
 * Without it the model falls back to the event stream (level + pursuit start/end only).
 */
export interface PoliceReadout {
  level: WantedLevel;
  /** Heat points 0..100. */
  heat: number;
  status: PursuitStatus;
  bounty: number;
  /** Seconds since the police last saw the player (null = never). */
  stalenessS: number | null;
  /** Seconds since line of sight was lost: the escape timer while `Searching` (`RacerRecord::los_lost_s`). */
  searchS?: number;
  /** Escape time for the level and phase, s (null = compute from `phase`). */
  escapeS?: number | null;
  phase?: DayPhase;
  unitsAssigned: number;
}

/** One rendered frame of live data. */
export interface LiveFrame {
  /** Physics tick. */
  tick: number;
  ticksPerSecond?: number;
  /** `EntityId` of the player's car. */
  player: number;
  /** Events drained since the previous frame. */
  events: SimEvent[];
  vehicle?: LiveVehicle | null;
  police?: PoliceReadout | null;
  damageMode?: DamageMode;
  /** In-game clock, seconds since midnight. */
  timeOfDayS?: number;
  /** Chase radar (ChaseHud minimap): the player's pose and the blips around them. Null clears it;
   * the chase HUD shows the radar while it has blips. */
  radar?: { pos: Vec2; headingRad: number; blips: MapBlip[] } | null;
}

/** Host hooks. */
export interface LiveOptions {
  /** Price the player's car (`wp_wasm::repair_quote_json`). Null when there is no car. */
  quote?: (player: number, request: BillRequest) => LiveBillJson | null;
  /** The user closed an overlay (`back` command). */
  onBack?: () => void;
  /** Display name for an entity (scanner lines). */
  nameOf?: (entity: number) => string;
}

/** The live model. */
export interface LiveModel extends GameModel {
  /** Feed one frame. */
  push(frame: LiveFrame): void;
  /** Re-price the bill (e.g. when the overlay opens), defaulting the wanted level to the current one. */
  openBill(): void;
  /** Forget events and heat (new session / scenario). */
  reset(): void;
}

const SCANNER_KEEP = 200;

const STATUS_TO_STATE: Record<PursuitStatus, HeatHud["state"]> = {
  Clear: "clear",
  Wanted: "noticed",
  Pursuit: "pursuit",
  Searching: "cooldown",
};

/** A run with no fixture data, so the reducer infers nothing from metrics. */
function emptyRun(): Run {
  return { id: "live", label: "live", synthetic: false, entities: [], events: [], metrics: {}, totalTicks: 0, ticksPerSecond: 240, captain: [], map: null };
}

/** Create a live model. */
export function createLiveModel(opts: LiveOptions = {}): LiveModel {
  const store = new Store<UiState>(emptyUiState("chaseHud"));
  let playback: Playback = emptyPlayback();
  let player = 0;
  let noticeId = 1;
  const run = emptyRun();
  const ctx = (): ReduceContext => ({
    run,
    player,
    bodySize: () => [1.8, 4.5, 1.4],
    nameOf: opts.nameOf ?? ((e) => (e === player ? "you" : e === 4294967295 ? "barrier" : `car #${e}`)),
  });

  function heatFrom(p: PoliceReadout | null | undefined): HeatHud {
    if (!p) return { ...playback.heat, heatKnown: false, bountyKnown: false };
    const state = STATUS_TO_STATE[p.status];
    const escape = p.escapeS ?? (p.phase ? escapeTimeS(p.level, p.phase) : null);
    return {
      level: p.level,
      heat: heatWithinLevel(p.heat, p.level),
      state,
      cooldown: state === "cooldown" && escape ? Math.min(1, (p.searchS ?? p.stalenessS ?? 0) / escape) : 0,
      unitsInPursuit: p.unitsAssigned,
      bounty: Math.round(p.bounty),
      heatKnown: true,
      bountyKnown: true,
    };
  }

  function push(f: LiveFrame): void {
    if (f.player !== player) {
      player = f.player;
      playback = emptyPlayback();
    }
    const c = ctx();
    for (const e of f.events) playback = applyEvent(playback, { tick: f.tick, t_s: f.tick / (f.ticksPerSecond ?? 240), event: e }, c);
    if (playback.scanner.length > SCANNER_KEEP * 2) playback = { ...playback, scanner: playback.scanner.slice(-SCANNER_KEEP) };
    store.update((s) => {
      const v = f.vehicle;
      const pb = playback.scanner;
      const same = s.scanner.length === Math.min(pb.length, SCANNER_KEEP) && s.scanner[s.scanner.length - 1]?.id === pb[pb.length - 1]?.id;
      return {
        ...s,
        hud: {
          ...s.hud,
          entity: player,
          speedMps: v?.speedMps ?? s.hud.speedMps,
          engineRpm: v?.engineRpm ?? s.hud.engineRpm,
          redlineRpm: v?.redlineRpm ?? s.hud.redlineRpm,
          gear: v?.gear ?? s.hud.gear,
          health: v?.health ?? s.hud.health,
          pos: f.radar?.pos ?? v?.pos ?? s.hud.pos,
          headingRad: f.radar?.headingRad ?? v?.headingRad ?? s.hud.headingRad,
          blips: f.radar === undefined ? s.hud.blips : (f.radar?.blips ?? []),
          regions: playback.regions,
          detached: playback.detached,
          components: playback.components,
          failed: playback.failed,
          wheelsDetached: playback.wheelsDetached,
          totalLoss: playback.totalLoss,
          heat: heatFrom(f.police),
          damageMode: f.damageMode ?? s.hud.damageMode,
          timeOfDayS: f.timeOfDayS ?? s.hud.timeOfDayS,
        },
        scanner: same ? s.scanner : pb.slice(-SCANNER_KEEP),
        replay: { ...s.replay, tick: f.tick, ticksPerSecond: f.ticksPerSecond ?? 240 },
      };
    });
  }

  function price(request: BillRequest): void {
    if (!opts.quote) return;
    try {
      const b = opts.quote(player, request);
      store.update((s) => ({ ...s, bill: b ? billFromWasm(b, request) : null }));
    } catch (e) {
      store.update((s) => ({ ...s, notice: { id: noticeId++, text: String(e), kind: "error" } }));
    }
  }

  function send(cmd: UiCommand): void {
    const s = store.get();
    switch (cmd.type) {
      case "bill.request":
        return price({ ...(s.bill?.request ?? DEFAULT_BILL_REQUEST), ...cmd.request });
      case "back":
      case "resume":
        return opts.onBack?.();
      case "notice.dismiss":
        return store.update((st) => ({ ...st, notice: null }));
      default:
        // Other screens are not live yet; they stay on the mock provider.
        return;
    }
  }

  return {
    get: () => store.get(),
    subscribe: (l) => store.subscribe(l),
    send,
    push,
    openBill: () => price({ ...(store.get().bill?.request ?? DEFAULT_BILL_REQUEST), wanted: store.get().hud.heat.level }),
    reset: () => {
      playback = emptyPlayback();
      store.update((s) => ({ ...s, scanner: [], hud: { ...s.hud, heat: emptyPlayback().heat }, bill: null }));
    },
  };
}
