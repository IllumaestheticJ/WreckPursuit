// Siren lab (audio panel): audition the siren v2 synth without a chase. Pick a pattern and
// how many units (they beat against each other through their per-unit spread), fire a whoop
// or an air-horn burst, run a 27 m/s pass-by (Doppler, distance, air absorption), and put the
// listener in a street canyon, a tunnel, behind a building or inside the car. Plays through
// the same siren rack as the game, local to the listener.

import { sirenV2, type SirenSynth } from "./siren";
import type { SirenRack } from "./siren_rack";
import type { AudioLibrary, SirenRoom, VoiceParams } from "./types";

export interface LabEnv {
  ctx: AudioContext;
  rack: SirenRack;
  lib: AudioLibrary;
}

type Cadence = { lo_hz: number; hi_hz: number; cycle_s: number; rise_frac: number; shape: "Sweep" | "TwoTone" };

const ROOMS: Record<string, SirenRoom | null> = {
  open: null,
  "street 12 m": { slap_left_s: 0.035, slap_right_s: 0.035, slap_gain: [0.3, 0.3], slap_feedback: 0.3, tunnel: 0, rt60_s: 1.8, street_m: 12 },
  "street 24 m": { slap_left_s: 0.07, slap_right_s: 0.07, slap_gain: [0.25, 0.25], slap_feedback: 0.25, tunnel: 0, rt60_s: 1.8, street_m: 24 },
  tunnel: { slap_left_s: 0, slap_right_s: 0, slap_gain: [0, 0], slap_feedback: 0, tunnel: 0.55, rt60_s: 1.8, street_m: 0 },
};
const OPEN: SirenRoom = { slap_left_s: 0, slap_right_s: 0, slap_gain: [0, 0], slap_feedback: 0, tunnel: 0, rt60_s: 1.8, street_m: 0 };

interface LabUnit {
  synth: SirenSynth;
  gain: GainNode;
  lp: BiquadFilterNode;
  panner: PannerNode;
  /** Lateral offset from the pass-by line, m. */
  offset: number;
  cycle: number;
  t0: number;
}

const C = 343;

export class SirenLab {
  readonly root: HTMLDivElement;
  private units: LabUnit[] = [];
  private pattern = "Wail";
  private count = 1;
  private occluded = false;
  private interior = false;
  private passStart: number | null = null;
  private roomName = "open";
  private raf = 0;
  private status: HTMLSpanElement;

  constructor(private env: () => LabEnv | null) {
    this.root = document.createElement("div");
    this.root.id = "siren-lab";
    const row = (...els: (HTMLElement | string)[]) => {
      const r = document.createElement("div");
      r.style.margin = "3px 0";
      r.append(...els);
      this.root.append(r);
    };
    const select = (opts: string[], value: string, on: (v: string) => void) => {
      const s = document.createElement("select");
      for (const o of opts) s.append(new Option(o, o, false, o === value));
      s.addEventListener("change", () => on(s.value));
      return s;
    };
    const button = (label: string, on: () => void) => {
      const b = document.createElement("button");
      b.textContent = label;
      b.addEventListener("click", on);
      return b;
    };
    const check = (label: string, on: (v: boolean) => void) => {
      const l = document.createElement("label");
      const c = document.createElement("input");
      c.type = "checkbox";
      c.addEventListener("change", () => on(c.checked));
      l.append(c, label, " ");
      return l;
    };
    this.status = Object.assign(document.createElement("span"), { className: "dim" });
    row(
      select(["Wail", "Yelp", "Phaser", "HiLo"], this.pattern, (v) => {
        this.pattern = v;
        if (this.units.length) this.play();
      }),
      " units ",
      select(["1", "2", "3"], "1", (v) => {
        this.count = Number(v);
        if (this.units.length) this.play();
      }),
      " ",
      button("play", () => this.play()),
      button("stop", () => this.stop()),
    );
    row(button("whoop", () => this.oneShot(1)), button("horn", () => this.horn()), button("pass-by", () => this.passBy()));
    row(
      "room ",
      select(Object.keys(ROOMS), "open", (v) => this.setRoom(v)),
      " ",
      check("behind building", (v) => (this.occluded = v)),
      check("interior", (v) => (this.interior = v)),
    );
    row(this.status);
  }

  private cadence(lib: AudioLibrary, pattern: string): Cadence {
    const cfg = lib.config as unknown as { sirens: { cadence: Record<string, Cadence> } };
    return cfg.sirens.cadence[pattern];
  }

  private params(doppler = 1, phase = 0): VoiceParams {
    return { gain: 1, pitch: 1, freq_hz: 0, load: 0, phase, lowpass_hz: 18000, doppler, pos: [0, 0, 0] };
  }

  /** Listener position and right / forward vectors (sim space, from the Web Audio listener). */
  private frame(ctx: AudioContext): { pos: number[]; right: number[]; fwd: number[] } {
    const l = ctx.listener;
    const pos = l.positionX ? [l.positionX.value, l.positionY.value, l.positionZ.value] : [0, 0, 0];
    let fwd = l.forwardX ? [l.forwardX.value, l.forwardY.value, 0] : [0, 1, 0];
    const n = Math.hypot(fwd[0], fwd[1]) || 1;
    fwd = [fwd[0] / n, fwd[1] / n, 0];
    return { pos, fwd, right: [fwd[1], -fwd[0], 0] };
  }

  private setRoom(name: string): void {
    const e = this.env();
    if (e) e.rack.labRoom = this.units.length || this.passStart !== null ? (ROOMS[name] ?? OPEN) : null;
    this.roomName = name;
  }

  /** Start (or restart) the selected pattern on `count` units. */
  play(): void {
    const e = this.env();
    if (!e) {
      this.status.textContent = "start audio first (click the page)";
      return;
    }
    this.stop();
    const { ctx, rack, lib } = e;
    const c = this.cadence(lib, this.pattern);
    const t = ctx.currentTime;
    for (let i = 0; i < this.count; i++) {
      // Per-unit spread like wp_audio::siren_unit::unit_spread (±1.5 % pitch, ±5 % rate).
      const pitch = 1 + [0, 0.011, -0.009][i];
      const tempo = 1 + [0, -0.04, 0.03][i];
      const s = { lo_hz: c.lo_hz * pitch, hi_hz: c.hi_hz * pitch, cycle_s: c.cycle_s / tempo, rise_frac: c.rise_frac, two_tone: c.shape === "TwoTone" ? 1 : 0 };
      const synth = sirenV2(ctx, { driver: 1, kind: 0, bright: 0.6, ...s }, this.params(), t);
      const gain = ctx.createGain();
      gain.gain.value = 0.5;
      const lp = ctx.createBiquadFilter();
      lp.type = "lowpass";
      lp.frequency.value = 18000;
      const panner = ctx.createPanner();
      panner.panningModel = "HRTF";
      panner.rolloffFactor = 0;
      synth.output.connect(gain).connect(lp).connect(panner).connect(rack.input);
      this.units.push({ synth, gain, lp, panner, offset: (i - (this.count - 1) / 2) * 5, cycle: s.cycle_s, t0: t });
    }
    rack.labRoom = ROOMS[this.roomName] ?? OPEN;
    this.status.textContent = `${this.pattern} × ${this.count}`;
    this.tick();
  }

  stop(): void {
    const e = this.env();
    cancelAnimationFrame(this.raf);
    if (e) {
      const t = e.ctx.currentTime;
      for (const u of this.units) {
        u.gain.gain.setTargetAtTime(0, t, 0.04);
        u.synth.stop(t + 0.2);
      }
      e.rack.labRoom = null;
    }
    this.units = [];
    this.passStart = null;
    this.status.textContent = "";
  }

  /** Drive a 27 m/s pass-by 8 m in front of the listener (restarts the units if idle). */
  passBy(): void {
    if (!this.units.length) this.play();
    const e = this.env();
    if (!e) return;
    this.passStart = e.ctx.currentTime;
  }

  private oneShot(kind: number): void {
    const e = this.env();
    if (!e) {
      this.status.textContent = "start audio first (click the page)";
      return;
    }
    const { ctx, rack } = e;
    const t = ctx.currentTime + 0.01;
    const v = sirenV2(ctx, { driver: 1, kind, bright: 0.65 }, this.params(), t);
    const g = ctx.createGain();
    g.gain.value = 0.6;
    v.output.connect(g).connect(rack.input);
    setTimeout(() => g.disconnect(), 1500);
  }

  /** Two blasts, as a blocked unit does. */
  private horn(): void {
    this.oneShot(2);
    setTimeout(() => this.oneShot(2), 500);
  }

  /** Per-frame: unit positions, Doppler, distance gain, occlusion / interior muffle. */
  private tick = (): void => {
    const e = this.env();
    if (!e || !this.units.length) return;
    const { ctx } = e;
    const t = ctx.currentTime;
    const { pos, fwd, right } = this.frame(ctx);
    const speed = 27;
    for (const u of this.units) {
      // Static: 15 m ahead; pass-by: along `right` from -90 m, 8 m ahead.
      let along = u.offset;
      let ahead = 15;
      let vel = [0, 0];
      if (this.passStart !== null) {
        along = -90 + speed * (t - this.passStart) + u.offset;
        ahead = 8;
        vel = [right[0] * speed, right[1] * speed];
        if (along > 90 + u.offset) this.passStart = null;
      }
      const rel = [right[0] * along + fwd[0] * ahead, right[1] * along + fwd[1] * ahead];
      const d = Math.hypot(rel[0], rel[1]);
      // Radial speed toward the listener (> 0 approaching) → Doppler c / (c - v).
      const vr = -(vel[0] * rel[0] + vel[1] * rel[1]) / Math.max(d, 1e-3);
      const doppler = C / (C - Math.max(-0.9 * C, Math.min(0.9 * C, vr)));
      const dist = 6 / (6 + Math.max(0, d - 6));
      const air = 18000 / (1 + d / 90);
      const occ = this.occluded ? { g: 0.35, lp: 900 } : { g: 1, lp: 18000 };
      const inside = this.interior ? { g: 0.5, lp: 1500 } : { g: 1, lp: 18000 };
      u.gain.gain.setTargetAtTime(0.5 * dist * occ.g * inside.g, t, 0.03);
      u.lp.frequency.setTargetAtTime(Math.min(air, occ.lp, inside.lp), t, 0.05);
      const p = [pos[0] + rel[0], pos[1] + rel[1], pos[2]];
      if (u.panner.positionX) {
        u.panner.positionX.setTargetAtTime(p[0], t, 0.02);
        u.panner.positionY.setTargetAtTime(p[1], t, 0.02);
        u.panner.positionZ.setTargetAtTime(p[2], t, 0.02);
      }
      const ph = (t - u.t0) / u.cycle;
      u.synth.update(this.params(doppler, ph - Math.floor(ph)), t);
    }
    this.raf = requestAnimationFrame(this.tick);
  };
}
