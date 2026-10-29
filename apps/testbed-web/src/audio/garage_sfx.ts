// Garage and workshop sound effects (owner 2026-10-29): short procedural one-shots for buying,
// the mechanic's impact wrench, bolting parts on and off, repairs and the paint gun, plus a
// quiet room-tone loop while the workshop is open.
//
// Everything is synthesised with Web Audio (noise bursts, oscillators): no recorded files, so
// no new licensed audio and no download. The sounds play on the mixer's Ui bus, scaled by the
// player's Effects volume (and the loop by the Ambience volume), so the master volume, the mute
// toggle and the menu sliders apply exactly as they do to the rest of the game. Presentation
// only: nothing here touches the simulation.

import type { AudioMixer } from "./voices";

/** A garage action that makes a sound. */
export type GarageSound = "buy" | "fit" | "install" | "remove" | "repair" | "paint";

/** Every garage sound, in a fixed order (counters, e2e). */
export const GARAGE_SOUNDS: GarageSound[] = ["buy", "fit", "install", "remove", "repair", "paint"];

/** Level of the room-tone loop at full Ambience volume (it sits well under the one-shots). */
const AMBIENCE_GAIN = 0.12;
/** The same sound asked for again within this many seconds plays once (a plan of several steps). */
const REPEAT_S = 0.12;

/** Procedural garage sounds on one AudioContext, into the mixer's Ui bus. */
export class GarageSfx {
  /** Sounds played (e2e checks). */
  readonly counts: Record<GarageSound, number> = { buy: 0, fit: 0, install: 0, remove: 0, repair: 0, paint: 0 };
  private readonly out: GainNode;
  private readonly ambOut: GainNode;
  private readonly noise: AudioBuffer;
  private loop: { src: AudioBufferSourceNode; hum: OscillatorNode; gain: GainNode } | null = null;
  private readonly last: Partial<Record<GarageSound, number>> = {};

  constructor(
    private readonly ctx: AudioContext,
    mixer: AudioMixer,
  ) {
    this.out = ctx.createGain();
    this.out.connect(mixer.input("Ui"));
    this.ambOut = ctx.createGain();
    this.ambOut.connect(this.out);
    // One second of white noise, shared by every burst (a fixed LCG: the same hiss every run).
    const n = ctx.sampleRate;
    this.noise = ctx.createBuffer(1, n, n);
    const d = this.noise.getChannelData(0);
    let x = 0x2545f491;
    for (let i = 0; i < n; i++) {
      x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
      d[i] = x / 0x80000000 - 1;
    }
  }

  /** Player volumes (0..=1): Effects scales every garage sound, Ambience the room tone. */
  setLevels(effects: number, ambience: number): void {
    const t = this.ctx.currentTime;
    this.out.gain.setTargetAtTime(effects, t, 0.02);
    this.ambOut.gain.setTargetAtTime(ambience, t, 0.05);
  }

  /** Whether the room-tone loop is playing. */
  get ambience(): boolean {
    return this.loop !== null;
  }

  /** Start / stop the workshop room tone (low, filtered noise and a faint lamp hum). */
  setAmbience(on: boolean): void {
    if (on === !!this.loop) return;
    const ctx = this.ctx;
    const t = ctx.currentTime;
    if (!on) {
      const l = this.loop!;
      this.loop = null;
      l.gain.gain.setTargetAtTime(0, t, 0.15);
      l.src.stop(t + 1);
      l.hum.stop(t + 1);
      return;
    }
    const gain = ctx.createGain();
    gain.gain.value = 0;
    gain.gain.setTargetAtTime(AMBIENCE_GAIN, t, 0.4);
    gain.connect(this.ambOut);
    const src = ctx.createBufferSource();
    src.buffer = this.noise;
    src.loop = true;
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 320;
    lp.Q.value = 0.4;
    src.connect(lp).connect(gain);
    // Fluorescent-lamp hum, far under the room noise.
    const hum = ctx.createOscillator();
    hum.frequency.value = 100;
    const humGain = ctx.createGain();
    humGain.gain.value = 0.05;
    hum.connect(humGain).connect(gain);
    src.start(t);
    hum.start(t);
    this.loop = { src, hum, gain };
  }

  /** Play one garage sound now. */
  play(s: GarageSound): void {
    const t = this.ctx.currentTime;
    const prev = this.last[s];
    if (prev !== undefined && t - prev < REPEAT_S) return;
    this.last[s] = t;
    this.counts[s]++;
    switch (s) {
      case "buy":
        return this.register(t);
      case "fit":
        return this.wrench(t);
      case "install":
        this.ratchet(t, 5, 0.065, 2600);
        return this.clunk(t + 0.36, 0.9);
      case "remove":
        this.clunk(t, 0.7);
        return this.ratchet(t + 0.12, 4, 0.075, 2100);
      case "repair":
        for (let i = 0; i < 3; i++) this.tap(t + i * 0.17, 1 - i * 0.15);
        return this.wrench(t + 0.55, 0.3);
      case "paint":
        return this.spray(t);
    }
  }

  /** A burst of filtered noise: `dur` s long, `type` filter at `hz`, peak `gain`. */
  private burst(t: number, dur: number, type: BiquadFilterType, hz: number, gain: number, q = 1, attack = 0.002): void {
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this.noise;
    const f = ctx.createBiquadFilter();
    f.type = type;
    f.frequency.value = hz;
    f.Q.value = q;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(gain, t + attack);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    src.connect(f).connect(g).connect(this.out);
    // A random-looking start point so repeated bursts do not sound identical.
    src.start(t, ((t * 7.31) % 1) * 0.8, dur + 0.05);
  }

  /** A decaying sine partial (bells, metal ring). */
  private tone(t: number, hz: number, dur: number, gain: number, type: OscillatorType = "sine", toHz?: number): void {
    const ctx = this.ctx;
    const o = ctx.createOscillator();
    o.type = type;
    o.frequency.setValueAtTime(hz, t);
    if (toHz) o.frequency.exponentialRampToValueAtTime(toHz, t + dur);
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(gain, t + 0.003);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    o.connect(g).connect(this.out);
    o.start(t);
    o.stop(t + dur + 0.05);
  }

  /** Cash register: the drawer's mechanical clack, then a two-note bell. */
  private register(t: number): void {
    this.burst(t, 0.03, "bandpass", 3200, 0.5, 2);
    this.burst(t + 0.04, 0.05, "bandpass", 1800, 0.35, 3);
    for (const [dt, hz] of [
      [0.08, 1568],
      [0.17, 2093],
    ]) {
      this.tone(t + dt, hz, 0.7, 0.22);
      // Bell overtone (inharmonic) for the metal.
      this.tone(t + dt, hz * 2.76, 0.25, 0.05);
    }
  }

  /** Impact wrench: a motor buzz hammered at ~24 Hz, ending on a tight click. */
  private wrench(t: number, dur = 0.5): void {
    const ctx = this.ctx;
    const o = ctx.createOscillator();
    o.type = "sawtooth";
    o.frequency.setValueAtTime(150, t);
    o.frequency.linearRampToValueAtTime(190, t + dur);
    const bp = ctx.createBiquadFilter();
    bp.type = "bandpass";
    bp.frequency.value = 1100;
    bp.Q.value = 1.5;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(0.18, t + 0.02);
    g.gain.setValueAtTime(0.18, t + dur - 0.04);
    g.gain.linearRampToValueAtTime(0, t + dur);
    o.connect(bp).connect(g).connect(this.out);
    o.start(t);
    o.stop(t + dur + 0.05);
    // The hammer: short noise knocks at 24 Hz over the buzz.
    for (let k = t + 0.01; k < t + dur - 0.02; k += 1 / 24) this.burst(k, 0.018, "bandpass", 2400, 0.28, 1.5);
    this.burst(t + dur, 0.04, "highpass", 3000, 0.3);
  }

  /** Ratchet: `n` clicks `step` s apart. */
  private ratchet(t: number, n: number, step: number, hz: number): void {
    for (let i = 0; i < n; i++) this.burst(t + i * step, 0.022, "bandpass", hz + (i % 2) * 300, 0.35, 4);
  }

  /** A part seating: a low thump with a short metal ring. */
  private clunk(t: number, gain: number): void {
    this.tone(t, 140, 0.16, 0.45 * gain, "sine", 60);
    this.burst(t, 0.07, "lowpass", 900, 0.4 * gain);
    this.tone(t + 0.005, 820, 0.18, 0.06 * gain);
  }

  /** A hammer tap on sheet metal. */
  private tap(t: number, gain: number): void {
    this.burst(t, 0.03, "bandpass", 2000, 0.4 * gain, 1.2);
    this.tone(t, 690, 0.22, 0.1 * gain);
    this.tone(t, 1870, 0.12, 0.05 * gain);
  }

  /** Paint gun: a soft hiss with a fade in and out. */
  private spray(t: number): void {
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this.noise;
    src.loop = true;
    const hp = ctx.createBiquadFilter();
    hp.type = "highpass";
    hp.frequency.value = 3500;
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 9000;
    const g = ctx.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(0.16, t + 0.06);
    g.gain.setValueAtTime(0.16, t + 0.7);
    g.gain.linearRampToValueAtTime(0, t + 0.95);
    src.connect(hp).connect(lp).connect(g).connect(this.out);
    src.start(t);
    src.stop(t + 1);
  }
}
