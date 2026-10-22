// Street racers (wp_wasm::street): NPC racers roam the city; honk (the rebindable `horn` action:
// Q / pad L3 by default, the 📯 touch button) next to one to challenge it, pick the finish (B)
// on the city map and a bet, race from where you are (A).
//
// The maps are the UI kit's (src/ui/map.ts MapOverlay → MapHud): this class feeds them the
// racers as level-coloured dots (number = level), the planned / running route with its A / B
// flags (`MapHud.setExtras`), a "Street racers" legend group, and during a challenge arms the
// full map's picker (tap / click = B; pad: left stick moves the crosshair, A sets B) with the
// challenge card in the map's host panel. What stays here: the challenge hint, the race panel
// (top-left stack, laid out by ui/layout.ts like the race HUD), the countdown and the result
// banner. All of it hides while `frame(…, visible = false)` (game menu, photo mode, crash replay).
import type { MapExtras, MapFlag, MapHud, MapMarker } from "../../../ui-kit/src/live";
import { getBindings } from "../input";
import { keyName, padName } from "../bindings";
import { sim, type StreetPreview, type StreetStatus } from "../wasm/sim";

const STYLE = `
#street-ui { position: absolute; inset: 0; pointer-events: none; z-index: 3; }
#street-ui > * { position: absolute; }
#street-hint { left: 50%; bottom: 18vh; transform: translateX(-50%); padding: 0.45em 0.9em; white-space: nowrap; }
#street-hud { left: 8px; top: 312px; width: min(360px, calc(100vw - 32px)); z-index: 60; }
#street-hud .st-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 0.4em; }
#street-result { left: 50%; top: 20%; transform: translate(-50%, -50%); text-align: center; text-shadow: 0 2px 8px #000; font-family: system-ui, sans-serif; }
#street-result b { display: block; font: 900 46px/1 system-ui, sans-serif; letter-spacing: 0.08em; }
#street-result span { display: block; margin-top: 6px; font-size: 15px; color: #eee; }
.st-lv { display: inline-block; padding: 0 0.5em; border-radius: 1em; color: #000; font-weight: 700; font-size: 0.85em; }
.st-dot { display: inline-block; width: 0.75em; height: 0.75em; border-radius: 50%; margin-right: 0.4em; vertical-align: -0.05em; box-shadow: 0 0 0 1px #fff; }
.wp-map-host .st-row { margin: 0.55em 0; font-size: 0.85em; line-height: 1.35; }
.wp-map-host h3 { margin: 0 0 0.3em; font-size: 1.05em; }
.wp-map-host ul { margin: 0.3em 0 0; padding: 0; list-style: none; font-size: 0.8em; }
.wp-map-host li { margin: 0.2em 0; }
.wp-map-host input[type=range] { width: 100%; }
.wp-map-host .st-buttons { display: flex; flex-wrap: wrap; gap: 0.5em; margin-top: 0.7em; }
.wp-map-host .st-buttons .wp-kbd { color: inherit; background: transparent; border-color: currentColor; }
`;

const money = (n: number) => (n < 0 ? "−$" : "$") + Math.abs(Math.round(n)).toLocaleString("en-US");
const km = (m: number) => (m >= 1000 ? `${(m / 1000).toFixed(2)} km` : `${Math.round(m)} m`);
const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const clock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

/** Floats per `street_racers()` record: [entity, level, x, y, yaw, mode]. */
const RACER_STRIDE = 6;
/** Gamepad buttons in the picker (standard mapping). Driving input is paused while the map is
 * open, so A / B / LB / RB / Menu are free there. */
const PAD = { a: 0, b: 1, lb: 4, rb: 5, menu: 9 } as const;
/** Crosshair speed of the left stick, CSS px per second at full deflection. */
const PAD_PAN_PX_S = 700;

/** Shown names of an action's first key and pad control (rebindable). */
function bound(action: "horn" | "map"): { key: string; pad: string } {
  const b = getBindings();
  return { key: b.keys[action].map(keyName)[0] ?? "", pad: b.pad[action].map(padName)[0] ?? "" };
}

/** The street racer UI (one per testbed). */
export class StreetUi {
  private readonly root: HTMLElement;
  private readonly hint: HTMLElement;
  private readonly hud: HTMLElement;
  private readonly count: HTMLElement;
  private readonly resultEl: HTMLElement;
  private racers: Float32Array = new Float32Array(0);
  /** Roaming street racers' entities this frame: everyone but the one racing the player. The
   * LOD director hands these to the instanced fleet away from the camera (render/lod.ts). */
  readonly roaming = new Set<number>();
  /** Latest status (refreshed a few times a second); read by e2e. */
  status: StreetStatus | null = null;
  /** Route preview of the open challenge (e2e reads it). */
  preview: StreetPreview | null = null;
  /** Stake chosen for the open challenge, $. */
  stake = 0;
  private offerEntity = -1;
  /** This UI opened the full map for the open challenge (and closes it with the challenge). */
  private openedMap = false;
  private resultSeq = 0;
  private resultUntil = 0;
  private frameNo = 0;
  private error = "";
  private cardKey = "";
  private legendSet = false;
  /** Pad buttons held last picker frame; null = not sampled yet (a button already held when
   * the picker opens, e.g. A as the handbrake, is not a press). */
  private padWas: boolean[] | null = null;
  private lastMs = 0;

  constructor(
    parent: HTMLElement,
    private readonly map: MapHud,
    /** Touch controls are showing (hint names the 📯 button). */
    private readonly touch: () => boolean = () => false,
  ) {
    const style = document.createElement("style");
    style.textContent = STYLE;
    document.head.appendChild(style);
    this.root = document.createElement("div");
    this.root.id = "street-ui";
    this.root.className = "wp-ui";
    parent.appendChild(this.root);
    const div = (id: string, cls = "") => {
      const el = document.createElement("div");
      el.id = id;
      el.className = `hidden ${cls}`;
      this.root.appendChild(el);
      return el;
    };
    this.hint = div("street-hint", "wp-panel");
    this.hud = div("street-hud", "wp-street-hud");
    this.count = div("street-count", "wp-race-countdown");
    this.resultEl = div("street-result");
    // Enter / Esc answer the challenge while the picker is open. Capture phase, registered before
    // the map's own key handler (added when it opens) and the testbed's (Enter = crash replay).
    window.addEventListener(
      "keydown",
      (e) => {
        if (e.repeat || !this.map.isOpen || !this.status?.offer) return;
        if ((e.target as HTMLElement)?.tagName === "INPUT" && e.code !== "Enter" && e.code !== "Escape") return;
        if (e.code === "Escape") this.decline();
        else if (e.code === "Enter" || e.code === "NumpadEnter") this.accept();
        else return;
        e.preventDefault();
        e.stopImmediatePropagation();
      },
      true,
    );
  }

  /** New session. */
  reset(): void {
    this.status = null;
    this.racers = new Float32Array(0);
    this.roaming.clear();
    this.preview = null;
    this.offerEntity = -1;
    this.resultSeq = 0;
    this.resultUntil = 0;
    this.cardKey = "";
    this.legendSet = false;
    this.endPicker();
    this.map.setExtras(null);
    this.map.setLegendExtras("", []);
    for (const el of [this.hint, this.hud, this.count, this.resultEl]) el.classList.add("hidden");
  }

  /** Map colour (CSS) of a driver level, from `street_status().levels` (wp_wasm owns them). */
  private levelColour(level: number, st: StreetStatus | null = this.status): string {
    return st?.levels.find((l) => l.level === level)?.colour ?? "#888888";
  }

  /** Body colour (0xRRGGBB) of a street racer's car, from its level; null for other cars. */
  colourFor(entity: number): number | null {
    if (this.racers.length === 0) this.racers = sim.streetRacers();
    for (let o = 0; o < this.racers.length; o += RACER_STRIDE) {
      if (this.racers[o] !== entity) continue;
      if (!this.status) this.status = sim.streetStatus();
      return parseInt(this.levelColour(this.racers[o + 1]).slice(1), 16);
    }
    return null;
  }

  /** Per frame, after the sim step. `visible` = false while something else owns the screen
   * (game menu, photo mode, crash replay): the street HUD hides; the map keeps its markers. */
  frame(now: number, visible: boolean): void {
    const dt = Math.min(0.1, Math.max(0, (now - this.lastMs) / 1000));
    this.lastMs = now;
    this.root.classList.toggle("hidden", !visible);
    this.racers = sim.streetRacers();
    if (this.frameNo++ % 4 === 0 || !this.status) this.status = sim.streetStatus();
    const st = this.status;
    this.roaming.clear();
    const racing = st?.race?.entity ?? -1;
    for (let o = 0; o < this.racers.length; o += RACER_STRIDE) if (this.racers[o] !== racing) this.roaming.add(this.racers[o]);
    if (!st) {
      if (this.offerEntity >= 0) this.endPicker();
      this.map.setExtras(null);
      for (const el of [this.hint, this.hud, this.count, this.resultEl]) el.classList.add("hidden");
      return;
    }
    if (!this.legendSet) {
      this.legendSet = true;
      this.map.setLegendExtras(
        "Street racers (honk next to one; number = level)",
        st.levels.map((l) => ({ colour: l.colour, label: String(l.level), text: `Lv ${l.level} ${l.name} · ${money(l.bet_min)}–${money(l.bet_max)}` })),
      );
    }
    // A fresh challenge opens the full map as the finish picker.
    if (st.offer && st.offer.entity !== this.offerEntity) {
      this.offerEntity = st.offer.entity;
      this.cardKey = "";
      this.preview = null;
      this.error = "";
      this.stake = Math.max(st.offer.bet_min, Math.min(st.offer.bet_max, Math.round((st.offer.bet_min * 2) / 50) * 50));
      this.map.setPicker((x, y) => this.pick(x, y));
      if (!this.map.isOpen) {
        this.map.open();
        this.openedMap = this.map.isOpen;
      }
    } else if (!st.offer && this.offerEntity >= 0) {
      this.endPicker();
    }
    this.map.setExtras(this.extras(st));
    if (this.map.isOpen) {
      if (st.offer) this.padPicker(dt);
      if (this.frameNo % 8 === 0 || this.cardKey === "") this.renderCard(st);
    }
    this.updateHint(st);
    this.updateHud(st);
    this.updateResult(st, now);
  }

  /** The map overlay this frame: racers, the route and its flags. */
  private extras(st: StreetStatus): MapExtras {
    const markers: MapMarker[] = [];
    const challenged = st.offer?.entity ?? st.race?.entity ?? -1;
    for (let o = 0; o < this.racers.length; o += RACER_STRIDE) {
      const id = this.racers[o];
      const lv = this.racers[o + 1];
      markers.push({
        id,
        x: this.racers[o + 2],
        y: this.racers[o + 3],
        colour: this.levelColour(lv, st),
        label: String(lv),
        hot: this.racers[o + 5] === 2 || id === challenged,
      });
    }
    const race = st.race;
    const route = race?.polyline ?? (st.offer ? (this.preview?.polyline ?? null) : null);
    const flags: MapFlag[] = [];
    const start = race?.start ?? st.offer?.start;
    if (start && this.map.isOpen) flags.push({ x: start[0], y: start[1], label: "A" });
    const finish = race?.finish ?? (st.offer ? this.preview?.finish : undefined);
    if (finish) flags.push({ x: finish[0], y: finish[1], label: "B", finish: true });
    return { markers, route, flags };
  }

  private endPicker(): void {
    this.offerEntity = -1;
    this.preview = null;
    this.error = "";
    this.map.setPicker(null);
    this.padWas = null;
    if (this.openedMap && this.map.isOpen) this.map.close();
    this.openedMap = false;
    this.cardKey = "";
  }

  /** The player picked B (map tap / click, or the pad's crosshair). */
  private pick(x: number, y: number): void {
    if (!this.status?.offer) return;
    try {
      this.preview = sim.streetPreview(x, y);
      this.error = "";
    } catch (err) {
      this.preview = null;
      this.error = String(err).replace(/^.*wp_wasm: /, "");
    }
    this.cardKey = "";
    if (this.status) this.renderCard(this.status);
  }

  /** Accept the open challenge with the picked route and stake (Race button, Enter, pad Menu). */
  accept(): void {
    const o = this.status?.offer;
    const p = this.preview;
    if (!o || !p?.ok) return;
    try {
      sim.streetAccept(p.finish[0], p.finish[1], this.stake);
      this.status = sim.streetStatus();
      this.endPicker();
      if (this.map.isOpen) this.map.close();
    } catch (err) {
      this.error = String(err).replace(/^.*wp_wasm: /, "");
      this.cardKey = "";
      if (this.status) this.renderCard(this.status);
    }
  }

  /** Decline the open challenge (Decline button, Esc, pad B). */
  decline(): void {
    sim.streetDecline();
    this.status = sim.streetStatus();
    this.endPicker();
    if (this.map.isOpen) this.map.close();
  }

  /** Gamepad in the picker: left stick moves the map under the crosshair, A sets B, LB / RB
   * change the stake, Menu races, B declines. */
  private padPicker(dt: number): void {
    const pad = typeof navigator !== "undefined" && navigator.getGamepads ? [...navigator.getGamepads()].find((p) => p?.connected) : null;
    if (!pad) return;
    const dz = (v: number) => (Math.abs(v) < 0.2 ? 0 : (v - Math.sign(v) * 0.2) / 0.8);
    const ax = dz(pad.axes[0] ?? 0);
    const ay = dz(pad.axes[1] ?? 0);
    if (ax || ay) this.map.panBy(-ax * PAD_PAN_PX_S * dt, -ay * PAD_PAN_PX_S * dt);
    const was = this.padWas;
    const now = pad.buttons.map((b) => b.pressed);
    this.padWas = now;
    if (!was) return;
    const press = (i: number) => !!now[i] && !was[i];
    const o = this.status?.offer;
    if (press(PAD.a)) this.map.pickCentre();
    if (press(PAD.b)) return this.decline();
    const dir = (press(PAD.rb) ? 1 : 0) - (press(PAD.lb) ? 1 : 0);
    if (o && dir) {
      this.stake = Math.max(o.bet_min, Math.min(o.bet_max, this.stake + dir * this.stakeStep(o.bet_min)));
      this.cardKey = "";
    }
    if (press(PAD.menu)) this.accept();
  }

  private stakeStep(betMin: number): number {
    return betMin >= 1000 ? 100 : betMin >= 250 ? 50 : 10;
  }

  /** The map's host panel: the challenge card, or the roster when the map is just open. */
  private renderCard(st: StreetStatus): void {
    const el = this.map.panel;
    const o = st.offer;
    const p = this.preview;
    if (el.contains(document.activeElement) && document.activeElement?.tagName === "INPUT") return; // not under the slider
    if (!o) {
      const key = `roster|${st.roster.map((r) => `${r.entity}:${r.mode}:${r.wins}:${r.losses}`).join(",")}`;
      if (key === this.cardKey) return;
      this.cardKey = key;
      el.hidden = false;
      const horn = bound("horn");
      el.innerHTML =
        `<div class="wp-panel-title"><span>Street racers</span><b>${st.roster.length}</b></div>` +
        `<div class="st-row wp-dim">Drive up to a racer and honk (${this.hornLabel(horn)}) to challenge them. Dot colour and number = driver level.</div>` +
        `<ul>${st.roster
          .map(
            (r) =>
              `<li><i class="st-dot" style="background:${r.colour}"></i>${esc(r.name)} <span class="wp-dim">· Lv ${r.level} · ${esc(r.car)} · you ${r.losses}–${r.wins}${r.mode === "Racing" ? " · racing you" : ""}</span></li>`,
          )
          .join("")}</ul>`;
      return;
    }
    const canBet = o.affordable && o.bet_max >= o.bet_min;
    this.stake = Math.max(o.bet_min, Math.min(o.bet_max, this.stake));
    const step = this.stakeStep(o.bet_min);
    const key = `offer|${o.entity}|${p?.finish.join(",")}|${p?.ok}|${this.error}|${this.stake}|${o.cash}|${canBet}`;
    if (key === this.cardKey) return;
    this.cardKey = key;
    el.hidden = false;
    const ready = !!p?.ok && canBet;
    el.innerHTML =
      `<div class="wp-panel-title"><span>Challenge</span><b>Lv ${o.level}</b></div>` +
      `<h3>${esc(o.name)} <span class="st-lv" style="background:${o.colour}">Lv ${o.level} ${esc(o.level_name)}</span></h3>` +
      `<div class="wp-dim st-row">${esc(o.car)} · your record vs them ${o.losses}–${o.wins}</div>` +
      `<div class="st-row">1. <b>Tap / click the map</b> to set the finish <b>B</b> (A is where you are).<br><span class="wp-dim">Pad: left stick moves the map under the crosshair, A sets B.</span></div>` +
      (p
        ? `<div class="st-row">Route <b>${km(p.length_m)}</b>${p.ok ? "" : ` <span class="wp-warn">${esc(p.reason ?? "")}</span>`}</div>`
        : `<div class="st-row wp-dim">No finish picked yet.</div>`) +
      (canBet
        ? `<div class="st-row">2. Bet <b id="st-stake">${money(this.stake)}</b> <span class="wp-dim">(${money(o.bet_min)}–${money(o.bet_max)}, cash ${money(o.cash)}; pad LB / RB)</span>` +
          `<input id="st-range" class="wp-slider" type="range" min="${o.bet_min}" max="${o.bet_max}" step="${step}" value="${this.stake}" /></div>` +
          `<div class="st-row">Win <b class="wp-ok" id="st-win">+${money(this.stake)}</b> · lose <b class="wp-danger" id="st-lose">${money(-this.stake)}</b></div>`
        : `<div class="st-row wp-warn">${esc(o.name)} races for at least ${money(o.bet_min)}; you have ${money(o.cash)}.</div>`) +
      (this.error ? `<div class="st-row wp-warn">${esc(this.error)}</div>` : "") +
      `<div class="st-buttons"><button class="wp-btn wp-btn--primary" id="st-go" ${ready ? "" : "disabled"}>Race <span class="wp-kbd">Enter</span></button>` +
      `<button class="wp-btn wp-btn--ghost" id="st-no">Decline <span class="wp-kbd">Esc</span></button></div>` +
      `<div class="st-row wp-dim">Pad: Menu races, B declines.</div>`;
    const range = el.querySelector<HTMLInputElement>("#st-range");
    if (range) {
      range.oninput = () => {
        this.stake = Number(range.value);
        el.querySelector("#st-stake")!.textContent = money(this.stake);
        el.querySelector("#st-win")!.textContent = `+${money(this.stake)}`;
        el.querySelector("#st-lose")!.textContent = money(-this.stake);
      };
      range.onchange = () => range.blur();
    }
    el.querySelector<HTMLButtonElement>("#st-go")!.onclick = () => this.accept();
    el.querySelector<HTMLButtonElement>("#st-no")!.onclick = () => this.decline();
  }

  private hornLabel(horn = bound("horn")): string {
    const parts = [horn.key && `<span class="wp-kbd">${esc(horn.key)}</span>`, horn.pad && `pad ${esc(horn.pad)}`, this.touch() && "📯"].filter(Boolean);
    return parts.join(" / ") || "horn";
  }

  // ----- HUD --------------------------------------------------------------------------------

  private updateHint(st: StreetStatus): void {
    const h = st.hint;
    const o = st.offer;
    let html = "";
    if (o && !this.map.isOpen) {
      const m = bound("map");
      html = `<b>${esc(o.name)}</b> is waiting: open the map (${[m.key && `<span class="wp-kbd">${esc(m.key)}</span>`, m.pad && `pad ${esc(m.pad)}`].filter(Boolean).join(" / ")}) to pick the finish`;
    } else if (h && !o && !st.race) {
      html = `Honk (${this.hornLabel()}) to challenge <b>${esc(h.name)}</b> <span class="st-lv" style="background:${h.colour}">Lv ${h.level} ${esc(h.level_name)}</span>`;
    }
    this.hint.classList.toggle("hidden", !html || this.map.isOpen);
    if (html && this.hint.innerHTML !== html) this.hint.innerHTML = html;
  }

  private updateHud(st: StreetStatus): void {
    const r = st.race;
    this.hud.classList.toggle("hidden", !r);
    this.count.classList.toggle("hidden", !r || (r.countdown_s <= 0 && r.elapsed_s > 1));
    this.count.classList.toggle("wp-race-go", !!r && r.countdown_s <= 0);
    if (!r) return;
    this.count.textContent = r.countdown_s > 0 ? String(Math.ceil(r.countdown_s)) : "GO";
    const gap = r.racer_left_m - r.player_left_m;
    const lead =
      r.countdown_s > 0
        ? `<span class="wp-dim">get ready</span>`
        : gap >= 0
          ? `<span class="wp-ok">+${Math.round(gap)} m</span>`
          : `<span class="wp-danger">−${Math.round(-gap)} m</span>`;
    this.hud.innerHTML =
      `<section class="wp-panel"><div class="wp-panel-title"><span>Street race</span><b>${money(r.stake)}</b></div>` +
      `<div class="st-row">vs <b>${esc(r.name)}</b> <span class="st-lv" style="background:${r.colour}">Lv ${r.level} ${esc(r.level_name)}</span></div>` +
      `<div class="st-grid wp-mono">` +
      `<div><div class="wp-dim wp-upper" style="font-size:0.68em">${gap >= 0 ? "Ahead" : "Behind"}</div><div style="font-size:1.2em">${lead}</div></div>` +
      `<div><div class="wp-dim wp-upper" style="font-size:0.68em">You → B</div><div style="font-size:1.2em">${km(r.player_left_m)}</div></div>` +
      `<div><div class="wp-dim wp-upper" style="font-size:0.68em">Them → B</div><div style="font-size:1.2em">${km(r.racer_left_m)}</div></div>` +
      `</div><div class="wp-dim" style="font-size:0.8em;margin-top:0.3em">${clock(r.time_left_s)} left · route ${km(r.length_m)}</div></section>`;
  }

  private updateResult(st: StreetStatus, now: number): void {
    const res = st.result;
    if (res && res.seq !== this.resultSeq) {
      this.resultSeq = res.seq;
      this.resultUntil = now + 7000;
      this.resultEl.innerHTML =
        `<b style="color:${res.won ? "#2ecc40" : "#ff4136"}">${res.won ? "YOU WON" : "YOU LOST"} ${res.won ? "+" : ""}${money(res.net)}</b>` +
        `<span>vs ${esc(res.name)} (Lv ${res.level} ${esc(res.level_name)}) · ${esc(res.reason)}` +
        `${res.margin_m > 1 ? ` · by ${km(res.margin_m)}` : ""} · cash ${money(res.cash_after)}</span>`;
    }
    this.resultEl.classList.toggle("hidden", now > this.resultUntil);
  }
}
