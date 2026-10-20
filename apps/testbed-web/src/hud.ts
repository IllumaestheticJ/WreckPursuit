// DOM/canvas HUD: speed, rpm bar, gear, mode + assist badges, perf, damage silhouette,
// component health, region energy, input display with rack-torque bar, help overlay.
import { AUDIO_CREDITS } from "./audio/credits";
import { T } from "./wasm/layout";
import type { AssistProfile, DriverInput, SimEvent, VehicleInfo } from "./wasm/sim";
import { layers, LAYERS } from "./settings";
import { CAMERA_HELP } from "./cameras";
import { heat, healthColour } from "./vehicle_view";
import { RTF_WARN } from "./rtf";

/** Region energy (J) shown as fully red in the silhouette / panel. */
const ENERGY_RED = 5000;

const el = (tag: string, cls: string, parent: HTMLElement, html = ""): HTMLElement => {
  const e = document.createElement(tag);
  e.className = cls;
  e.innerHTML = html;
  parent.appendChild(e);
  return e;
};

const bar = (frac: number, colour: string, centred = false): string => {
  const f = Math.max(-1, Math.min(1, frac));
  if (centred) {
    const left = f < 0 ? 50 + f * 50 : 50;
    return `<span class="bar"><i style="left:${left}%;width:${Math.abs(f) * 50}%;background:${colour}"></i><b></b></span>`;
  }
  return `<span class="bar"><i style="width:${Math.max(0, f) * 100}%;background:${colour}"></i></span>`;
};

/** Fields the event feed leaves out: who (shown separately), positions and raw payloads. */
const FEED_SKIP = new Set(["vehicle", "pos", "heading", "payload"]);

/** One event's fields as `key=value` pairs. Single-key wrapper objects (serde enum variants such
 * as `{ Radio: { text } }` or `kind: { Speeding: { over_mps } }`) are unwrapped into their name
 * and fields, so the feed reads `Radio unit=1 text=…` instead of raw JSON. */
export function feedDetail(body: Record<string, unknown>): string {
  const fmt = (v: unknown): string => {
    if (typeof v === "number") return String(Number(v.toFixed(2)));
    if (typeof v === "string") return v;
    if (v && typeof v === "object" && !Array.isArray(v)) {
      const entries = Object.entries(v as Record<string, unknown>);
      if (entries.length === 1 && entries[0][1] && typeof entries[0][1] === "object" && !Array.isArray(entries[0][1])) {
        const inner = feedDetail(entries[0][1] as Record<string, unknown>);
        return inner ? `${entries[0][0]} ${inner}` : entries[0][0];
      }
      return feedDetail(v as Record<string, unknown>);
    }
    return JSON.stringify(v);
  };
  return Object.entries(body)
    .filter(([k]) => !FEED_SKIP.has(k))
    .map(([k, v]) => {
      if (v && typeof v === "object" && !Array.isArray(v)) {
        // An enum variant (`{ Radio: { … } }`; serde names variants in PascalCase): name, then fields.
        if (/^[A-Z]/.test(k)) return `${k} ${fmt(v)}`.trim();
        // A field holding an enum variant (`kind: { Speeding: { … } }`): drop the `key=` prefix.
        if (Object.keys(v).length === 1 && /^[A-Z]/.test(Object.keys(v)[0])) return fmt(v);
      }
      return `${k}=${fmt(v)}`;
    })
    .join(" ");
}

export interface HudFrame {
  info: VehicleInfo | null;
  tel: Float32Array | null;
  input: DriverInput;
  source: string;
  assists: AssistProfile | null;
  mode: string;
  rack: number;
  fps: number;
  simMs: number;
  renderMs: number;
  /** JS time between the sim step and the render call (views, HUD, scene updates), ms. */
  viewsMs: number;
  /** Real-time factor: sim seconds per wall second (1 = real time). */
  rtf: number;
  /** Graphics quality label (render/quality.ts). */
  gfx?: string;
  tick: number;
  hours: number;
  hash: string;
  vehicles: number;
  replay: boolean;
  recording: boolean;
  energy: Float32Array | null;
  regionsOfNodes: Uint8Array | null;
  rest: Float32Array | null;
  camera: string;
}

export class Hud {
  private readonly main: HTMLElement;
  private readonly comps: HTMLElement;
  private readonly regions: HTMLElement;
  private readonly feed: HTMLElement;
  private readonly feedLines: string[] = [];
  private readonly inputs: HTMLElement;
  private readonly help: HTMLElement;
  private readonly silhouette: HTMLCanvasElement;

  constructor(root: HTMLElement) {
    const left = el("div", "left-col", root);
    this.main = el("div", "panel hud-main", left);
    const right = el("div", "right-col", root);
    this.silhouette = document.createElement("canvas");
    this.silhouette.width = 120;
    this.silhouette.height = 220;
    this.silhouette.className = "panel silhouette";
    right.appendChild(this.silhouette);
    this.comps = el("div", "panel comps", right);
    this.regions = el("div", "panel regions", right);
    this.feed = el("div", "panel feed", right);
    this.inputs = el("div", "panel inputs", left);
    this.help = el("div", "panel help hidden", root);
    this.setControlsHelp([]);
  }

  /** Rebuild the help overlay with the current bindings (one line per action). */
  setControlsHelp(bindingLines: string[]): void {
    this.help.innerHTML =
      `<b>WreckPursuit testbed — controls</b> <span class="dim">(change them in the controls panel: F9 by default, or the “controls” button)</span><br>` +
      `<div class="help-binds">${bindingLines.join("<br>")}</div>` +
      `<br>Inside the city map: drag / wheel / arrows / + − / 0 whole city / F follow (driving input is paused while it is open)<br>` +
      `<b>Cameras</b> ${CAMERA_HELP}<br>` +
      `. frame step (replay) · [ / ] replay speed<br><br>` +
      `<b>Slide control</b> (keyboard/gamepad assists): the car catches power slides itself — ` +
      `centre the steering and it straightens up; steer into the turn to hold a drift, ` +
      `countersteer to straighten faster. Strongest on powerful RWD cars; off on wheel.<br><br>` +
      `<b>Debug layers</b><br>` +
      LAYERS.map((l) => `${l.key.replace(/^(Key|Digit)/, "")} — ${l.label}`).join("<br>") +
      `<br><br>${AUDIO_CREDITS}`;
  }

  /** Append notable sim events (damage, detachment, violations) to the event feed. */
  pushEvents(events: SimEvent[], tick: number): void {
    for (const ev of events) {
      const [kind, body] = Object.entries(ev)[0] ?? [];
      if (!kind || kind === "Contact" || kind === "LatticeDeformed") continue;
      const b = (body ?? {}) as Record<string, unknown>;
      const who = b.vehicle !== undefined ? `#${b.vehicle}` : "";
      this.feedLines.unshift(`${(tick / 240).toFixed(2)}s ${kind} ${who} ${feedDetail(b)}`);
    }
    this.feedLines.length = Math.min(this.feedLines.length, 12);
  }

  /** Add one line of text to the top of the event feed. */
  note(text: string): void {
    this.feedLines.unshift(text);
    this.feedLines.length = Math.min(this.feedLines.length, 12);
  }

  clearEvents(): void {
    this.feedLines.length = 0;
  }

  toggleHelp(): void {
    this.help.classList.toggle("hidden");
  }

  update(f: HudFrame): void {
    const t = f.tel;
    const speed = t ? t[T.speed] * 3.6 : 0;
    const rpm = t ? t[T.rpm] : 0;
    const gear = t ? t[T.gear] : 0;
    const redline = f.info?.redline_rpm ?? 7000;
    const limit = f.info?.rev_limit_rpm ?? 7500;
    const gearTxt = gear < 0 ? "R" : gear === 0 ? "N" : String(gear);
    const a = f.assists;
    const badge = (on: boolean, txt: string) => `<span class="badge ${on ? "on" : "off"}">${txt}</span>`;
    const hh = Math.floor(f.hours);
    const mm = Math.floor((f.hours - hh) * 60);
    this.main.innerHTML =
      `<div class="speed">${speed.toFixed(0)}<small> km/h</small> <span class="gear">${gearTxt}</span></div>` +
      `<div>rpm ${rpm.toFixed(0)} ${bar(rpm / limit, rpm > redline ? "#ff4040" : "#40c0ff")}</div>` +
      `<div>${badge(true, f.mode)}${f.replay ? badge(true, "REPLAY") : ""}${f.recording ? '<span class="badge rec">● REC</span>' : ""}</div>` +
      (a
        ? `<div>${badge(a.abs, "ABS")}${badge(a.tcs, "TCS")}${badge(a.esc, "ESC")}${badge(a.steer_speed_sensitivity > 0, "STEER")}` +
          `${badge(a.countersteer_help > 0, "CSTR")}${badge(a.throttle_filter > 0, "THR-F")}${badge(a.auto_gears, "AUTO")}${badge(a.auto_clutch, a.auto_clutch ? "A-CLU" : "M-CLU")}</div>`
        : "") +
      `<div class="dim">${f.fps.toFixed(0)} fps · sim ${f.simMs.toFixed(2)} ms · views ${f.viewsMs.toFixed(1)} ms · render ${f.renderMs.toFixed(2)} ms · ${f.vehicles} car(s)</div>` +
      (f.rtf < RTF_WARN
        ? `<div class="rtf-warn">⚠ slow motion: ${f.rtf.toFixed(2)}× real time (browser can't keep up)</div>`
        : `<div class="dim">real time ${f.rtf.toFixed(2)}×</div>`) +
      (f.gfx ? `<div class="dim">gfx ${f.gfx}</div>` : "") +
      `<div class="dim">tick ${f.tick} · ${String(hh).padStart(2, "0")}:${String(mm).padStart(2, "0")} · cam ${f.camera} · hash ${f.hash}</div>` +
      `<div class="dim">health ${t ? (t[T.health] * 100).toFixed(0) : "–"}% · ${f.info?.display_name ?? ""} · F1 help</div>`;

    this.drawSilhouette(f);

    this.comps.style.display = layers.on("components") ? "" : "none";
    if (layers.on("components") && t && f.info) {
      const rows = f.info.components
        .map((name, i) => ({ name, h: t[T.componentBase + i], mounted: f.info!.mounted_components.includes(name) }))
        .filter((c) => c.mounted || c.h < 1);
      this.comps.innerHTML =
        `<b>components</b>` +
        rows.map((c) => `<div>${c.name.padEnd(13, " ")}${bar(c.h, `#${healthColour(c.h).getHexString()}`)}</div>`).join("");
    }

    this.regions.style.display = layers.on("regions") ? "" : "none";
    if (layers.on("regions") && f.energy && f.info) {
      this.regions.innerHTML =
        `<b>region energy (J, elastic proxy)</b>` +
        f.info.regions
          .map((r, i) => `<div>${r.padEnd(17, " ")}${f.energy![i].toFixed(0).padStart(6, " ")}</div>`)
          .join("");
    }

    this.feed.style.display = layers.on("events") && this.feedLines.length ? "" : "none";
    if (layers.on("events")) {
      this.feed.innerHTML =
        "<b>events</b>" + this.feedLines.map((l) => `<div>${l.replace(/</g, "&lt;")}</div>`).join("");
    }

    this.inputs.style.display = layers.on("inputs") ? "" : "none";
    if (layers.on("inputs")) {
      const i = f.input;
      this.inputs.innerHTML =
        `<b>input (${f.source})</b>` +
        `<div>steer${bar(i.steer, "#7fdbff", true)}</div>` +
        `<div>thr  ${bar(i.throttle, "#2ecc40")}</div>` +
        `<div>brk  ${bar(i.brake, "#ff4136")}</div>` +
        `<div>hbk  ${bar(i.handbrake, "#ff851b")}</div>` +
        `<div>clt  ${bar(i.clutch, "#aaaaaa")}</div>` +
        `<div title="Browser has no FFB: rack torque shown as a bar (±20 N·m)">rack${bar(f.rack / 20, "#f012be", true)} ${f.rack.toFixed(1)} N·m</div>`;
    }
  }

  /** Top-down damage silhouette: each region's node bounding box coloured by its energy. */
  private drawSilhouette(f: HudFrame): void {
    const c = this.silhouette.getContext("2d")!;
    c.clearRect(0, 0, this.silhouette.width, this.silhouette.height);
    c.fillStyle = "rgba(0,0,0,0.55)";
    c.fillRect(0, 0, this.silhouette.width, this.silhouette.height);
    if (!f.rest || !f.regionsOfNodes || !f.info) return;
    const [w, l] = f.info.body_size_m;
    const sx = (this.silhouette.width - 20) / w;
    const sy = (this.silhouette.height - 30) / l;
    const s = Math.min(sx, sy);
    const cx = this.silhouette.width / 2;
    const cy = this.silhouette.height / 2 + 6;
    const boxes = new Map<number, [number, number, number, number]>();
    for (let n = 0; n < f.regionsOfNodes.length; n++) {
      const r = f.regionsOfNodes[n];
      const x = f.rest[n * 3];
      const y = f.rest[n * 3 + 1];
      const b = boxes.get(r) ?? [x, y, x, y];
      boxes.set(r, [Math.min(b[0], x), Math.min(b[1], y), Math.max(b[2], x), Math.max(b[3], y)]);
    }
    c.font = "10px monospace";
    c.fillStyle = "#ccc";
    c.fillText("damage (top view)", 4, 11);
    for (const [r, [x0, y0, x1, y1]] of boxes) {
      const e = f.energy ? f.energy[r] : 0;
      c.fillStyle = e > 1 ? `#${heat(e / ENERGY_RED).getHexString()}` : "#6a7078";
      c.globalAlpha = e > 1 ? 0.85 : 0.35;
      const pad = 0.12;
      c.fillRect(cx + (x0 - pad) * s, cy - (y1 + pad) * s, (x1 - x0 + 2 * pad) * s, (y1 - y0 + 2 * pad) * s);
    }
    c.globalAlpha = 1;
    c.strokeStyle = "#888";
    c.strokeRect(cx - (w / 2) * s, cy - (l / 2) * s, w * s, l * s);
    c.fillStyle = "#ffee88";
    c.fillRect(cx - 6, cy - (l / 2) * s - 6, 12, 4); // nose marker
  }
}
