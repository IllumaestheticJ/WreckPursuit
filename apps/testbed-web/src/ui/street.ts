// Street racers (wp_wasm::street): NPC racers roam the city; honk (Q / pad Y) next to one to
// challenge it, pick the finish (B) on the map and a bet, race from where you are (A).
//  - minimap (bottom right, heading-up): roads, you, every street racer as a dot in its level
//    colour (number = level), the race route and B. Click it, or press ` (backquote), for
//    the full city map.
//  - full map: north-up city with the level legend; during a challenge it is the picker —
//    click to set B, choose the stake, Race (Enter) / Decline (Esc).
//  - race HUD (top centre): opponent, stake, distance left for both, countdown; result banner.
import { sim, type StreetPreview, type StreetStatus } from "../wasm/sim";

const STYLE = `
#street-mini { position: absolute; right: 8px; bottom: 44px; width: 210px; height: 210px; border-radius: 50%; z-index: 2;
  background: rgba(10, 12, 16, 0.88); box-shadow: 0 0 0 2px #2b3038; pointer-events: auto; cursor: pointer; }
#street-legend { position: absolute; right: 226px; bottom: 44px; display: flex; flex-direction: column; gap: 2px; font-size: 11px;
  background: rgba(0, 0, 0, 0.6); padding: 4px 7px; border-radius: 4px; pointer-events: none; }
#street-legend span i { display: inline-block; width: 9px; height: 9px; border-radius: 50%; margin-right: 3px; vertical-align: -1px; }
#street-hint { position: absolute; left: 50%; bottom: 64px; transform: translateX(-50%); font: 14px/1.3 system-ui, sans-serif;
  background: rgba(0, 0, 0, 0.7); padding: 6px 12px; border-radius: 6px; pointer-events: none; }
#street-hint b { color: #fff; }
#street-hud { position: absolute; left: 50%; top: 92px; transform: translateX(-50%); font: 13px/1.35 system-ui, sans-serif;
  background: rgba(0, 0, 0, 0.72); padding: 6px 12px; border-radius: 6px; text-align: center; pointer-events: none; min-width: 300px; }
#street-hud .ahead { color: #2ecc40; font-weight: 700; } #street-hud .behind { color: #ff4136; font-weight: 700; }
#street-count { position: absolute; left: 50%; top: 38%; transform: translate(-50%, -50%); font: 900 96px/1 system-ui, sans-serif;
  color: #ffdc00; text-shadow: 0 4px 12px #000; pointer-events: none; }
#street-result { position: absolute; left: 50%; top: 30%; transform: translate(-50%, -50%); text-align: center; pointer-events: none;
  text-shadow: 0 2px 8px #000; font-family: system-ui, sans-serif; }
#street-result b { display: block; font: 900 46px/1 system-ui, sans-serif; letter-spacing: 0.08em; }
#street-result span { display: block; margin-top: 6px; font-size: 15px; color: #eee; }
#street-map { position: absolute; inset: 0; background: rgba(0, 0, 0, 0.55); display: flex; align-items: center; justify-content: center;
  gap: 12px; pointer-events: auto; z-index: 6; font: 13px/1.4 system-ui, sans-serif; }
#street-map canvas { background: #0d1015; border-radius: 8px; box-shadow: 0 0 0 2px #2b3038; cursor: crosshair; max-width: 100%; }
#street-map .card { background: rgba(16, 18, 24, 0.96); border: 1px solid #333; border-radius: 8px; padding: 12px 14px; width: 270px; }
#street-map .card h3 { margin: 0 0 6px; font-size: 18px; color: #fff; }
#street-map .lv { display: inline-block; padding: 1px 7px; border-radius: 10px; color: #000; font-weight: 700; font-size: 12px; }
#street-map .row { margin: 8px 0; }
#street-map input[type=range] { width: 100%; }
#street-map button { font: 600 14px system-ui, sans-serif; border-radius: 5px; border: 0; padding: 7px 14px; cursor: pointer; margin-right: 6px; }
#street-map button.go { background: #2ecc40; color: #000; } #street-map button.go:disabled { background: #444; color: #888; cursor: default; }
#street-map button.no { background: #333; color: #ddd; }
#street-map .warn { color: #ffb020; } #street-map .dim { color: #999; }
#street-map ul { margin: 6px 0 0; padding: 0; list-style: none; max-height: 240px; overflow: auto; }
#street-map li i { display: inline-block; width: 10px; height: 10px; border-radius: 50%; margin-right: 6px; vertical-align: -1px; }
`;

const MINI_M_PER_RADIUS = 260; // metres from the centre to the rim of the minimap
const money = (n: number) => (n < 0 ? "−$" : "$") + Math.abs(Math.round(n)).toLocaleString("en-US");
const km = (m: number) => (m >= 1000 ? `${(m / 1000).toFixed(2)} km` : `${Math.round(m)} m`);
const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;");

interface Lanes {
  off: HTMLCanvasElement;
  minX: number;
  maxY: number;
  w: number;
  h: number;
  /** offscreen px per metre */
  k: number;
}

export class StreetUi {
  private readonly mini: HTMLCanvasElement;
  private readonly legend: HTMLElement;
  private readonly hint: HTMLElement;
  private readonly hud: HTMLElement;
  private readonly count: HTMLElement;
  private readonly resultEl: HTMLElement;
  private readonly mapEl: HTMLElement;
  private readonly big: HTMLCanvasElement;
  private readonly card: HTMLElement;
  private lanes: Lanes | null = null;
  private racers: Float32Array = new Float32Array(0);
  /** Latest status (refreshed a few times a second); read by e2e. */
  status: StreetStatus | null = null;
  private mapOpen = false;
  private preview: StreetPreview | null = null;
  private stake = 0;
  private offerEntity = -1;
  private resultSeq = 0;
  private resultUntil = 0;
  private frameNo = 0;
  private player: { x: number; y: number; yaw: number } | null = null;
  private error = "";

  constructor(parent: HTMLElement) {
    const style = document.createElement("style");
    style.textContent = STYLE;
    document.head.appendChild(style);
    const div = (id: string) => {
      const el = document.createElement("div");
      el.id = id;
      el.className = "hidden";
      parent.appendChild(el);
      return el;
    };
    this.mini = document.createElement("canvas");
    this.mini.id = "street-mini";
    this.mini.width = this.mini.height = 210 * devicePixelRatio;
    this.mini.className = "hidden";
    this.mini.title = "city map (`)";
    this.mini.onclick = () => this.toggleMap();
    parent.appendChild(this.mini);
    this.legend = div("street-legend");
    this.hint = div("street-hint");
    this.hud = div("street-hud");
    this.count = div("street-count");
    this.resultEl = div("street-result");
    this.mapEl = div("street-map");
    this.big = document.createElement("canvas");
    this.card = document.createElement("div");
    this.card.className = "card";
    this.mapEl.append(this.big, this.card);
    this.big.addEventListener("click", (e) => this.onMapClick(e));
    this.mapEl.addEventListener("click", (e) => {
      if (e.target === this.mapEl && !this.status?.offer) this.setMap(false);
    });
    window.addEventListener(
      "keydown",
      (e) => {
        if (e.repeat || (e.target as HTMLElement)?.tagName === "INPUT") return;
        if (e.code === "Backquote") {
          this.toggleMap();
          e.preventDefault();
        } else if (this.mapOpen && e.code === "Escape") {
          if (this.status?.offer) this.decline();
          else this.setMap(false);
        } else if (this.mapOpen && e.code === "Enter" && this.status?.offer) {
          this.accept();
          e.preventDefault();
        }
      },
      true,
    );
  }

  /** New session: fetch the road layer once. */
  reset(): void {
    this.lanes = null;
    this.status = null;
    this.racers = new Float32Array(0) as Float32Array;
    this.legend.innerHTML = "";
    this.preview = null;
    this.offerEntity = -1;
    this.resultSeq = 0;
    this.resultUntil = 0;
    this.setMap(false);
    let flat: Float32Array;
    try {
      flat = sim.mapLanes();
    } catch {
      flat = new Float32Array(0);
    }
    if (flat.length === 0) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (let o = 0; o < flat.length; ) {
      const n = flat[o++];
      for (let i = 0; i < n; i++, o += 2) {
        minX = Math.min(minX, flat[o]);
        maxX = Math.max(maxX, flat[o]);
        minY = Math.min(minY, flat[o + 1]);
        maxY = Math.max(maxY, flat[o + 1]);
      }
    }
    minX -= 40, minY -= 40, maxX += 40, maxY += 40;
    const k = Math.min(1.5, 2048 / Math.max(maxX - minX, maxY - minY));
    const off = document.createElement("canvas");
    off.width = Math.ceil((maxX - minX) * k);
    off.height = Math.ceil((maxY - minY) * k);
    const c = off.getContext("2d")!;
    c.strokeStyle = "#4b5361";
    c.lineCap = "round";
    c.lineJoin = "round";
    c.lineWidth = Math.max(1.5, 4.2 * k);
    c.beginPath();
    for (let o = 0; o < flat.length; ) {
      const n = flat[o++];
      for (let i = 0; i < n; i++, o += 2) {
        const x = (flat[o] - minX) * k;
        const y = (maxY - flat[o + 1]) * k;
        if (i === 0) c.moveTo(x, y);
        else c.lineTo(x, y);
      }
    }
    c.stroke();
    this.lanes = { off, minX, maxY, w: maxX - minX, h: maxY - minY, k };
  }

  /** Body colour (0xRRGGBB) of a street racer's car, from its level; null for other cars. */
  colourFor(entity: number): number | null {
    if (this.racers.length === 0) this.racers = sim.streetRacers();
    for (let o = 0; o < this.racers.length; o += 6) {
      if (this.racers[o] === entity) {
        const lv = this.status?.levels[this.racers[o + 1] - 1] ?? null;
        return parseInt((lv?.colour ?? LEVEL_FALLBACK[this.racers[o + 1] - 1] ?? "#888888").slice(1), 16);
      }
    }
    return null;
  }

  /** Per frame. `player` = the player's world position and yaw (null = no car). */
  frame(now: number, player: { x: number; y: number; yaw: number } | null): void {
    this.player = player;
    this.racers = sim.streetRacers();
    if (this.frameNo++ % 4 === 0) this.status = sim.streetStatus();
    const st = this.status;
    const on = st !== null && this.lanes !== null;
    this.mini.classList.toggle("hidden", !on);
    this.legend.classList.toggle("hidden", !on);
    if (!on || !st) {
      for (const el of [this.hint, this.hud, this.count, this.resultEl]) el.classList.add("hidden");
      return;
    }
    if (this.legend.childElementCount === 0) {
      this.legend.innerHTML = st.levels.map((l) => `<span><i style="background:${l.colour}"></i>${l.level} ${esc(l.name)}</span>`).join("") + `<span class="dim">\` map</span>`;
    }
    // A fresh challenge opens the map as the finish picker.
    if (st.offer && st.offer.entity !== this.offerEntity) {
      this.offerEntity = st.offer.entity;
      this.preview = null;
      this.error = "";
      this.stake = Math.max(st.offer.bet_min, Math.min(st.offer.bet_max, Math.round((st.offer.bet_min * 2) / 50) * 50));
      this.setMap(true);
    } else if (!st.offer && this.offerEntity >= 0) {
      this.offerEntity = -1;
      this.preview = null;
      if (this.mapOpen) this.setMap(false);
    }
    this.drawMini();
    if (this.mapOpen) this.drawBig();
    this.updateHint(st);
    this.updateHud(st);
    this.updateResult(st, now);
  }

  private toggleMap(): void {
    if (this.status?.offer) return; // the picker stays until accepted / declined
    this.setMap(!this.mapOpen);
  }

  private setMap(open: boolean): void {
    this.mapOpen = open && this.lanes !== null;
    this.mapEl.classList.toggle("hidden", !this.mapOpen);
    if (this.mapOpen) {
      const side = Math.floor(Math.min(innerHeight - 40, innerWidth - 320));
      this.big.width = this.big.height = side * devicePixelRatio;
      this.big.style.width = this.big.style.height = `${side}px`;
      this.renderCard();
    }
  }

  // ----- minimap ----------------------------------------------------------------------------

  private drawMini(): void {
    const L = this.lanes!;
    const c = this.mini.getContext("2d")!;
    const W = this.mini.width;
    const R = W / 2;
    const s = R / MINI_M_PER_RADIUS; // px per metre
    c.clearRect(0, 0, W, W);
    const p = this.player;
    if (!p) return;
    c.save();
    c.beginPath();
    c.arc(R, R, R - 1, 0, Math.PI * 2);
    c.clip();
    // Heading-up: rotate the world so the player's forward points up.
    c.save();
    c.translate(R, R);
    c.rotate(p.yaw);
    c.scale(s / L.k, s / L.k);
    c.translate(-(p.x - L.minX) * L.k, -(L.maxY - p.y) * L.k);
    c.drawImage(L.off, 0, 0);
    c.restore();
    const toMini = (x: number, y: number): [number, number] => {
      const rx = (x - p.x) * s;
      const ry = -(y - p.y) * s;
      const cs = Math.cos(p.yaw), sn = Math.sin(p.yaw);
      return [R + rx * cs - ry * sn, R + rx * sn + ry * cs];
    };
    const st = this.status!;
    const route = st.race?.polyline ?? (st.offer ? this.preview?.polyline : null);
    if (route && route.length > 1) {
      c.strokeStyle = "#ffdc00";
      c.lineWidth = 3 * devicePixelRatio;
      c.beginPath();
      route.forEach(([x, y], i) => (i ? c.lineTo(...toMini(x, y)) : c.moveTo(...toMini(x, y))));
      c.stroke();
    }
    const finish = st.race?.finish ?? (st.offer ? this.preview?.finish : null);
    c.restore();
    if (finish) this.flag(c, clampRim(toMini(finish[0], finish[1]), R, 10 * devicePixelRatio), 1);
    // Racers: level colour + number; clamped to the rim when out of range.
    for (let o = 0; o < this.racers.length; o += 6) {
      const [ex, ey] = toMini(this.racers[o + 2], this.racers[o + 3]);
      const out = Math.hypot(ex - R, ey - R) > R - 9 * devicePixelRatio;
      const at = out ? clampRim([ex, ey], R, 9 * devicePixelRatio) : ([ex, ey] as [number, number]);
      this.racerDot(c, at, this.racers[o + 1], this.racers[o + 5], out ? 0.7 : 1);
    }
    // You.
    c.save();
    c.translate(R, R);
    this.arrow(c, devicePixelRatio);
    c.restore();
  }

  private racerDot(c: CanvasRenderingContext2D, [x, y]: [number, number], level: number, mode: number, scale: number): void {
    const lv = this.status?.levels[level - 1];
    const r = (mode === 2 ? 8 : 6.5) * devicePixelRatio * scale;
    c.beginPath();
    c.arc(x, y, r, 0, Math.PI * 2);
    c.fillStyle = lv?.colour ?? "#888";
    c.fill();
    c.lineWidth = (mode > 0 ? 2.5 : 1.5) * devicePixelRatio;
    c.strokeStyle = mode === 2 ? "#ffdc00" : "#fff";
    c.stroke();
    c.fillStyle = "#000";
    c.font = `700 ${Math.round(9 * devicePixelRatio * scale)}px system-ui, sans-serif`;
    c.textAlign = "center";
    c.textBaseline = "middle";
    c.fillText(String(level), x, y + 0.5);
  }

  private arrow(c: CanvasRenderingContext2D, k: number): void {
    c.beginPath();
    c.moveTo(0, -9 * k);
    c.lineTo(6 * k, 7 * k);
    c.lineTo(0, 3.5 * k);
    c.lineTo(-6 * k, 7 * k);
    c.closePath();
    c.fillStyle = "#fff";
    c.fill();
    c.lineWidth = 1.5 * k;
    c.strokeStyle = "#000";
    c.stroke();
  }

  private flag(c: CanvasRenderingContext2D, [x, y]: [number, number], k: number): void {
    const d = devicePixelRatio * k;
    c.fillStyle = "#000";
    c.fillRect(x - 1 * d, y - 14 * d, 2 * d, 14 * d);
    for (let i = 0; i < 3; i++)
      for (let j = 0; j < 2; j++) {
        c.fillStyle = (i + j) % 2 ? "#000" : "#fff";
        c.fillRect(x + 1 * d + i * 3.5 * d, y - 14 * d + j * 3.5 * d, 3.5 * d, 3.5 * d);
      }
  }

  // ----- full map / challenge picker --------------------------------------------------------

  private bigTransform(): { k: number; ox: number; oy: number } {
    const L = this.lanes!;
    const W = this.big.width;
    const k = (W - 20) / Math.max(L.w, L.h);
    return { k, ox: (W - L.w * k) / 2, oy: (W - L.h * k) / 2 };
  }

  private drawBig(): void {
    const L = this.lanes!;
    const c = this.big.getContext("2d")!;
    const W = this.big.width;
    const { k, ox, oy } = this.bigTransform();
    const to = (x: number, y: number): [number, number] => [ox + (x - L.minX) * k, oy + (L.maxY - y) * k];
    c.clearRect(0, 0, W, W);
    c.drawImage(L.off, ox, oy, L.w * k, L.h * k);
    const st = this.status!;
    const route = st.race?.polyline ?? this.preview?.polyline;
    if (route && route.length > 1) {
      c.strokeStyle = "#ffdc00";
      c.lineWidth = 3.5 * devicePixelRatio;
      c.beginPath();
      route.forEach(([x, y], i) => (i ? c.lineTo(...to(x, y)) : c.moveTo(...to(x, y))));
      c.stroke();
    }
    const start = st.race?.start ?? st.offer?.start;
    if (start) {
      const [x, y] = to(start[0], start[1]);
      c.fillStyle = "#fff";
      c.font = `800 ${Math.round(13 * devicePixelRatio)}px system-ui, sans-serif`;
      c.textAlign = "center";
      c.fillText("A", x, y - 12 * devicePixelRatio);
    }
    const finish = st.race?.finish ?? this.preview?.finish;
    if (finish) {
      const [x, y] = to(finish[0], finish[1]);
      this.flag(c, [x, y], 1.4);
      c.fillStyle = "#fff";
      c.font = `800 ${Math.round(13 * devicePixelRatio)}px system-ui, sans-serif`;
      c.fillText("B", x - 8 * devicePixelRatio, y - 18 * devicePixelRatio);
    }
    for (let o = 0; o < this.racers.length; o += 6) {
      this.racerDot(c, to(this.racers[o + 2], this.racers[o + 3]), this.racers[o + 1], this.racers[o + 5], 1.15);
    }
    if (this.player) {
      c.save();
      c.translate(...to(this.player.x, this.player.y));
      c.rotate(-this.player.yaw);
      this.arrow(c, devicePixelRatio * 1.1);
      c.restore();
    }
    if (this.frameNo % 8 === 0) this.renderCard();
  }

  private onMapClick(e: MouseEvent): void {
    const st = this.status;
    if (!st?.offer || !this.lanes) return;
    const rect = this.big.getBoundingClientRect();
    const px = ((e.clientX - rect.left) / rect.width) * this.big.width;
    const py = ((e.clientY - rect.top) / rect.height) * this.big.height;
    const { k, ox, oy } = this.bigTransform();
    const x = this.lanes.minX + (px - ox) / k;
    const y = this.lanes.maxY - (py - oy) / k;
    try {
      this.preview = sim.streetPreview(x, y);
      this.error = "";
    } catch (err) {
      this.preview = null;
      this.error = String(err).replace(/^.*wp_wasm: /, "");
    }
    this.renderCard();
  }

  private accept(): void {
    const o = this.status?.offer;
    const p = this.preview;
    if (!o || !p?.ok) return;
    try {
      sim.streetAccept(p.finish[0], p.finish[1], this.stake);
      this.error = "";
      this.status = sim.streetStatus();
      this.offerEntity = -1;
      this.preview = null;
      this.setMap(false);
    } catch (err) {
      this.error = String(err).replace(/^.*wp_wasm: /, "");
      this.renderCard();
    }
  }

  private decline(): void {
    sim.streetDecline();
    this.status = sim.streetStatus();
    this.offerEntity = -1;
    this.preview = null;
    this.setMap(false);
  }

  private renderCard(): void {
    const st = this.status;
    if (!st) return;
    const o = st.offer;
    const focused = document.activeElement === this.card.querySelector("input");
    if (focused) return; // do not rebuild the slider under the pointer
    if (!o) {
      this.card.innerHTML =
        `<h3>Street racers</h3><div class="dim">Honk (Q) next to a racer to challenge them. Dot colour = driver level.</div>` +
        `<ul>${st.levels.map((l) => `<li><i style="background:${l.colour}"></i><b>${l.level} ${esc(l.name)}</b> <span class="dim">bets ${money(l.bet_min)}–${money(l.bet_max)}</span></li>`).join("")}</ul>` +
        `<div class="row"><b>Roster</b></div><ul>${st.roster
          .map((r) => `<li><i style="background:${r.colour}"></i>${esc(r.name)} <span class="dim">· ${esc(r.car)} · you ${r.losses}–${r.wins}${r.mode === "Racing" ? " · racing you" : ""}</span></li>`)
          .join("")}</ul>` +
        `<div class="row dim">\` or Esc closes the map</div>`;
      return;
    }
    const p = this.preview;
    const canBet = o.affordable && o.bet_max >= o.bet_min;
    this.stake = Math.max(o.bet_min, Math.min(o.bet_max, this.stake));
    const step = o.bet_min >= 1000 ? 100 : o.bet_min >= 250 ? 50 : 10;
    this.card.innerHTML =
      `<h3>${esc(o.name)} <span class="lv" style="background:${o.colour}">Lv ${o.level} ${esc(o.level_name)}</span></h3>` +
      `<div class="dim">${esc(o.car)} · your record vs them ${o.losses}–${o.wins}</div>` +
      `<div class="row">1. <b>Click the map</b> to set the finish <b>B</b> (A is where you are now).</div>` +
      (p
        ? `<div class="row">Route ${km(p.length_m)}${p.ok ? "" : ` <span class="warn">${esc(p.reason ?? "")}</span>`}</div>`
        : `<div class="row dim">no finish picked yet</div>`) +
      (canBet
        ? `<div class="row">2. Bet <b id="st-stake">${money(this.stake)}</b> <span class="dim">(${money(o.bet_min)}–${money(o.bet_max)}, cash ${money(o.cash)})</span>` +
          `<input id="st-range" type="range" min="${o.bet_min}" max="${o.bet_max}" step="${step}" value="${this.stake}" /></div>` +
          `<div class="row">Win: <b style="color:#2ecc40">+${money(this.stake)}</b> · lose: <b style="color:#ff4136">${money(-this.stake)}</b></div>`
        : `<div class="row warn">${esc(o.name)} races for at least ${money(o.bet_min)}; you have ${money(o.cash)}.</div>`) +
      (this.error ? `<div class="row warn">${esc(this.error)}</div>` : "") +
      `<div class="row"><button class="go" id="st-go" ${p?.ok && canBet ? "" : "disabled"}>Race! (Enter)</button><button class="no" id="st-no">Decline (Esc)</button></div>`;
    const range = this.card.querySelector<HTMLInputElement>("#st-range");
    if (range) {
      range.oninput = () => {
        this.stake = Number(range.value);
        this.card.querySelector("#st-stake")!.textContent = money(this.stake);
        this.card.querySelectorAll<HTMLElement>(".row b[style]").forEach((b, i) => (b.textContent = i === 0 ? `+${money(this.stake)}` : money(-this.stake)));
      };
      range.onchange = () => range.blur();
    }
    this.card.querySelector<HTMLButtonElement>("#st-go")!.onclick = () => this.accept();
    this.card.querySelector<HTMLButtonElement>("#st-no")!.onclick = () => this.decline();
  }

  // ----- HUD --------------------------------------------------------------------------------

  private updateHint(st: StreetStatus): void {
    const h = st.hint;
    this.hint.classList.toggle("hidden", !h || this.mapOpen);
    if (h) {
      this.hint.innerHTML = `<b>Q</b> honk to challenge <b>${esc(h.name)}</b> <span class="lv" style="background:${h.colour};color:#000;border-radius:10px;padding:0 6px;font-weight:700">Lv ${h.level} ${esc(h.level_name)}</span>`;
    }
  }

  private updateHud(st: StreetStatus): void {
    const r = st.race;
    this.hud.classList.toggle("hidden", !r);
    this.count.classList.toggle("hidden", !r || (r.countdown_s <= 0 && r.elapsed_s > 1));
    if (!r) return;
    this.count.textContent = r.countdown_s > 0 ? String(Math.ceil(r.countdown_s)) : "GO!";
    const gap = r.racer_left_m - r.player_left_m;
    const lead =
      r.countdown_s > 0
        ? `<span class="dim">get ready…</span>`
        : gap >= 0
          ? `<span class="ahead">AHEAD ${Math.round(gap)} m</span>`
          : `<span class="behind">BEHIND ${Math.round(-gap)} m</span>`;
    this.hud.innerHTML =
      `vs <b>${esc(r.name)}</b> <span style="color:${r.colour}">Lv ${r.level} ${esc(r.level_name)}</span> · <b>${money(r.stake)}</b> on the line<br>` +
      `${lead} · you ${km(r.player_left_m)} to B · them ${km(r.racer_left_m)}` +
      ` · <span class="dim">${Math.floor(r.time_left_s / 60)}:${String(Math.floor(r.time_left_s % 60)).padStart(2, "0")} left</span>`;
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

/** Level colours before the first status arrives (same as wp_wasm::street::LEVELS). */
const LEVEL_FALLBACK = ["#3fb950", "#2f81f7", "#a371f7", "#f0883e", "#f85149"];

function clampRim([x, y]: [number, number], R: number, inset: number): [number, number] {
  const dx = x - R, dy = y - R;
  const d = Math.hypot(dx, dy);
  const max = R - inset;
  return d <= max ? [x, y] : [R + (dx / d) * max, R + (dy / d) * max];
}
