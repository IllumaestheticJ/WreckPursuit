// Touge battles in the testbed: the pass picker (pass, uphill / downhill, day / night,
// dry / rain, challenger / boss), the rep-gated wager menu (side bet, pink slips) and the battle
// panel shown next to the race results (who won, who went off the mountain, the opponent
// car's condition, the settlement). Data comes from wp_wasm's touge exports (`touge_list_json`,
// `touge_menu_json`, `touge_result_json`); the battle itself is an ordinary race session, so the
// race HUD and results (ui/race.tsx) draw the rest.
import { render } from "preact";
import { Badge, Button, Money, Panel } from "../../../ui-kit/src/ui/kit";
import "../../../ui-kit/src/ui/theme.css";
import type { LedgerLine } from "./runEnd";

/** `wp_wasm::touge::RacerCard`. */
export interface TougeRacer {
  name: string;
  title: string;
  style: string;
  car: string;
}

/** `wp_wasm::touge::TougeCard`. */
export interface TougeCard {
  id: string;
  name: string;
  blurb: string;
  length_m: number;
  climb_m: number;
  drop_stretches: number;
  challenger: TougeRacer;
  boss: TougeRacer;
}

/** `wp_race::touge::MenuLine`. */
export interface MenuLine {
  unlocked: boolean;
  locked_reason: string | null;
}

/** `wp_race::touge::WagerMenu`. */
export interface WagerMenu {
  rep: number;
  tier: string;
  cash: number;
  race: MenuLine;
  side_bet: MenuLine;
  max_stake: number;
  side_bet_payout: number;
  pink_slip: MenuLine;
  win_rep: number;
  loss_rep: number;
  purse: number;
  opponent_car: string;
}

/** `wp_race::touge::Wager` (serde tag `kind`). */
export type Wager = { kind: "none" } | { kind: "side_bet"; stake: number } | { kind: "pink_slip" };

/** `wp_race::touge::TougeSettlement`. */
export interface TougeSettlement {
  result: "Won" | "Lost" | "Draw";
  player_totaled: boolean;
  bet_payout: number;
  bet_net: number;
  purse: number;
  rep: number;
  car_won: number | null;
  car_won_id: string | null;
  car_lost: number | null;
}

/** `wp_wasm::touge::TougeReport`. */
export interface TougeReport {
  scenario: string;
  map: string;
  opponent: string;
  title: string;
  opponent_car: string;
  /** Body colour 0xRRGGBB the NPC car races in (a won pink slip arrives in it). */
  opponent_paint: number;
  /** The NPC car's mods (its build; a won pink slip arrives with them). */
  opponent_mods: string[];
  direction: string;
  daylight: string;
  surface: string;
  boss: boolean;
  wager: Wager;
  wager_taken: boolean;
  wager_error: string | null;
  outcome: { result: "Won" | "Lost" | "Draw"; tick: number; totaled: number | null; breach: "OffCourse" | "OffTheEdge" | null } | null;
  opponent_health: number | null;
  opponent_totaled: boolean | null;
  settlement: TougeSettlement | null;
  settle_error: string | null;
  ledger: LedgerLine[];
}

/** The picker's choices: the keys `touge_menu_json` / `touge_start` take. */
export interface TougeChoice {
  map: string;
  direction: "up" | "down";
  daylight: "day" | "night";
  surface: "dry" | "rain";
  opponent: "challenger" | "boss";
}

/** What the view needs from the app. */
export interface TougeActions {
  list: () => TougeCard[];
  menu: (c: TougeChoice) => WagerMenu;
  start: (c: TougeChoice, wager: Wager) => void;
  /** Testbed only: set the plain session's rep (null: refused, e.g. during a career). */
  setRep: ((rep: number) => string | null) | null;
  /** The player closed the picker (the career garage reopens when it opened it). */
  closed?: () => void;
  /** During a career run: "Back to garage" on the decided battle's panel (null otherwise). */
  garage?: () => (() => void) | null;
}

const toggle = <T extends string>(value: T, options: [T, string][], set: (v: T) => void, testid: string) => (
  <span class="wp-touge-toggle" data-testid={testid} style={{ display: "inline-flex", gap: "0.25em" }}>
    {options.map(([v, label]) => (
      <Button key={v} size="sm" variant={v === value ? "primary" : "ghost"} onClick={() => set(v)}>
        {label}
      </Button>
    ))}
  </span>
);

const locked = (l: MenuLine) => (l.unlocked ? null : <span class="wp-dim" style={{ fontSize: "0.8em" }}> · {l.locked_reason}</span>);

function Picker(props: {
  cards: TougeCard[];
  choice: TougeChoice;
  menu: WagerMenu | null;
  menuError: string | null;
  wager: Wager;
  stake: number;
  repNote: string | null;
  canSetRep: boolean;
  onChoice: (c: TougeChoice) => void;
  onWager: (w: Wager) => void;
  onStake: (s: number) => void;
  onStart: () => void;
  onSetRep: (rep: number) => void;
  onClose: () => void;
}) {
  const c = props.choice;
  const set = (patch: Partial<TougeChoice>) => props.onChoice({ ...c, ...patch });
  const m = props.menu;
  const card = props.cards.find((k) => k.id === c.map) ?? props.cards[0];
  const racer = card ? card[c.opponent] : null;
  const canRace = m?.race.unlocked ?? false;
  return (
    <div class="wp-race-picker" data-testid="touge-picker">
      <Panel title="Touge battles" tag={<Button size="sm" variant="ghost" onClick={props.onClose}>close</Button>}>
        <div class="wp-race-cards">
          {props.cards.map((k) => (
            <div class="wp-race-card" key={k.id} data-touge={k.id} style={k.id === c.map ? { outline: "1px solid var(--wp-accent, #6cf)" } : {}}>
              <div class="wp-spread">
                <b>{k.name}</b>
                <Button size="sm" variant={k.id === c.map ? "primary" : "ghost"} onClick={() => set({ map: k.id })}>
                  {k.id === c.map ? "Selected" : "Select"}
                </Button>
              </div>
              <div class="wp-dim" style={{ fontSize: "0.85em", margin: "0.3em 0" }}>
                {k.blurb} {(k.length_m / 1000).toFixed(1)} km · {Math.round(k.climb_m)} m climb · {k.drop_stretches} open drops
              </div>
              <div style={{ fontSize: "0.8em" }}>
                <div>
                  Challenger <b>{k.challenger.name}</b> <span class="wp-dim">({k.challenger.car}) — {k.challenger.style}</span>
                </div>
                <div>
                  Boss <b>{k.boss.name}</b>, “{k.boss.title}” <span class="wp-dim">({k.boss.car}) — {k.boss.style}</span>
                </div>
              </div>
            </div>
          ))}
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "0.35em 0.8em", alignItems: "center", marginTop: "0.6em" }}>
          <span class="wp-dim">Direction</span>
          {toggle(c.direction, [["down", "Downhill"], ["up", "Uphill"]], (v) => set({ direction: v }), "touge-direction")}
          <span class="wp-dim">Time</span>
          {toggle(c.daylight, [["day", "Day"], ["night", "Night"]], (v) => set({ daylight: v }), "touge-daylight")}
          <span class="wp-dim">Road</span>
          {toggle(c.surface, [["dry", "Dry"], ["rain", "Rain"]], (v) => set({ surface: v }), "touge-surface")}
          <span class="wp-dim">Opponent</span>
          {toggle(c.opponent, [["challenger", "Challenger"], ["boss", "Boss"]], (v) => set({ opponent: v }), "touge-opponent")}
        </div>
        {props.menuError && <p class="wp-danger" style={{ fontSize: "0.85em" }}>{props.menuError}</p>}
        {m && (
          <div style={{ marginTop: "0.6em", fontSize: "0.9em" }} data-testid="touge-wagers">
            <div class="wp-spread">
              <span>
                Rep <b class="wp-mono">{m.rep}</b> <Badge tone="info">{m.tier}</Badge>
              </span>
              <span>
                Cash <Money value={m.cash} />
              </span>
            </div>
            {racer && (
              <div class="wp-dim" style={{ margin: "0.3em 0" }}>
                vs {racer.name} in the {m.opponent_car}: win {m.win_rep > 0 ? `+${m.win_rep}` : m.win_rep} rep
                {m.purse > 0 ? <> and <Money value={m.purse} /></> : null}, lose {m.loss_rep} rep. Off the mountain = totaled, battle over.
              </div>
            )}
            {!canRace && <p class="wp-danger">Locked{locked(m.race)}</p>}
            <div style={{ display: "flex", flexDirection: "column", gap: "0.3em", marginTop: "0.3em" }}>
              <label>
                <input type="radio" name="touge-wager" checked={props.wager.kind === "none"} onChange={() => props.onWager({ kind: "none" })} /> No wager
              </label>
              <label style={{ opacity: m.side_bet.unlocked ? 1 : 0.5 }}>
                <input
                  type="radio"
                  name="touge-wager"
                  disabled={!m.side_bet.unlocked}
                  checked={props.wager.kind === "side_bet"}
                  onChange={() => props.onWager({ kind: "side_bet", stake: props.stake })}
                />{" "}
                Side bet, pays {m.side_bet_payout}× (max <Money value={m.max_stake} />)
                {locked(m.side_bet)}
                {props.wager.kind === "side_bet" && (
                  <input
                    type="number"
                    min={0}
                    max={m.max_stake}
                    step={500}
                    value={props.stake}
                    style={{ width: "7em", marginLeft: "0.5em" }}
                    data-testid="touge-stake"
                    onInput={(e) => props.onStake(Number((e.target as HTMLInputElement).value) || 0)}
                  />
                )}
              </label>
              <label style={{ opacity: m.pink_slip.unlocked ? 1 : 0.5 }}>
                <input
                  type="radio"
                  name="touge-wager"
                  disabled={!m.pink_slip.unlocked}
                  checked={props.wager.kind === "pink_slip"}
                  onChange={() => props.onWager({ kind: "pink_slip" })}
                />{" "}
                <b>Pink slips</b>: winner takes the loser's car exactly as it finished
                {locked(m.pink_slip)}
              </label>
            </div>
          </div>
        )}
        <div class="wp-spread" style={{ marginTop: "0.7em", gap: "0.5em", flexWrap: "wrap" }}>
          {props.canSetRep ? (
            <span style={{ fontSize: "0.8em" }} data-testid="touge-set-rep">
              <span class="wp-dim">Testbed rep: </span>
              {[0, 2000, 5000, 10000].map((r) => (
                <Button key={r} size="sm" variant="ghost" onClick={() => props.onSetRep(r)}>
                  {r}
                </Button>
              ))}
              {props.repNote && <span class="wp-dim"> {props.repNote}</span>}
            </span>
          ) : (
            <span />
          )}
          <Button variant="primary" onClick={props.onStart} disabled={!canRace}>
            Battle
          </Button>
        </div>
      </Panel>
    </div>
  );
}

const DIR_LABEL: Record<string, string> = { up: "uphill", down: "downhill" };

function BattlePanel(props: { report: TougeReport; onGarage: (() => void) | null }) {
  const r = props.report;
  const s = r.settlement;
  const o = r.outcome;
  return (
    <div class="wp-touge-battle" data-testid="touge-battle" style={{ position: "absolute", left: "1em", bottom: "1em", width: "22em", pointerEvents: "auto" }}>
      <Panel title={`${r.boss ? "Boss" : "Challenger"}: ${r.opponent}`} tag={o ? (o.result === "Won" ? "WON" : o.result === "Draw" ? "DRAW" : "LOST") : "BATTLE"}>
        <div class="wp-dim" style={{ fontSize: "0.85em" }}>
          “{r.title}” in the {r.opponent_car} · {DIR_LABEL[r.direction] ?? r.direction} · {r.daylight} · {r.surface}
        </div>
        <div style={{ fontSize: "0.85em", marginTop: "0.3em" }}>
          Wager:{" "}
          {r.wager.kind === "none" ? "none" : r.wager.kind === "side_bet" ? <>side bet <Money value={r.wager.stake} /></> : <b>pink slips</b>}
          {r.wager_error && <span class="wp-danger"> refused ({r.wager_error})</span>}
        </div>
        {o && (
          <div style={{ marginTop: "0.4em" }}>
            <b class={o.result === "Won" ? "wp-ok" : o.result === "Draw" ? "wp-dim" : "wp-danger"}>
              {o.result === "Won" ? "You won the battle" : o.result === "Draw" ? "Draw: both cars stuck, stakes refunded" : "You lost the battle"}
            </b>
            {o.breach && (
              <div class="wp-danger" style={{ fontSize: "0.85em" }}>
                {o.result === "Won" ? `${r.opponent} ` : "You "}
                {o.breach === "OffCourse" ? "left the road. Totaled." : "went off the mountain. Totaled."}
              </div>
            )}
          </div>
        )}
        {r.opponent_health !== null && (
          <div style={{ fontSize: "0.85em", marginTop: "0.3em" }}>
            Opponent car: {Math.round(r.opponent_health * 100)}% health{r.opponent_totaled ? ", total loss" : ""}
          </div>
        )}
        {s && (
          <div style={{ marginTop: "0.4em", fontSize: "0.9em" }} data-testid="touge-settlement">
            {s.bet_payout !== 0 || s.bet_net !== 0 ? (
              <div class="wp-spread">
                <span class="wp-dim">Side bet</span>
                <Money value={s.bet_net} tone />
              </div>
            ) : null}
            {s.purse > 0 && (
              <div class="wp-spread">
                <span class="wp-dim">Purse</span>
                <Money value={s.purse} tone />
              </div>
            )}
            <div class="wp-spread">
              <span class="wp-dim">Rep</span>
              <span class="wp-mono">{s.rep > 0 ? `+${s.rep}` : s.rep}</span>
            </div>
            {s.car_won_id && <div class="wp-ok">Pink slip: the {s.car_won_id} is in your garage, as it finished.</div>}
            {s.car_lost !== null && <div class="wp-danger">Pink slip: your car now belongs to {r.opponent}.</div>}
          </div>
        )}
        {r.settle_error && <p class="wp-danger" style={{ fontSize: "0.85em" }}>{r.settle_error}</p>}
        {o && props.onGarage && (
          // A career battle (playtest row 167): settled on the career; back to its garage.
          <div style={{ marginTop: "0.6em" }}>
            <Button variant="primary" onClick={props.onGarage} testid="touge-to-garage">
              Back to garage
            </Button>
          </div>
        )}
      </Panel>
    </div>
  );
}

/** Mounts the touge picker and the battle panel into `el`. */
export class TougeView {
  /** Pass cards while the picker is open. */
  cards: TougeCard[] | null = null;
  /** Latest battle report (e2e reads it); null outside a touge battle. */
  report: TougeReport | null = null;
  private choice: TougeChoice = { map: "", direction: "down", daylight: "night", surface: "dry", opponent: "challenger" };
  private menu: WagerMenu | null = null;
  private menuError: string | null = null;
  private wager: Wager = { kind: "none" };
  private stake = 1000;
  private repNote: string | null = null;
  private lastDraw = 0;

  constructor(
    private readonly el: HTMLElement,
    private readonly actions: TougeActions,
  ) {}

  /** Open the picker. */
  openPicker(): void {
    this.cards = this.actions.list();
    if (!this.choice.map && this.cards.length > 0) this.choice.map = this.cards[0].id;
    this.refreshMenu();
  }

  /** Close the picker. */
  closePicker(): void {
    this.cards = null;
    this.draw();
  }

  /** New session (touge or not): drop the last battle panel. */
  reset(): void {
    this.report = null;
    this.cards = null;
    this.draw();
  }

  /** Per frame during a touge battle (redraws at ~4 Hz). */
  update(report: TougeReport | null, nowMs: number): void {
    if (nowMs - this.lastDraw < 250) return;
    this.lastDraw = nowMs;
    this.report = report;
    this.draw();
  }

  private refreshMenu(): void {
    try {
      this.menu = this.actions.menu(this.choice);
      this.menuError = null;
      // A wager the new menu no longer allows falls back to none.
      if ((this.wager.kind === "side_bet" && !this.menu.side_bet.unlocked) || (this.wager.kind === "pink_slip" && !this.menu.pink_slip.unlocked))
        this.wager = { kind: "none" };
      if (this.menu.max_stake > 0) this.stake = Math.min(this.stake, this.menu.max_stake);
    } catch (e) {
      this.menu = null;
      this.menuError = String(e).replace(/^(Error: )?wp_wasm: /, "");
    }
    this.draw();
  }

  private draw(): void {
    const setRep = this.actions.setRep;
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }} data-testid="touge-view">
        {this.report && <BattlePanel report={this.report} onGarage={this.actions.garage?.() ?? null} />}
        {this.cards && (
          <Picker
            cards={this.cards}
            choice={this.choice}
            menu={this.menu}
            menuError={this.menuError}
            wager={this.wager}
            stake={this.stake}
            repNote={this.repNote}
            canSetRep={setRep !== null}
            onChoice={(c) => {
              this.choice = c;
              this.refreshMenu();
            }}
            onWager={(w) => {
              this.wager = w;
              this.draw();
            }}
            onStake={(s) => {
              this.stake = s;
              if (this.wager.kind === "side_bet") this.wager = { kind: "side_bet", stake: s };
              this.draw();
            }}
            onStart={() => {
              const wager = this.wager.kind === "side_bet" ? { kind: "side_bet" as const, stake: this.stake } : this.wager;
              this.cards = null;
              this.actions.start(this.choice, wager);
            }}
            onSetRep={(rep) => {
              this.repNote = setRep ? setRep(rep) : null;
              this.refreshMenu();
            }}
            onClose={() => {
              this.closePicker();
              this.actions.closed?.();
            }}
          />
        )}
      </div>,
      this.el,
    );
  }
}
