// Audio debug panel: master + per-bus sliders, mute (M), scanner tier, siren mode for the
// focused car, test radio lines, voice/culling counters and the radio log.

import { BUSES, type Bus, type DirectorStats, type EngineVoicing, type RadioPriority, type SampleStatus, type SoundSource } from "./types";
import type { RadioLog } from "./scanner";

const STORE_KEY = "wp.testbed.audio.v1";

export interface PanelSettings {
  master: number;
  buses: Record<Bus, number>;
  tier: string;
  tts: boolean;
  open: boolean;
  /** Recorded samples (default) or the procedural fallback packs. */
  source: SoundSource;
  /** City ambience bed gain (0 = off). */
  ambience: number;
  /** Modelled engine synthesizer (default) or layers for every car. */
  engine: EngineVoicing;
}

function loadSettings(): PanelSettings {
  const d: PanelSettings = {
    master: 0.8,
    buses: { Engine: 1, World: 1, Police: 1, Ui: 1, Music: 1 },
    tier: "Pro",
    tts: true,
    open: false,
    source: "recorded",
    ambience: 0.5,
    engine: "modelled",
  };
  try {
    const s = JSON.parse(localStorage.getItem(STORE_KEY) ?? "{}");
    return { ...d, ...s, buses: { ...d.buses, ...(s.buses ?? {}) } };
  } catch {
    return d;
  }
}

export interface PanelHandlers {
  start(): void;
  setMaster(v: number): void;
  setBus(bus: Bus, v: number): void;
  setTier(tier: string): void;
  setTts(on: boolean): void;
  setSiren(mode: string): void;
  testRadio(priority: RadioPriority): void;
  setSource(source: SoundSource): void;
  setAmbience(gain: number): void;
  setEngineVoicing(v: EngineVoicing): void;
}

export class AudioPanel {
  readonly settings = loadSettings();
  private root: HTMLDivElement;
  private status: HTMLDivElement;
  private stats: HTMLDivElement;
  private log: HTMLDivElement;
  private body: HTMLDivElement;

  constructor(parent: HTMLElement, private h: PanelHandlers) {
    this.root = document.createElement("div");
    this.root.className = "panel";
    this.root.id = "audio-panel";
    Object.assign(this.root.style, {
      position: "absolute",
      right: "8px",
      bottom: "8px",
      maxWidth: "330px",
      maxHeight: "50vh",
      overflow: "auto",
      font: "11px monospace",
    });
    const head = document.createElement("div");
    head.style.cursor = "pointer";
    head.innerHTML = "<b>audio</b> <span class='dim'>(F8 mute — rebindable, click to fold)</span>";
    this.status = document.createElement("div");
    this.status.className = "dim";
    this.body = document.createElement("div");
    head.addEventListener("click", () => {
      this.settings.open = !this.settings.open;
      this.applyOpen();
      this.save();
    });
    this.root.append(head, this.status, this.body);

    this.slider("master", this.settings.master, (v) => {
      this.settings.master = v;
      h.setMaster(v);
    });
    for (const b of BUSES) {
      this.slider(b.toLowerCase(), this.settings.buses[b], (v) => {
        this.settings.buses[b] = v;
        h.setBus(b, v);
      });
    }

    const row = (label: string, el: HTMLElement) => {
      const r = document.createElement("div");
      r.style.margin = "3px 0";
      r.append(label + " ", el);
      this.body.append(r);
      return r;
    };
    const src = this.select(["recorded", "procedural"], this.settings.source, (v) => {
      this.settings.source = v as SoundSource;
      h.setSource(this.settings.source);
    });
    src.id = "audio-source";
    row("sounds", src).append(" ", Object.assign(document.createElement("span"), { className: "dim", textContent: "(procedural = fallback)" }));
    const eng = this.select(["modelled", "layers"], this.settings.engine, (v) => {
      this.settings.engine = v as EngineVoicing;
      h.setEngineVoicing(this.settings.engine);
    });
    eng.id = "audio-engine";
    row("engines", eng).append(" ", Object.assign(document.createElement("span"), { className: "dim", textContent: "(modelled: player + 6 nearest)" }));
    this.slider("ambience", this.settings.ambience, (v) => {
      this.settings.ambience = v;
      h.setAmbience(v);
    });
    const tier = this.select(["None", "Basic", "Pro", "Elite"], this.settings.tier, (v) => {
      this.settings.tier = v;
      h.setTier(v);
    });
    const tts = document.createElement("input");
    tts.type = "checkbox";
    tts.checked = this.settings.tts;
    tts.addEventListener("change", () => {
      this.settings.tts = tts.checked;
      h.setTts(tts.checked);
      this.save();
    });
    const r = row("scanner", tier);
    r.append(" tts ", tts);
    row(
      "siren (focus car)",
      this.select(["Default", "Off", "Auto", "Wail", "Yelp", "Phaser", "HiLo"], "Default", (v) => h.setSiren(v)),
    );
    const radio = document.createElement("span");
    for (const p of ["Routine", "Pursuit", "Urgent"] as RadioPriority[]) {
      const btn = document.createElement("button");
      btn.textContent = p.toLowerCase();
      btn.addEventListener("click", () => h.testRadio(p));
      radio.append(btn, " ");
    }
    row("test radio", radio);
    this.stats = document.createElement("div");
    this.stats.style.whiteSpace = "pre";
    this.log = document.createElement("div");
    this.log.style.whiteSpace = "pre-wrap";
    this.log.className = "dim";
    this.body.append(this.stats, this.log);
    parent.append(this.root);
    this.applyOpen();
    this.setStatus("click or press a key to start audio");
  }

  private applyOpen(): void {
    this.body.style.display = this.settings.open ? "" : "none";
  }

  save(): void {
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify(this.settings));
    } catch {
      // storage unavailable: settings last for this page only
    }
  }

  private slider(label: string, value: number, on: (v: number) => void): void {
    const r = document.createElement("div");
    const s = document.createElement("input");
    s.type = "range";
    s.min = "0";
    s.max = "1.5";
    s.step = "0.05";
    s.value = String(value);
    s.style.width = "150px";
    s.style.verticalAlign = "middle";
    const out = document.createElement("span");
    out.textContent = value.toFixed(2);
    s.addEventListener("input", () => {
      const v = Number(s.value);
      out.textContent = v.toFixed(2);
      on(v);
      this.save();
    });
    r.append(label.padEnd(7, " ") + " ", s, " ", out);
    this.body.append(r);
  }

  private select(options: string[], value: string, on: (v: string) => void): HTMLSelectElement {
    const s = document.createElement("select");
    for (const o of options) s.append(new Option(o, o, false, o === value));
    s.addEventListener("change", () => {
      on(s.value);
      this.save();
    });
    return s;
  }

  setStatus(text: string): void {
    this.status.textContent = text;
  }

  update(st: DirectorStats, backendVoices: number, radio: RadioLog[], samples: SampleStatus, engines = ""): void {
    if (!this.settings.open) return;
    const smp = `samples ${samples.loaded}/${samples.total}` + (samples.failed.length ? ` (${samples.failed.length} failed)` : "");
    const bus = BUSES.map((b) => `${b.toLowerCase()} ${st.per_bus[b] ?? 0}`).join("  ");
    this.stats.textContent =
      `voices ${st.looped} loop + ${st.one_shots} shot (culled ${st.culled})  backend ${backendVoices}\n` +
      `${bus}\n` +
      `vehicles ${st.vehicles}  contact pairs ${st.contact_pairs}  radio q ${st.radio_queued} dropped ${st.radio_dropped}\n` +
      `${smp}  source ${this.settings.source}${engines ? `\n${engines}` : ""}`;
    this.log.textContent = radio
      .map((l) => `[${l.priority}] unit ${l.unit}: ${l.text}${l.interrupted ? " (cut)" : ""}`)
      .join("\n");
  }
}
