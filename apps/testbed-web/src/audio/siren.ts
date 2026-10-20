// Siren v2 synth (playtest row 82): an electronic siren driver into a horn speaker.
//
//   driver:  a trapezoid wave (a square with finite edge slew: odd harmonics rolling off like
//            1/n · sinc, plus a trace of even harmonics from driver asymmetry) on the cadence
//            frequency, Doppler and per-unit drift on `detune`;
//   horn:    flare cutoff (two high-passes ~400-480 Hz), two throat/bell resonances, a comb dip,
//            soft clipping of the driver stage, PA band-limit low-pass.
//
// Cues (data/audio/packs/siren_v2.json, synth "siren" with `driver: 1`): `kind` 0 = cadence
// loop (wail / yelp / phaser / hi-lo, cadence params from wp_audio; starts at the director's
// phase, then free-runs on the audio clock, pitch only), 1 = whoop chirp, 2 = air horn blast.
// The cadence is scheduled a little ahead with setValueCurveAtTime through
// `CurvePlan`, whose curve starts are integer multiples of the segment from a fixed origin so
// consecutive curves can never overlap (playtest row 30: accumulated float starts overlapped,
// the browser threw and the frame loop stopped). Mirrors wp_audio::siren::{CurvePlan,
// SirenCadence::freq_at_phase}.

import type { VoiceParams } from "./types";
import type { SynthVoice } from "./synth";

type Params = Record<string, number>;

/** Counters for the debug panel and the e2e (curve errors must stay 0). */
export const sirenStats = { curves: 0, curveErrors: 0, loops: 0, whoops: 0, horns: 0 };

/** Siren cadence frequency at cycle phase `phase` (mirror of SirenCadence::freq_at_phase). */
export function cadenceFreq(s: Params, phase: number): number {
  const p = phase - Math.floor(phase);
  let x: number;
  if (s.two_tone) {
    x = p < 0.5 ? 1 : 0;
  } else {
    // Smoothstep rise and fall: glides at both turnarounds (no corner at the peak, no dwell
    // at the bottom; that shape made the yelp sound like laughing).
    const r = Math.min(0.99, Math.max(0.01, s.rise_frac ?? 0.5));
    const ease = (u: number) => u * u * (3 - 2 * u);
    x = p < r ? ease(p / r) : 1 - ease((p - r) / (1 - r));
  }
  const lo = s.lo_hz ?? 650;
  const hi = s.hi_hz ?? 1600;
  return lo + (hi - lo) * x;
}

/** One planned automation curve. */
export interface CurveSegment {
  index: number;
  start: number;
  dur: number;
}

/**
 * Back-to-back automation segments that never overlap (mirror of wp_audio::siren::CurvePlan).
 * Starts are `origin + k·seg` from an integer `k`; each curve lasts `seg - gap`.
 */
export class CurvePlan {
  readonly seg: number;
  readonly gap: number;
  next = 0;
  constructor(
    readonly origin: number,
    seg: number,
    gap: number,
  ) {
    this.seg = Math.max(0.004, seg);
    this.gap = Math.min(this.seg / 4, Math.max(1e-5, gap));
  }

  /** Segments covering up to `now + lookahead`; never back-dated before `now`. */
  plan(now: number, lookahead: number): CurveSegment[] {
    const out: CurveSegment[] = [];
    const firstOk = Math.ceil((now - this.origin) / this.seg);
    if (firstOk > this.next) this.next = firstOk;
    for (;;) {
      const start = this.origin + this.next * this.seg;
      if (start > now + lookahead) break;
      out.push({ index: this.next, start, dur: this.seg - this.gap });
      this.next++;
    }
    return out;
  }
}

// ----- shared nodes ---------------------------------------------------------------------------

const waves = new WeakMap<BaseAudioContext, { driver: PeriodicWave; buzz: PeriodicWave }>();

/** sin(πx)/(πx). */
const sinc = (x: number) => (x === 0 ? 1 : Math.sin(Math.PI * x) / (Math.PI * x));

function periodicWaves(ctx: BaseAudioContext) {
  let w = waves.get(ctx);
  if (w) return w;
  const n = 48;
  // Trapezoid: edges take ~3.5 % of the period (a real driver's slew), so the upper harmonics
  // fall faster than a square's. 2 % even harmonics: the push-pull stage is not symmetric.
  const re = new Float32Array(n);
  const im = new Float32Array(n);
  for (let k = 1; k < n; k++) {
    im[k] = k % 2 ? sinc(k * 0.035) / k : 0.02 / k;
  }
  const driver = ctx.createPeriodicWave(re, im);
  // Air-horn reed buzz: every harmonic, 1/k (saw-like), slightly lifted 2nd-4th.
  const re2 = new Float32Array(n);
  const im2 = new Float32Array(n);
  for (let k = 1; k < n; k++) im2[k] = (k >= 2 && k <= 4 ? 1.4 : 1) / k;
  const buzz = ctx.createPeriodicWave(re2, im2);
  w = { driver, buzz };
  waves.set(ctx, w);
  return w;
}

const curves = new Map<number, Float32Array<ArrayBuffer>>();
function softClip(ctx: BaseAudioContext, drive: number): WaveShaperNode {
  const key = Math.round(drive * 100);
  let c = curves.get(key);
  if (!c) {
    c = new Float32Array(1024);
    const k = 1 + drive * 6;
    for (let i = 0; i < c.length; i++) {
      const x = (i / (c.length - 1)) * 2 - 1;
      c[i] = Math.tanh(k * x) / Math.tanh(k);
    }
    curves.set(key, c);
  }
  const s = ctx.createWaveShaper();
  s.curve = c;
  s.oversample = "2x";
  return s;
}

function biquad(ctx: BaseAudioContext, type: BiquadFilterType, f: number, q: number, gainDb = 0): BiquadFilterNode {
  const b = ctx.createBiquadFilter();
  b.type = type;
  b.frequency.value = f;
  b.Q.value = q;
  b.gain.value = gainDb;
  return b;
}

/** The horn speaker: `input` → flare cutoff → resonances → driver clipping → PA band-limit. */
function hornSpeaker(ctx: BaseAudioContext, bright: number, flareHz = 480): { input: AudioNode; output: GainNode } {
  const hp1 = biquad(ctx, "highpass", flareHz, 0.9);
  const hp2 = biquad(ctx, "highpass", flareHz * 0.8, 0.6);
  const throat = biquad(ctx, "peaking", 1150, 1.8, 6);
  const bell = biquad(ctx, "peaking", 2700, 2.5, 4);
  const comb = biquad(ctx, "peaking", 1900, 3, -3);
  const drive = softClip(ctx, 0.35);
  const pre = ctx.createGain();
  pre.gain.value = 0.55;
  const band = biquad(ctx, "lowpass", 5000 + 2000 * bright, 0.8);
  const band2 = biquad(ctx, "lowpass", 8000, 0.7);
  const out = ctx.createGain();
  out.gain.value = 0.6;
  hp1.connect(hp2).connect(throat).connect(bell).connect(comb).connect(pre).connect(drive).connect(band).connect(band2).connect(out);
  return { input: hp1, output: out };
}

const cents = (ratio: number) => 1200 * Math.log2(Math.min(2, Math.max(0.5, ratio || 1)));

/** A siren synth that can also be scheduled far ahead (offline renders). */
export interface SirenSynth extends SynthVoice {
  /** Schedule the cadence up to audio time `t` (live voices schedule themselves on update). */
  scheduleUntil(t: number): void;
}

// ----- the three cues ------------------------------------------------------------------------

/** Cadence loop: wail / yelp / phaser / hi-lo. */
function sirenLoop(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SirenSynth {
  sirenStats.loops++;
  const { driver } = periodicWaves(ctx);
  const horn = hornSpeaker(ctx, s.bright ?? 0.6);
  const osc = ctx.createOscillator();
  osc.setPeriodicWave(driver);
  osc.frequency.value = cadenceFreq(s, p.phase);
  osc.detune.value = cents(p.doppler);
  // Fade in so a pattern change crossfades with the old loop's release (no click).
  const fade = ctx.createGain();
  fade.gain.setValueAtTime(0, t);
  fade.gain.linearRampToValueAtTime(1, t + 0.12);
  osc.connect(horn.input);
  horn.output.connect(fade);
  osc.start(t);

  const cycle = Math.max(0.02, s.cycle_s ?? 1);
  const seg = Math.min(0.1, cycle / 2);
  // ~400 points/s, and at least 64 per cadence cycle so even the 10 Hz phaser sweeps cleanly.
  const points = Math.max(8, Math.ceil(seg * 400), Math.ceil((64 * seg) / cycle));
  // Phase clock: phase at audio time τ = phase0 + (τ - t0) / cycle. Seeded once from the
  // director's phase, then free-running on the audio clock (see `update`).
  const t0 = t;
  const phase0 = p.phase;
  // Curves start 2 ms after the initial value so the two can never coincide.
  const plan = new CurvePlan(t + 0.002, seg, 0.001);
  const schedule = (now: number, lookahead: number) => {
    for (const c of plan.plan(now, lookahead)) {
      const curve = new Float32Array(points);
      for (let i = 0; i < points; i++) curve[i] = cadenceFreq(s, phase0 + (c.start + (c.dur * i) / (points - 1) - t0) / cycle);
      try {
        osc.frequency.setValueCurveAtTime(curve, c.start, c.dur);
        sirenStats.curves++;
      } catch (e) {
        // Must never happen (CurvePlan cannot overlap); counted, never thrown at the page.
        sirenStats.curveErrors++;
        console.warn("siren curve rejected", e);
      }
    }
  };
  schedule(t, 0.25);
  return {
    output: fade,
    update(p2, now) {
      // Only Doppler follows the director per update; the cadence keeps its own clock. It used
      // to re-lock to the director's phase whenever they disagreed by > 5 % of a cycle, but
      // that phase comes from sim ticks sampled once per rendered frame: frame jitter, the
      // fixed-step remainder and a sim slower than real time put it 10-40 ms off the audio
      // clock all the time. For the yelp (0.3 s cycle, 5 % = 15 ms) that was a phase jump
      // every few frames, i.e. audible pitch steps several times a second. A pattern change
      // is a new voice, so the sweep never needs to be sample-locked to the ticks.
      osc.detune.setTargetAtTime(cents(p2.doppler), now, 0.03);
      schedule(now, 0.25);
    },
    scheduleUntil(until) {
      schedule(ctx.currentTime, Math.max(0, until - ctx.currentTime));
    },
    stop: (end) => {
      try {
        osc.stop(end);
      } catch {
        // already stopped
      }
    },
  };
}

/** Whoop: one quick rise and fall, the "chirp" a unit gives when it lights up. */
function whoop(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SirenSynth {
  sirenStats.whoops++;
  const { driver } = periodicWaves(ctx);
  const horn = hornSpeaker(ctx, s.bright ?? 0.6);
  const osc = ctx.createOscillator();
  osc.setPeriodicWave(driver);
  const pitch = p.pitch || 1;
  const dur = 0.55;
  const n = 64;
  const curve = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const u = i / (n - 1);
    const f = u < 0.4 ? 550 + 1100 * (1 - (1 - u / 0.4) ** 2) : 1650 - 1000 * ((u - 0.4) / 0.6) ** 1.5;
    curve[i] = f * pitch;
  }
  osc.frequency.setValueCurveAtTime(curve, t, dur);
  osc.detune.value = cents(p.doppler);
  const env = ctx.createGain();
  env.gain.setValueAtTime(0, t);
  env.gain.linearRampToValueAtTime(1, t + 0.015);
  env.gain.setValueAtTime(1, t + dur - 0.12);
  env.gain.linearRampToValueAtTime(0, t + dur);
  osc.connect(horn.input);
  horn.output.connect(env);
  osc.start(t);
  osc.stop(t + dur + 0.05);
  return {
    output: env,
    update(p2, now) {
      osc.detune.setTargetAtTime(cents(p2.doppler), now, 0.03);
    },
    scheduleUntil() {},
    stop: (end) => {
      try {
        osc.stop(end);
      } catch {
        // already stopped
      }
    },
  };
}

/** Air horn: a short, buzzy two-reed blast through the same horn speaker. */
function airhorn(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SirenSynth {
  sirenStats.horns++;
  const { buzz } = periodicWaves(ctx);
  const horn = hornSpeaker(ctx, s.bright ?? 0.7, 230);
  const pitch = p.pitch || 1;
  const len = 0.42;
  const oscs = [370, 466].map((f) => {
    const o = ctx.createOscillator();
    o.setPeriodicWave(buzz);
    o.frequency.setValueAtTime(f * pitch * 0.97, t);
    // Reeds settle in, then droop as the air runs out.
    o.frequency.linearRampToValueAtTime(f * pitch, t + 0.04);
    o.frequency.setValueAtTime(f * pitch, t + len - 0.1);
    o.frequency.linearRampToValueAtTime(f * pitch * 0.96, t + len);
    o.detune.value = cents(p.doppler);
    o.connect(horn.input);
    o.start(t);
    o.stop(t + len + 0.05);
    return o;
  });
  const env = ctx.createGain();
  env.gain.setValueAtTime(0, t);
  env.gain.linearRampToValueAtTime(1, t + 0.012);
  env.gain.setValueAtTime(0.9, t + len - 0.07);
  env.gain.linearRampToValueAtTime(0, t + len);
  horn.output.connect(env);
  return {
    output: env,
    update(p2, now) {
      for (const o of oscs) o.detune.setTargetAtTime(cents(p2.doppler), now, 0.03);
    },
    scheduleUntil() {},
    stop: (end) => {
      for (const o of oscs) {
        try {
          o.stop(end);
        } catch {
          // already stopped
        }
      }
    },
  };
}

/** Siren v2 synth for a cue (`kind` 0 loop, 1 whoop, 2 air horn), starting at `t`. */
export function sirenV2(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SirenSynth {
  const kind = Math.round(s.kind ?? 0);
  if (kind === 1) return whoop(ctx, s, p, t);
  if (kind === 2) return airhorn(ctx, s, p, t);
  return sirenLoop(ctx, s, p, t);
}

/**
 * Offline render of one siren cue (e2e spectral checks, no AudioContext gesture needed):
 * `cadence` = lo_hz / hi_hz / cycle_s / rise_frac / two_tone; returns mono samples.
 */
export async function renderSiren(opts: {
  kind?: number;
  cadence?: Params;
  doppler?: number;
  seconds: number;
  sampleRate?: number;
}): Promise<Float32Array> {
  const sr = opts.sampleRate ?? 48000;
  const ctx = new OfflineAudioContext(1, Math.ceil(opts.seconds * sr), sr);
  const p: VoiceParams = {
    gain: 1,
    pitch: 1,
    freq_hz: 0,
    load: 0,
    phase: 0,
    lowpass_hz: 18000,
    doppler: opts.doppler ?? 1,
    pos: [0, 0, 0],
  };
  const v = sirenV2(ctx, { driver: 1, kind: opts.kind ?? 0, bright: 0.6, ...(opts.cadence ?? {}) }, p, 0);
  v.scheduleUntil(opts.seconds);
  v.output.connect(ctx.destination);
  const buf = await ctx.startRendering();
  return buf.getChannelData(0);
}
