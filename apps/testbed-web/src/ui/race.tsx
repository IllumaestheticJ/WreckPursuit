// Street races in the testbed (workstream 15, machine L): the race picker, the race HUD
// (position, lap, checkpoint, gaps, countdown, standings, M's UI-kit minimap with the route and
// the next gate) and the results panel with G's payout. Data comes from wp_wasm's race exports
// (`race_list`, `race_info`, `race_standings`, `race_course`, `race_result_json`); drawn with the
// UI kit's primitives. A police capture mid-race hands the screen to the end-of-run panel
// (`handoff_run_end`), so this view stays out of its way.
//
// Betting (wp_race::betting + the career economy): each card shows the regulars' form and the
// odds on yourself (win, podium, head-to-head against a rival); a bet picked on the card is
// placed right after the race starts, during the countdown. The results panel settles it and
// shows how everyone's form rating moved.
import { render } from "preact";
import { useState } from "preact/hooks";
import { Badge, Button, Money, Panel, Stat } from "../../../ui-kit/src/ui/kit";
import { Minimap } from "../../../ui-kit/src/screens/hud/Minimap";
import type { CityMap, MapBlip } from "../../../ui-kit/src/model/types";
import "../../../ui-kit/src/ui/theme.css";
import "../../../ui-kit/src/screens/hud/hud.css";
import { RACE_STRIDE, RI, RS } from "../wasm/layout";
import type { Course } from "../race_marker";
import type { LedgerLine } from "./runEnd";

/** `wp_wasm::race::RaceCard`. */
export interface RaceCard {
  id: string;
  name: string;
  blurb: string;
  city: string;
  kind: string;
  laps: number;
  legality: string;
  time_of_day_h: number;
  length_m: number;
  police: number;
  hot_meet: boolean;
  /** The race's police style (`data/police/races.toml`) and how it works. */
  police_style: string;
  police_summary: string;
  drivers: { id: string; nickname: string; rating: number; form: string; name: string; style: string; car: string }[];
  purse: number;
  entry_fee: number;
  terrain: string;
  odds: OddsCard[];
}

/** `wp_wasm::race::OddsCard`: a bet on yourself. */
export interface OddsCard {
  label: string;
  kind: "win" | "podium" | "h2h";
  opponent: string | null;
  odds: number;
}

/** `wp_wasm::race::BetCard`: a ticket. */
export interface BetCard {
  label: string;
  text: string;
  stake: number;
  odds: number;
  outcome: "open" | "won" | "lost" | "void";
  paid: number;
}

/** `wp_wasm::race::RegularCard`: one of the city's regulars. */
export interface RegularCard {
  id: string;
  billing: string;
  car: string;
  home: string;
  specialty: string;
  style: string;
  rating: number;
  starts: number;
  wins: number;
  podiums: number;
  dnfs: number;
  form: string;
}

/** A bet picked on a picker card, placed at the start. */
export interface BetPick {
  label: string;
  stake: number;
}

/** Picker-race betting (venue City) on the current account. */
export interface BetStatus {
  max: number;
  lock: string | null;
}

/** `wp_wasm::race::ResultRow`. */
export interface ResultRow {
  position: number;
  entity: number;
  name: string;
  car: string;
  is_player: boolean;
  status: "Finished" | "Running" | "DNF" | "DQ";
  reason: string | null;
  time_s: number | null;
  gap_s: number | null;
  best_lap_s: number | null;
  clean: boolean;
  contacts: number;
  civilian_hits: number;
}

/** `wp_wasm::run::RacePayout`. */
export interface RacePayout {
  entry_fee: number;
  purse: number;
  clean_bonus: number;
  rep: number;
  bounty: number;
  cash_after: number;
  ledger: LedgerLine[];
}

/** `wp_wasm::race::RaceReport`. */
export interface RaceReport {
  id: string;
  name: string;
  kind: string;
  legality: string;
  length_m: number;
  final_results: boolean;
  rows: ResultRow[];
  player_row: number;
  advertised_purse: number;
  advertised_entry_fee: number;
  payout: RacePayout | null;
  economy_error: string | null;
  handoff_run_end: boolean;
  bets: BetCard[];
  /** `[name, change, new rating]`. */
  rating_changes: [string, number, number][];
}

/** One frame of race state the view draws. */
export interface RaceFrame {
  info: Float32Array;
  standings: Float32Array;
  report: RaceReport | null;
  /** Player pose (sim XY, heading yaw rad: 0 = +Y, positive = left). */
  player: { pos: [number, number]; yaw: number } | null;
  /** Other vehicles to blip: `[entity, x, y, yaw, kind]` (kind 1 AI racer, 2 police). */
  others: [number, number, number, number, number][];
}

const KIND_LABEL: Record<string, string> = { sprint: "Sprint", circuit: "Circuit", checkpoint: "Checkpoint" };
const LEGALITY_LABEL: Record<string, string> = { street: "Street", illegal_daytime: "Illegal · daytime", sanctioned: "Sanctioned" };
const STATUS_LABEL = ["", "FIN", "DNF", "DQ"];
const REASON_LABEL: Record<string, string> = {
  Captured: "busted",
  TotalLoss: "wrecked",
  Retired: "retired",
  Fled: "fled the police",
  FinishWindow: "too slow",
  Stuck: "stuck",
  TimeLimit: "time limit",
  LeftCorridor: "left the route",
  ImplausibleMovement: "teleported",
  JumpStart: "jump start",
};

const fmtTime = (s: number) => {
  const m = Math.floor(s / 60);
  const r = s - m * 60;
  return `${m}:${r.toFixed(2).padStart(5, "0")}`;
};
const fmtHour = (h: number) => `${String(Math.floor(h)).padStart(2, "0")}:${String(Math.round((h % 1) * 60)).padStart(2, "0")}`;
const ordinal = (n: number) => `${n}${n % 10 === 1 && n !== 11 ? "st" : n % 10 === 2 && n !== 12 ? "nd" : n % 10 === 3 && n !== 13 ? "rd" : "th"}`;

const BET_LABEL = (o: OddsCard) => (o.kind === "win" ? "You win" : o.kind === "podium" ? "You finish top 3" : `You beat ${o.opponent ?? "?"}`);

/** Bet slip on a picker card: pick a selection and a stake, or no bet. */
function BetSlip(props: { card: RaceCard; status: BetStatus; pick: BetPick | null; onPick: (p: BetPick | null) => void }) {
  const { card, status, pick } = props;
  if (card.odds.length === 0) return null;
  if (status.lock) {
    const win = card.odds.find((o) => o.kind === "win");
    const top3 = card.odds.find((o) => o.kind === "podium");
    return (
      <div class="wp-dim" style={{ fontSize: "0.8em", marginTop: "0.3em" }} data-testid="bet-locked">
        Odds on you: win {win?.odds.toFixed(2)}
        {top3 ? ` · top 3 ${top3.odds.toFixed(2)}` : ""} · betting locked: {status.lock}
      </div>
    );
  }
  const max = Math.max(0, status.max);
  if (max < 100) return <div class="wp-dim" style={{ fontSize: "0.8em" }}>Betting: not enough cash for the minimum stake (100).</div>;
  const label = pick?.label ?? "";
  const stake = pick?.stake ?? Math.min(500, max);
  const odds = card.odds.find((o) => o.label === label);
  return (
    <div class="wp-spread" style={{ fontSize: "0.8em", marginTop: "0.3em", gap: "0.4em", pointerEvents: "auto" }} data-testid="bet-slip">
      <select
        class="wp-select"
        data-testid="bet-pick"
        value={label}
        onChange={(e) => {
          const v = (e.target as HTMLSelectElement).value;
          props.onPick(v ? { label: v, stake } : null);
        }}
      >
        <option value="">No bet</option>
        {card.odds.map((o) => (
          <option key={o.label} value={o.label}>
            {BET_LABEL(o)} @ {o.odds.toFixed(2)}
          </option>
        ))}
      </select>
      {label && (
        <span>
          stake{" "}
          <input
            type="number"
            min={100}
            max={max}
            step={100}
            value={stake}
            class="wp-select"
            style={{ width: "6em" }}
            onChange={(e) => {
              const v = Math.max(100, Math.min(max, Math.round(Number((e.target as HTMLInputElement).value) || 0)));
              props.onPick({ label, stake: v });
            }}
          />{" "}
          → <Money value={Math.floor((stake * Math.round((odds?.odds ?? 0) * 1000)) / 1000)} /> <span class="wp-dim">(max {max})</span>
        </span>
      )}
    </div>
  );
}

function Picker(props: { cards: RaceCard[]; status: BetStatus; onStart: (id: string, bet: BetPick | null) => void; onClose: () => void }) {
  const [picks, setPicks] = useState<Record<string, BetPick | null>>({});
  return (
    <div class="wp-race-picker" data-testid="race-picker">
      <Panel title="Street races" tag={<Button size="sm" variant="ghost" onClick={props.onClose}>close</Button>}>
        <div class="wp-race-cards">
          {props.cards.map((c) => (
            <div class="wp-race-card" key={c.id} data-race={c.id}>
              <div class="wp-spread">
                <b>{c.name}</b>
                <span>
                  <Badge tone="info">{KIND_LABEL[c.kind] ?? c.kind}{c.kind === "circuit" ? ` · ${c.laps} lap${c.laps > 1 ? "s" : ""}` : ""}</Badge>{" "}
                  <Badge tone={c.legality === "illegal_daytime" ? "danger" : "warn"}>{LEGALITY_LABEL[c.legality] ?? c.legality}</Badge>
                </span>
              </div>
              <div class="wp-dim" style={{ fontSize: "0.85em", margin: "0.3em 0" }}>
                {c.blurb} {c.city === "vesper_v2" ? "Port Vesper" : "Grid city"} · {(c.length_m / 1000).toFixed(1)} km · {fmtHour(c.time_of_day_h)}
                {c.police > 1 ? " · police presence MAX" : ""}
                {c.hot_meet ? " · hot meet: a patrol will come" : ""}
              </div>
              {c.police_style ? (
                <div style={{ fontSize: "0.8em", margin: "0 0 0.3em" }}>
                  <b>Police: {c.police_style}</b> <span class="wp-dim">— {c.police_summary}</span>
                </div>
              ) : null}
              <div style={{ fontSize: "0.8em" }}>
                {c.drivers.map((d) => (
                  <div key={d.name}>
                    <b>{d.name}</b> <span class="wp-dim">"{d.nickname}" ({d.car}) — {d.style}</span>{" "}
                    <span class="wp-mono" style={{ whiteSpace: "nowrap" }} title="form rating · last five results (X = DNF)">
                      {d.rating} {d.form.replace(/^-+/, "")}
                    </span>
                  </div>
                ))}
              </div>
              <div class="wp-spread" style={{ marginTop: "0.4em" }}>
                <span>
                  Purse <Money value={c.purse} /> · buy-in <Money value={c.entry_fee} />
                </span>
                <Button variant="primary" size="sm" onClick={() => props.onStart(c.id, picks[c.id] ?? null)}>
                  Race
                </Button>
              </div>
              <BetSlip card={c} status={props.status} pick={picks[c.id] ?? null} onPick={(p) => setPicks({ ...picks, [c.id]: p })} />
            </div>
          ))}
        </div>
      </Panel>
    </div>
  );
}

function Hud(props: { frame: RaceFrame; names: Map<number, string>; map: CityMap | null; course: Course | null }) {
  const { info, standings } = props.frame;
  const player = info[RI.player];
  const n = info[RI.racers];
  const rows: number[][] = [];
  for (let o = 0; o + RACE_STRIDE <= standings.length; o += RACE_STRIDE) rows.push(Array.from(standings.subarray(o, o + RACE_STRIDE)));
  const me = rows.find((r) => r[RS.entity] === player);
  const laps = info[RI.laps];
  const gates = info[RI.gates];
  const closed = info[RI.closed] > 0.5;
  const nextGate = me ? me[RS.nextGate] : 0;
  // Checkpoints this lap (a circuit's gate 0 is the line; it closes the lap).
  const cpNow = me ? (closed ? (nextGate === 0 ? gates : nextGate) : nextGate + 1) : 0;
  const gap = (v: number | undefined) => (v === undefined || v < 0 ? "—" : `${v.toFixed(1)} s`);
  const phase = info[RI.phase];
  const countdown = info[RI.countdownS];
  // The kit's minimap takes compass headings (clockwise); sim yaw turns left-positive.
  const blips: MapBlip[] = [];
  for (const [id, x, y, yaw, kind] of props.frame.others) blips.push({ id, kind: kind === 2 ? "police" : "racer", pos: [x, y], headingRad: -yaw });
  const g = props.course?.gates[nextGate];
  if (g) blips.push({ id: 9999, kind: "checkpoint", pos: [g.pos[0], g.pos[1]] });
  // Direction to the next gate relative to the car's heading (arrow fallback next to the map).
  let arrowDeg = 0;
  if (g && props.frame.player) {
    const [px, py] = props.frame.player.pos;
    const bearing = Math.atan2(-(g.pos[0] - px), g.pos[1] - py); // yaw convention: 0 = +Y, left positive
    arrowDeg = (-(bearing - props.frame.player.yaw) * 180) / Math.PI;
  }
  return (
    <>
      {phase === 0 && (
        <div class="wp-race-countdown" data-testid="race-countdown">
          {countdown > 0.05 ? Math.ceil(countdown) : "GO"}
        </div>
      )}
      {phase === 1 && info[RI.elapsedS] < 1.5 && <div class="wp-race-countdown wp-race-go">GO</div>}
      <div class="wp-race-hud" data-testid="race-hud">
        <Panel title="Race" tag={me ? (me[RS.status] > 0 ? STATUS_LABEL[me[RS.status]] : fmtTime(Math.max(0, info[RI.elapsedS]))) : ""}>
          <div class="wp-race-hud-grid">
          <div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "0.4em" }}>
            <Stat label="Pos" value={me ? `${me[RS.position]}/${n}` : "—"} tone={me && me[RS.position] === 1 ? "ok" : undefined} />
            <Stat label={closed ? "Lap" : "Run"} value={closed ? `${me ? me[RS.lap] : 1}/${laps}` : `${Math.round(info[RI.lengthM])} m`} />
            <Stat label="CP" value={`${Math.min(cpNow, gates)}/${gates}`} />
            <Stat label="Ahead" value={gap(me?.[RS.gapAheadS])} />
            <Stat label="Behind" value={gap(me?.[RS.gapBehindS])} />
            <div title="next checkpoint" style={{ display: "flex", alignItems: "center", justifyContent: "center" }}>
              <span data-testid="race-gate-arrow" style={{ display: "inline-block", fontSize: "1.8em", transform: `rotate(${arrowDeg}deg)` }}>
                ▲
              </span>
            </div>
          </div>
          <table class="wp-mono wp-race-standings">
            <tbody>
              {rows.map((r) => (
                <tr key={r[RS.entity]} class={r[RS.entity] === player ? "is-local" : ""}>
                  <td>{r[RS.position]}</td>
                  <td>{props.names.get(r[RS.entity]) ?? `#${r[RS.entity]}`}</td>
                  <td class="num">{r[RS.status] > 0 ? STATUS_LABEL[r[RS.status]] : `${Math.round(r[RS.progressM])} m`}</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
          {props.map && props.frame.player && (
            <div data-testid="race-minimap">
              <Minimap map={props.map} pos={props.frame.player.pos} headingRad={-props.frame.player.yaw} blips={blips} radiusM={300} size="9.5em" />
            </div>
          )}
          </div>
        </Panel>
      </div>
    </>
  );
}

function Results(props: { report: RaceReport; onAgain: () => void; onPicker: () => void; onFree: () => void }) {
  const r = props.report;
  const me = r.rows[r.player_row];
  const p = r.payout;
  return (
    <div class="wp-race-results" data-testid="race-results">
      <Panel title={r.name} tag={r.final_results ? "RESULTS" : "PROVISIONAL"}>
        <div style={{ marginBottom: "0.5em" }}>
          {me.status === "Finished" ? (
            <b class={me.position === 1 ? "wp-ok" : ""}>You finished {ordinal(me.position)}</b>
          ) : (
            <b class="wp-danger">
              {me.status} — {REASON_LABEL[me.reason ?? ""] ?? me.reason}
            </b>
          )}
          {!r.final_results && <span class="wp-dim"> · waiting for the rest of the field…</span>}
        </div>
        <table class="wp-table wp-mono" style={{ width: "100%", fontSize: "0.85em" }}>
          <thead>
            <tr>
              <th>Pos</th>
              <th>Driver</th>
              <th>Car</th>
              <th class="num">Time</th>
              <th class="num">Gap</th>
            </tr>
          </thead>
          <tbody>
            {r.rows.map((row) => (
              <tr key={row.entity} class={row.is_player ? "is-local" : ""} data-status={row.status}>
                <td>{row.status === "Finished" ? row.position : "—"}</td>
                <td>
                  {row.name}
                  {row.clean && row.status === "Finished" ? " ✓" : ""}
                </td>
                <td class="wp-dim">{row.car}</td>
                <td class="num">
                  {row.time_s !== null ? (
                    fmtTime(row.time_s)
                  ) : (
                    <Badge tone={row.status === "Running" ? "dim" : "danger"} title={row.reason ?? ""}>
                      {row.status === "Running" ? "racing" : `${row.status} · ${REASON_LABEL[row.reason ?? ""] ?? row.reason}`}
                    </Badge>
                  )}
                </td>
                <td class="num">{row.gap_s ? `+${row.gap_s.toFixed(2)}` : ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {p && (
          <div style={{ marginTop: "0.6em" }} data-testid="race-payout">
            <div class="wp-spread">
              <span class="wp-dim">Buy-in</span>
              <Money value={-p.entry_fee} tone />
            </div>
            <div class="wp-spread">
              <span class="wp-dim">Purse</span>
              <Money value={p.purse} tone />
            </div>
            {p.clean_bonus > 0 && (
              <div class="wp-spread">
                <span class="wp-dim">Clean race bonus</span>
                <Money value={p.clean_bonus} tone />
              </div>
            )}
            <div class="wp-spread">
              <span class="wp-dim">Rep</span>
              <span class="wp-mono">{p.rep > 0 ? `+${p.rep}` : p.rep}</span>
            </div>
            {p.bounty > 0 && (
              <div class="wp-spread">
                <span class="wp-dim">Bounty</span>
                <span class="wp-mono">+{p.bounty}</span>
              </div>
            )}
            <div class="wp-spread" style={{ marginTop: "0.3em" }}>
              <b>Cash</b>
              <Money value={p.cash_after} />
            </div>
          </div>
        )}
        {r.bets.length > 0 && (
          <div style={{ marginTop: "0.6em" }} data-testid="race-bets">
            <b>Bets</b>
            {r.bets.map((b) => (
              <div class="wp-spread" key={b.label} data-outcome={b.outcome}>
                <span class="wp-dim">
                  {b.text} · <Money value={b.stake} /> @ {b.odds.toFixed(2)}
                </span>
                {b.outcome === "open" ? (
                  <Badge tone="dim">pays <Money value={b.paid} /></Badge>
                ) : b.outcome === "won" ? (
                  <Money value={b.paid - b.stake} tone />
                ) : b.outcome === "void" ? (
                  <Badge tone="dim">void · refunded</Badge>
                ) : (
                  <Money value={-b.stake} tone />
                )}
              </div>
            ))}
          </div>
        )}
        {r.rating_changes.length > 0 && (
          <div style={{ marginTop: "0.6em", fontSize: "0.85em" }} data-testid="race-form">
            <b>Form</b>{" "}
            {r.rating_changes.map(([name, d, rating]) => (
              <span key={name} class="wp-mono" style={{ marginRight: "0.8em" }}>
                {name} {rating} <span class={d >= 0 ? "wp-ok" : "wp-danger"}>({d >= 0 ? "+" : ""}{d})</span>
              </span>
            ))}
          </div>
        )}
        {!p && r.final_results && <p class="wp-dim">Advertised purse <Money value={r.advertised_purse} /> (no account in this session).</p>}
        {r.economy_error && <p class="wp-danger" style={{ fontSize: "0.85em" }}>{r.economy_error}</p>}
        <div style={{ display: "flex", gap: "0.5em", marginTop: "0.8em", flexWrap: "wrap" }}>
          <Button variant="primary" onClick={props.onAgain}>
            Race again
          </Button>
          <Button onClick={props.onPicker}>Races…</Button>
          <Button variant="ghost" onClick={props.onFree}>
            Free drive
          </Button>
        </div>
      </Panel>
    </div>
  );
}

/** Mounts the race picker / HUD / results into `el`. */
export class RaceView {
  /** Picker cards while the picker is open. */
  cards: RaceCard[] | null = null;
  /** Betting status for the open picker. */
  private betStatus: BetStatus = { max: 0, lock: null };
  /** Latest results (e2e reads it). */
  report: RaceReport | null = null;
  /** Race id of the current session ("" = no race). */
  raceId = "";
  private names = new Map<number, string>();
  private course: Course | null = null;
  private map: CityMap | null = null;
  private lastDraw = 0;
  private frame: RaceFrame | null = null;

  constructor(
    private readonly el: HTMLElement,
    private readonly actions: {
      start: (id: string, bet?: BetPick | null) => void;
      freeDrive: () => void;
      list: () => RaceCard[];
      betStatus: () => BetStatus;
    },
  ) {}

  /** Open the picker. */
  openPicker(): void {
    this.cards = this.actions.list();
    this.betStatus = this.actions.betStatus();
    this.draw();
  }

  /** Close the picker. */
  closePicker(): void {
    this.cards = null;
    this.draw();
  }

  /** New session: `raceId` "" when it is not a race; `names` by entity; the race's course. */
  reset(raceId: string, names: Map<number, string>, course: Course | null): void {
    this.raceId = raceId;
    this.names = names;
    this.course = course;
    this.report = null;
    this.frame = null;
    this.cards = null;
    this.map = course
      ? (() => {
          const xs = course.route.map((p) => p[0]);
          const ys = course.route.map((p) => p[1]);
          return {
            id: `race-${raceId}`,
            boundsMin: [Math.min(...xs), Math.min(...ys)],
            boundsMax: [Math.max(...xs), Math.max(...ys)],
            roads: [{ id: 0, points: course.route.map((p) => [p[0], p[1]] as [number, number]), speedLimitMps: 0 }],
            zones: [],
            pois: [],
          } satisfies CityMap;
        })()
      : null;
    this.draw();
  }

  /** Call every frame during a race session (redraws at ~10 Hz). */
  update(frame: RaceFrame, nowMs: number): void {
    this.frame = frame;
    if (frame.report) this.report = frame.report;
    if (nowMs - this.lastDraw < 100) return;
    this.lastDraw = nowMs;
    this.draw();
  }

  private draw(): void {
    const f = this.frame;
    const showResults = this.report && !this.report.handoff_run_end;
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }} data-testid="race-view">
        {this.raceId && f && f.info.length > 0 && !showResults && <Hud frame={f} names={this.names} map={this.map} course={this.course} />}
        {showResults && this.report && (
          <Results report={this.report} onAgain={() => this.actions.start(this.raceId)} onPicker={() => this.openPicker()} onFree={() => this.actions.freeDrive()} />
        )}
        {this.cards && <Picker cards={this.cards} status={this.betStatus} onStart={(id, bet) => this.actions.start(id, bet)} onClose={() => this.closePicker()} />}
      </div>,
      this.el,
    );
  }
}
