// Procedural placeholder synths: every cue in data/audio/packs/*_proc.json names one of these
// (wp_audio::data::KNOWN_SYNTHS) so every car makes a sound before recorded packs exist.
// Each synth returns a node graph ending in `output`; the voice pool adds gain, low-pass and
// panner. Loops follow `update()`; one-shots schedule their whole envelope at creation.

import { sirenV2 } from "./siren";
import type { VoiceParams } from "./types";

export interface SynthVoice {
  /** End of the synth graph (connect into the voice chain). */
  output: AudioNode;
  /** Apply new per-frame parameters (loops). */
  update(p: VoiceParams, t: number): void;
  /** Stop every source at time `t`. */
  stop(t: number): void;
}

type Params = Record<string, number>;

// ----- shared buffers ----------------------------------------------------------------------

type NoiseKind = "white" | "crackle" | "wander";
const buffers = new WeakMap<BaseAudioContext, Record<NoiseKind, AudioBuffer>>();

/** Deterministic LCG so the placeholder noise is the same on every load. */
function lcg(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

function noiseBuffers(ctx: BaseAudioContext) {
  let b = buffers.get(ctx);
  if (b) return b;
  const len = Math.floor(ctx.sampleRate * 2);
  const white = ctx.createBuffer(1, len, ctx.sampleRate);
  const crackle = ctx.createBuffer(1, len, ctx.sampleRate);
  const r = lcg(1234);
  const w = white.getChannelData(0);
  const c = crackle.getChannelData(0);
  for (let i = 0; i < len; i++) {
    w[i] = r() * 2 - 1;
    // Sparse clicks with short decays: gravel / crackle texture.
    c[i] = r() < 0.004 ? (r() * 2 - 1) : c[i - 1] ? c[i - 1] * 0.82 : 0;
  }
  // Smooth random wander in -1..1, a new target every ~1/16 s with cosine interpolation:
  // modulation source for jittery pitch / amplitude (tire stick-slip) instead of a sine LFO.
  // Uses its own LCG so the white / crackle buffers stay as they were.
  const wander = ctx.createBuffer(1, len, ctx.sampleRate);
  const wd = wander.getChannelData(0);
  const rw = lcg(4321);
  const step = Math.floor(ctx.sampleRate / 16);
  let a = rw() * 2 - 1;
  let next = rw() * 2 - 1;
  for (let i = 0; i < len; i++) {
    const k = i % step;
    if (k === 0 && i > 0) {
      a = next;
      // Loop seam: the last segment heads back to the first sample's value.
      next = i + step >= len ? wd[0] : rw() * 2 - 1;
    }
    const u = (1 - Math.cos((Math.PI * k) / step)) / 2;
    wd[i] = a + (next - a) * u;
  }
  b = { white, crackle, wander };
  buffers.set(ctx, b);
  return b;
}

function noise(ctx: BaseAudioContext, kind: NoiseKind = "white"): AudioBufferSourceNode {
  const src = ctx.createBufferSource();
  src.buffer = noiseBuffers(ctx)[kind];
  src.loop = true;
  // Random start so overlapping voices do not phase.
  src.start(ctx.currentTime, Math.random() * 1.9);
  return src;
}

function osc(ctx: BaseAudioContext, type: OscillatorType, freq: number): OscillatorNode {
  const o = ctx.createOscillator();
  o.type = type;
  o.frequency.value = freq;
  o.start();
  return o;
}

function gain(ctx: BaseAudioContext, v: number): GainNode {
  const g = ctx.createGain();
  g.gain.value = v;
  return g;
}

function filter(ctx: BaseAudioContext, type: BiquadFilterType, f: number, q = 0.7): BiquadFilterNode {
  const b = ctx.createBiquadFilter();
  b.type = type;
  b.frequency.value = f;
  b.Q.value = q;
  return b;
}

/** Soft-clip curves, cached per drive amount (building one costs ~1000 tanh calls). */
const curves = new Map<number, Float32Array<ArrayBuffer>>();

function shaper(ctx: BaseAudioContext, drive: number): WaveShaperNode {
  const key = Math.round(drive * 100);
  let curve = curves.get(key);
  if (!curve) {
    const n = 1024;
    curve = new Float32Array(n);
    const k = 1 + (key / 100) * 8;
    for (let i = 0; i < n; i++) {
      const x = (i / (n - 1)) * 2 - 1;
      curve[i] = Math.tanh(k * x) / Math.tanh(k);
    }
    curves.set(key, curve);
  }
  const s = ctx.createWaveShaper();
  s.curve = curve;
  s.oversample = "2x";
  return s;
}

const clampF = (f: number, lo = 20, hi = 18000) => Math.min(hi, Math.max(lo, f || lo));
const smooth = (param: AudioParam, v: number, t: number, tau = 0.025) => param.setTargetAtTime(v, t, tau);

/** Exponential-ish one-shot envelope on a gain param. */
function envelope(g: GainNode, t: number, attack: number, decay: number, peak = 1): void {
  g.gain.setValueAtTime(0, t);
  g.gain.linearRampToValueAtTime(peak, t + attack);
  g.gain.setTargetAtTime(0, t + attack, Math.max(decay, 0.005) / 3);
}

function stopAll(nodes: AudioScheduledSourceNode[], t: number): void {
  for (const n of nodes) {
    try {
      n.stop(t);
    } catch {
      // already stopped
    }
  }
}

// ----- loops -------------------------------------------------------------------------------

/** Timbre blend of an engine instrument (see `engineInstrument`). */
export interface EngineMix {
  /** Brightness 0..1 (low-pass opening), energy-weighted over the active layers. */
  bright: number;
  /** Overrun share 0..1: crossfades the on-throttle drive into the softer off-throttle one. */
  off: number;
  /** Combustion / intake noise level (pack `noise`). */
  noise: number;
  /** Limiter chop depth 0..1. */
  chop: number;
}

/** An engine synth whose timbre can move continuously (`setMix`). */
export interface EngineSynth extends SynthVoice {
  setMix(m: EngineMix, t: number): void;
}

/**
 * Engine: one harmonic stack on the firing frequency (sub, 1×, 2×, 3×), uneven-firing AM,
 * combustion noise, on/off-throttle drive and a limiter chop. Every timbre control is a
 * continuous parameter so one instance can stand in for all of a car's procedural layers:
 * separate per-layer synths play the *same* tone, so summing them cancels or reinforces at
 * random with their start phase (playtest #19: up to 6 dB holes in the crossfades).
 */
export function engineInstrument(ctx: BaseAudioContext, s: Params, p: VoiceParams, mix: EngineMix): EngineSynth {
  const f = clampF(p.freq_hz, 5, 2000);
  const uneven = s.uneven ?? 0;
  const balance = s.balance ?? 0.4;
  const out = gain(ctx, 1);
  const pre = gain(ctx, 1);
  const lp = filter(ctx, "lowpass", 2000, 0.9);
  // On / off throttle: the same signal through a hard and a softer drive, crossfaded.
  const onG = gain(ctx, 1 - mix.off);
  const offG = gain(ctx, mix.off);
  pre.connect(shaper(ctx, s.drive ?? 0.3)).connect(onG).connect(lp);
  pre.connect(shaper(ctx, (s.drive ?? 0.3) * 0.6)).connect(offG).connect(lp);
  // Uneven firing: amplitude modulation at half the firing rate (cross-plane burble).
  const am = gain(ctx, 1 - uneven * 0.35);
  lp.connect(am);
  const lfo = osc(ctx, "sine", f * 0.5);
  lfo.connect(gain(ctx, uneven * 0.35)).connect(am.gain);
  // Limiter: square-wave chop of the whole note, depth 0..1.
  const chopGain = gain(ctx, 1 - 0.5 * mix.chop);
  const chop = osc(ctx, "square", s.chop_hz ?? 18);
  const chopDepth = gain(ctx, 0.5 * mix.chop);
  chop.connect(chopDepth).connect(chopGain.gain);
  am.connect(chopGain).connect(out);

  const orders: [number, OscillatorType, number][] = [
    [0.5, "triangle", (s.h1 ?? 0.5) * (0.25 + uneven) * (1.5 - balance)],
    [1, "sawtooth", (s.h2 ?? 1) * 0.6],
    [2, "square", (s.h3 ?? 0.5) * 0.25],
    [3, "sawtooth", (s.h4 ?? 0.3) * 0.2],
  ];
  const oscs = orders.map(([o, type, a]) => {
    const n = osc(ctx, type, f * o);
    n.connect(gain(ctx, a)).connect(pre);
    return { n, o };
  });
  // Combustion / intake noise.
  const nz = noise(ctx);
  const nbp = filter(ctx, "bandpass", f * 3, 0.8);
  const ng = gain(ctx, mix.noise * (0.5 + balance));
  nz.connect(nbp).connect(ng).connect(pre);

  const sources: AudioScheduledSourceNode[] = [...oscs.map((x) => x.n), lfo, nz, chop];
  let cur = { ...mix };
  let f2 = f;
  let load = p.load;
  const setLp = (t: number) => {
    const bright = cur.bright * (0.4 + 0.6 * load);
    smooth(lp.frequency, clampF(f2 * (2.5 + 14 * bright), 150, 12000), t);
  };
  const v: EngineSynth = {
    output: out,
    update(p2, t) {
      f2 = clampF(p2.freq_hz, 5, 2000);
      load = p2.load;
      for (const { n, o } of oscs) smooth(n.frequency, f2 * o, t, 0.012);
      smooth(lfo.frequency, f2 * 0.5, t, 0.012);
      smooth(nbp.frequency, clampF(f2 * 3), t);
      setLp(t);
    },
    setMix(m, t) {
      cur = { ...m };
      smooth(onG.gain, 1 - m.off, t);
      smooth(offG.gain, m.off, t);
      smooth(ng.gain, m.noise * (0.5 + balance), t);
      smooth(chopGain.gain, 1 - 0.5 * m.chop, t);
      smooth(chopDepth.gain, 0.5 * m.chop, t);
      setLp(t);
    },
    stop: (t) => stopAll(sources, t),
  };
  v.update(p, ctx.currentTime);
  return v;
}

/** One engine layer played on its own (cue params fix the timbre). */
function engine(ctx: BaseAudioContext, s: Params, p: VoiceParams): SynthVoice {
  return engineInstrument(ctx, s, p, {
    bright: s.bright ?? 0.5,
    off: s.off ? 1 : 0,
    noise: s.noise ?? 0.1,
    chop: s.chop_hz ? 1 : 0,
  });
}

/** Filtered noise loop: rolling surfaces, scrub, scrapes. `track` > 0 makes the low-pass
 * follow the voice pitch (speed / root for rolling surfaces): `lowpass_hz × pitch^track`, so a
 * surface darkens as the car slows instead of only getting quieter. (Playback rate alone does
 * nothing audible to white noise.)
 *
 * Optional shaping (absent = the original single 12 dB/oct low-pass, +9 dB peak, square AM):
 * - `lp_stages` 2: a second low-pass in series (24 dB/oct). White noise through one 12 dB/oct
 *   pole keeps a third of its energy above the cut-off, which is what made asphalt hiss.
 * - `peak_db`: gain of the `bandpass_hz` peak (default 9 dB).
 * - `ripple_depth` 0..1: AM depth of `ripple_hz` (default 0.5, square); when set, the square
 *   is softened by a 30 Hz low-pass so slab joints pulse instead of clicking. */
function noiseLoop(ctx: BaseAudioContext, s: Params, p: VoiceParams): SynthVoice {
  const out = gain(ctx, 1);
  const src = noise(ctx);
  const hp = filter(ctx, "highpass", s.highpass_hz ?? 60);
  const lpBase = s.lowpass_hz ?? 2000;
  const lps = [filter(ctx, "lowpass", lpBase)];
  if ((s.lp_stages ?? 1) >= 2) lps.push(filter(ctx, "lowpass", lpBase));
  let tail: AudioNode = src.connect(hp);
  for (const f of lps) tail = tail.connect(f);
  if (s.bandpass_hz) {
    const bp = filter(ctx, "peaking", s.bandpass_hz, 2);
    bp.gain.value = s.peak_db ?? 9;
    tail.connect(bp);
    tail = bp;
  }
  const am = gain(ctx, 1);
  tail.connect(am).connect(out);
  const sources: AudioScheduledSourceNode[] = [src];
  let ripple: OscillatorNode | null = null;
  if (s.ripple_hz) {
    const depth = Math.min(1, Math.max(0, s.ripple_depth ?? 0.5));
    am.gain.value = 1 - depth;
    ripple = osc(ctx, "square", s.ripple_hz);
    let mod: AudioNode = ripple;
    if (s.ripple_depth !== undefined) mod = mod.connect(filter(ctx, "lowpass", 30));
    mod.connect(gain(ctx, depth)).connect(am.gain);
    sources.push(ripple);
  }
  if (s.rumble) {
    const r = noise(ctx);
    const rlp = filter(ctx, "lowpass", 120);
    r.connect(rlp).connect(gain(ctx, s.rumble)).connect(out);
    sources.push(r);
  }
  if (s.crackle) {
    const c = noise(ctx, "crackle");
    c.connect(filter(ctx, "highpass", 600)).connect(gain(ctx, s.crackle * 2)).connect(out);
    sources.push(c);
  }
  const v: SynthVoice = {
    output: out,
    update(p2, t) {
      const rate = Math.min(3, Math.max(0.3, p2.pitch));
      smooth(src.playbackRate, rate, t);
      if (s.track) for (const f of lps) smooth(f.frequency, clampF(lpBase * Math.pow(rate, s.track)), t, 0.05);
      if (ripple && s.ripple_hz) smooth(ripple.frequency, s.ripple_hz * rate, t);
    },
    stop: (t) => stopAll(sources, t),
  };
  v.update(p, ctx.currentTime);
  return v;
}

/** Steady tone(s): whine (turbo, supercharger), tone. */
function whine(ctx: BaseAudioContext, s: Params, p: VoiceParams): SynthVoice {
  const out = gain(ctx, 1);
  const base = s.freq_hz ?? 2000;
  const o = osc(ctx, "sine", base);
  const o2 = osc(ctx, "sine", base * 2.01);
  o.connect(gain(ctx, 0.7)).connect(out);
  o2.connect(gain(ctx, 0.15)).connect(out);
  const nz = noise(ctx);
  const bp = filter(ctx, "bandpass", base, 6);
  nz.connect(bp).connect(gain(ctx, s.noise ?? 0.1)).connect(out);
  const v: SynthVoice = {
    output: out,
    update(p2, t) {
      const f = clampF(base * p2.pitch, 50, 16000);
      smooth(o.frequency, f, t);
      smooth(o2.frequency, f * 2.01, t);
      smooth(bp.frequency, f, t);
    },
    stop: (t) => stopAll([o, o2, nz], t),
  };
  v.update(p, ctx.currentTime);
  return v;
}

/** Tire squeal. A sliding tread is a stick-slip oscillator: the rubber grabs and lets go at a
 * rate set by the tread-block / carcass resonances, so the sound is a narrow-band, slightly
 * inharmonic howl whose pitch jitters randomly (not a clean vibrato) and whose loudness
 * flutters. Built as noise through a bank of resonant band-passes (the howl) plus a jittered
 * sawtooth core, a broadband hiss that grows with intensity (`load`), and a lower, rougher,
 * chattering layer for a braking lock-up (`timbre` → 1). Driving wheelspin is the separate
 * `spin` synth below.
 *
 * Params: `freq_hz` howl fundamental (default 900), `jitter` pitch jitter depth (fraction,
 * default 0.025), `chatter_hz` stick-slip flutter rate (default 22). */
function squeal(ctx: BaseAudioContext, s: Params, p: VoiceParams): SynthVoice {
  const out = gain(ctx, 0.75); // headroom: the resonators peak well above their RMS
  const base = s.freq_hz ?? 900;
  const jitter = s.jitter ?? 0.025;
  const chatterHz = s.chatter_hz ?? 22;
  // Final shaping: no rumble below the howl, tame the fizz above it.
  const hp = filter(ctx, "highpass", 280, 0.7);
  const lp = filter(ctx, "lowpass", 7000, 0.7);
  // Stick-slip amplitude flutter.
  const flutter = gain(ctx, 1);
  hp.connect(lp).connect(flutter).connect(out);
  const bus = gain(ctx, 1);
  bus.connect(hp);

  // Howl: white noise through narrow resonators at inharmonic ratios of the tread mode.
  const exc = noise(ctx);
  const modes: [number, number, number][] = [
    [1, 34, 1],
    [1.48, 26, 0.55],
    [2.07, 20, 0.32],
    [2.71, 16, 0.18],
  ];
  const howl = gain(ctx, 0);
  howl.connect(bus);
  const bands = modes.map(([ratio, q, a]) => {
    const bp = filter(ctx, "bandpass", base * ratio, q);
    // A Q≈30 band keeps ~1/1000 of white-noise power; make it up so the howl dominates.
    exc.connect(bp).connect(gain(ctx, a * 9)).connect(howl);
    return { bp, ratio };
  });

  // Tonal core: band-limited sawtooth with random pitch jitter (the "eeee").
  const core = osc(ctx, "sawtooth", base);
  const coreLp = filter(ctx, "lowpass", base * 3, 0.8);
  const coreG = gain(ctx, 0);
  core.connect(coreLp).connect(coreG).connect(bus);
  const wob = noise(ctx, "wander");
  const wobDepth = gain(ctx, base * jitter);
  wob.connect(wobDepth);
  wobDepth.connect(core.frequency);
  for (const b of bands) wobDepth.connect(gain(ctx, b.ratio)).connect(b.bp.frequency);

  // Flutter: faster wander on the output gain; deeper on a lock-up.
  const flut = noise(ctx, "wander");
  flut.playbackRate.value = chatterHz / 16;
  const flutDepth = gain(ctx, 0.15);
  flut.connect(flutDepth).connect(flutter.gain);

  // Broadband slide hiss (rubber tearing) — rises with intensity.
  const hissSrc = noise(ctx);
  const hissBp = filter(ctx, "bandpass", base * 2.5, 0.9);
  const hissG = gain(ctx, 0);
  hissSrc.connect(hissBp).connect(hissG).connect(bus);

  // Lock-up: lower, rough, chattering band plus a gritty low roar.
  const lockSrc = noise(ctx);
  const lockBp = filter(ctx, "bandpass", base * 0.6, 4);
  const lockG = gain(ctx, 0);
  lockSrc.connect(lockBp).connect(lockG).connect(bus);
  const roarSrc = noise(ctx, "crackle");
  const roarLp = filter(ctx, "lowpass", 900, 0.7);
  const roarG = gain(ctx, 0);
  roarSrc.connect(roarLp).connect(gain(ctx, 0.5)).connect(roarG).connect(bus);

  const sources: AudioScheduledSourceNode[] = [exc, core, wob, flut, hissSrc, lockSrc, roarSrc];
  const v: SynthVoice = {
    output: out,
    update(p2, t) {
      const k = Math.min(1, Math.max(0, p2.load));
      const lock = Math.min(1, Math.max(0, p2.timbre ?? 0));
      const f = clampF(base * p2.pitch, 200, 4000);
      for (const b of bands) smooth(b.bp.frequency, clampF(f * b.ratio), t, 0.03);
      smooth(core.frequency, f, t, 0.03);
      smooth(coreLp.frequency, clampF(f * (2.5 + 2 * k)), t);
      smooth(wobDepth.gain, f * jitter * (1 + 1.5 * lock + k), t);
      smooth(hissBp.frequency, clampF(f * 2.5), t);
      smooth(lockBp.frequency, clampF(f * 0.6), t);
      smooth(flut.playbackRate, (chatterHz * (1 + 0.8 * lock)) / 16, t, 0.05);
      // Onset is a clean howl; a hard slide turns harsher and broader; a lock-up trades howl
      // for grit.
      smooth(howl.gain, (0.55 + 0.25 * k) * (1 - 0.55 * lock), t);
      smooth(coreG.gain, (0.16 + 0.1 * k) * (1 - 0.6 * lock), t);
      smooth(hissG.gain, 0.05 + 0.15 * k * k, t);
      smooth(lockG.gain, 3.2 * lock * (0.5 + 0.5 * k), t);
      smooth(roarG.gain, 1.6 * lock * k, t);
      smooth(flutDepth.gain, 0.12 + 0.12 * k + 0.3 * lock, t);
    },
    stop: (t) => stopAll(sources, t),
  };
  v.update(p, ctx.currentTime);
  return v;
}

/**
 * Wheelspin: a low rough howl (band-passed saw, lower than cornering squeal) chopped by the tread
 * "chirp" at a rate rising with slip speed, plus smoke / scrub hiss. `pitch` and `load` come from
 * the director (slip speed and spin intensity).
 */
function spin(ctx: BaseAudioContext, s: Params, p: VoiceParams): SynthVoice {
  const out = gain(ctx, 1);
  const base = s.freq_hz ?? 620;
  const chirpHz = s.chirp_hz ?? 34;
  const am = gain(ctx, 0.65);
  am.connect(out);
  const chirp = osc(ctx, "square", chirpHz);
  const chirpDepth = gain(ctx, 0.35);
  chirp.connect(chirpDepth).connect(am.gain);
  const o = osc(ctx, "sawtooth", base);
  const bp = filter(ctx, "bandpass", base * 1.3, 4);
  const og = gain(ctx, 0.4);
  o.connect(bp).connect(og).connect(am);
  const nz = noise(ctx);
  const nbp = filter(ctx, "bandpass", 2500, 0.8);
  const ng = gain(ctx, 0.3);
  nz.connect(nbp).connect(ng).connect(out);
  const v: SynthVoice = {
    output: out,
    update(p2, t) {
      const pitch = Math.min(3, Math.max(0.3, p2.pitch));
      smooth(o.frequency, clampF(base * pitch), t);
      smooth(bp.frequency, clampF(base * 1.3 * pitch), t);
      smooth(chirp.frequency, chirpHz * pitch, t);
      smooth(og.gain, 0.25 + 0.5 * p2.load, t);
      // Smoke hiss only with real spin: a light chirp at launch (low load) is the howl alone.
      smooth(ng.gain, (s.hiss ?? 0.7) * p2.load, t);
    },
    stop: (t) => stopAll([o, chirp, nz], t),
  };
  v.update(p, ctx.currentTime);
  return v;
}

/** Horn: two detuned square tones (the classic dissonant car horn interval). */
function horn(ctx: BaseAudioContext, s: Params): SynthVoice {
  const out = gain(ctx, 1);
  const lp = filter(ctx, "lowpass", 2800);
  lp.connect(out);
  const a = osc(ctx, "square", s.f1 ?? 420);
  const b = osc(ctx, "square", s.f2 ?? 500);
  a.connect(gain(ctx, 0.4)).connect(lp);
  b.connect(gain(ctx, 0.4)).connect(lp);
  return { output: out, update() {}, stop: (t) => stopAll([a, b], t) };
}

/** Steam / radio static hiss. */
function hiss(ctx: BaseAudioContext, s: Params): SynthVoice {
  const out = gain(ctx, 1);
  const n = noise(ctx);
  n.connect(filter(ctx, "highpass", s.highpass_hz ?? 1000)).connect(filter(ctx, "lowpass", s.lowpass_hz ?? 6000)).connect(out);
  return { output: out, update() {}, stop: (t) => stopAll([n], t) };
}

/** Siren cadence, mirrored from wp_audio::siren::SirenCadence::freq_at_phase. */
export function sirenFreqAtPhase(s: Params, phase: number): number {
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
  const hi = s.hi_hz ?? 1500;
  return lo + (hi - lo) * x;
}

/** Siren (v1 packs): a continuous tone whose pitch follows the cadence on the audio clock.
 * Only the PITCH is modulated; the amplitude never changes for the life of the voice.
 *
 * The cadence free-runs from the director's phase at creation. It used to re-lock to the
 * director's phase whenever they disagreed by more than 5 % of a cycle, but the director's
 * phase comes from sim ticks sampled once per rendered frame: frame jitter, the fixed-step
 * remainder and a sim running slower than real time (the city) put it 10-40 ms off the audio
 * clock all the time. For the yelp (0.3 s cycle, 5 % = 15 ms) that meant a phase jump every
 * few frames, i.e. pitch hiccups a few times a second on top of the sweep. Each curve was
 * also computed for a time 1 ms earlier than where it was placed, so the local clock slipped
 * and forced more re-locks. A new pattern is a new voice, so the voice keeps its own clock.
 * (Siren v2 packs, `driver: 1`, use `siren.ts`, which works the same way.) */
function siren(ctx: BaseAudioContext, s: Params, p: VoiceParams): SynthVoice {
  const out = gain(ctx, 1);
  const t0 = ctx.currentTime;
  const phase0 = p.phase;
  const cycle = Math.max(0.02, s.cycle_s ?? 1);
  const freqAt = (t: number) => sirenFreqAtPhase(s, phase0 + (t - t0) / cycle);
  const o = osc(ctx, s.wave && s.wave >= 1 ? "square" : "triangle", freqAt(t0));
  const o2 = osc(ctx, "sawtooth", freqAt(t0));
  const lp = filter(ctx, "lowpass", 4500);
  o.connect(gain(ctx, 0.35)).connect(lp);
  o2.connect(gain(ctx, 0.15)).connect(lp);
  lp.connect(out);
  const seg = Math.min(cycle, 0.2);
  // At least ~64 points per cadence cycle so even the 10 Hz phaser sweeps cleanly.
  const n = Math.max(24, Math.ceil((64 * seg) / cycle));
  let scheduledUntil = t0;
  let doppler = p.doppler || 1;
  const schedule = (now: number) => {
    if (scheduledUntil < now) scheduledUntil = now;
    while (scheduledUntil < now + 0.3) {
      // Back-to-back curves must not overlap: float rounding in the running end time can put
      // a start a hair before the previous curve's end, which throws NotSupportedError and
      // stops the frame loop. Leave a 1 ms gap, but sample the cadence at the curve's real
      // times so the gap does not shift the sweep.
      const start = scheduledUntil + 0.001;
      const curve = new Float32Array(n);
      for (let i = 0; i < n; i++) curve[i] = freqAt(start + (seg * i) / (n - 1)) * doppler;
      for (const node of [o, o2]) {
        try {
          node.frequency.setValueCurveAtTime(curve, start, seg);
        } catch {
          node.frequency.cancelScheduledValues(start);
        }
      }
      scheduledUntil = start + seg;
    }
  };
  schedule(t0);
  return {
    output: out,
    update(p2, t) {
      // Doppler applies from the next curve on; the cadence keeps its own clock.
      doppler = p2.doppler || 1;
      schedule(t);
    },
    stop: (t) => stopAll([o, o2], t),
  };
}

// ----- one-shots ---------------------------------------------------------------------------

/** Impact: band noise burst + resonant ring + crack click. */
function impact(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SynthVoice {
  const out = gain(ctx, 1);
  const f = (s.freq_hz ?? 200) * (p.pitch || 1);
  const decay = s.decay_s ?? 0.3;
  const nz = noise(ctx);
  const bp = filter(ctx, "bandpass", f * 2, 0.9);
  const ng = gain(ctx, 0);
  nz.connect(bp).connect(ng).connect(out);
  envelope(ng, t, 0.002, decay, (s.noise ?? 0.5) * (0.6 + 0.6 * p.load));
  const ring = osc(ctx, "triangle", f);
  ring.frequency.setValueAtTime(f * 1.3, t);
  ring.frequency.exponentialRampToValueAtTime(f, t + 0.05);
  const rg = gain(ctx, 0);
  ring.connect(rg).connect(out);
  envelope(rg, t, 0.001, decay * 0.8, (s.ring ?? 0.3) * 0.8);
  const crack = noise(ctx);
  const hp = filter(ctx, "highpass", 2500);
  const cg = gain(ctx, 0);
  crack.connect(hp).connect(cg).connect(out);
  envelope(cg, t, 0.0005, 0.03, s.crack ?? 0.3);
  const sources = [nz, ring, crack];
  stopAll(sources, t + decay * 2 + 0.1);
  return { output: out, update() {}, stop: (t2) => stopAll(sources, t2) };
}

/** Glass: noise burst + many random high tinkles. */
function glass(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SynthVoice {
  const out = gain(ctx, 1);
  const decay = s.decay_s ?? 1;
  const nz = noise(ctx);
  const ng = gain(ctx, 0);
  nz.connect(filter(ctx, "highpass", 3000)).connect(ng).connect(out);
  envelope(ng, t, 0.001, decay * 0.4, 0.8);
  const sources: AudioScheduledSourceNode[] = [nz];
  const n = Math.round(s.tinkles ?? 20);
  for (let i = 0; i < n; i++) {
    const at = t + Math.random() * decay * Math.random();
    const o = ctx.createOscillator();
    o.type = "sine";
    o.frequency.value = (3000 + Math.random() * 6000) * (p.pitch || 1);
    const g = gain(ctx, 0);
    o.connect(g).connect(out);
    envelope(g, at, 0.001, 0.05 + Math.random() * 0.1, 0.15 + Math.random() * 0.2);
    o.start(at);
    o.stop(at + 0.3);
    sources.push(o);
  }
  stopAll([nz], t + decay * 2);
  return { output: out, update() {}, stop: (t2) => stopAll(sources, t2) };
}

/** Thump: pitch-dropping low sine (suspension, landings) with optional noise. */
function thump(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SynthVoice {
  const out = gain(ctx, 1);
  const f = (s.freq_hz ?? 50) * (p.pitch || 1);
  const decay = s.decay_s ?? 0.2;
  const o = ctx.createOscillator();
  o.type = "sine";
  o.frequency.setValueAtTime(f * 1.8, t);
  o.frequency.exponentialRampToValueAtTime(f, t + decay * 0.5);
  const g = gain(ctx, 0);
  o.connect(g).connect(out);
  envelope(g, t, 0.003, decay, 1);
  o.start(t);
  o.stop(t + decay * 2 + 0.05);
  const sources: AudioScheduledSourceNode[] = [o];
  if (s.noise) {
    const nz = noise(ctx);
    const ng = gain(ctx, 0);
    nz.connect(filter(ctx, "lowpass", 400)).connect(ng).connect(out);
    envelope(ng, t, 0.002, decay * 0.6, s.noise);
    stopAll([nz], t + decay * 2);
    sources.push(nz);
  }
  return { output: out, update() {}, stop: (t2) => stopAll(sources, t2) };
}

/** Blow-off valve: noise through a falling band-pass. */
function blowoff(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SynthVoice {
  const out = gain(ctx, 1);
  const decay = s.decay_s ?? 0.4;
  const nz = noise(ctx);
  const bp = filter(ctx, "bandpass", (s.lowpass_hz ?? 4000) * (p.pitch || 1), 1.5);
  bp.frequency.exponentialRampToValueAtTime(500, t + decay);
  const g = gain(ctx, 0);
  nz.connect(bp).connect(g).connect(out);
  envelope(g, t, 0.01, decay, 1);
  stopAll([nz], t + decay * 2);
  return { output: out, update() {}, stop: (t2) => stopAll([nz], t2) };
}

/** Exhaust pop: short low burst. */
function pop(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SynthVoice {
  const v = thump(ctx, { freq_hz: (s.freq_hz ?? 90) * 1.5, decay_s: s.decay_s ?? 0.08, noise: 1.2 }, p, t);
  return v;
}

/** Radio squelch: band noise click. */
function squelch(ctx: BaseAudioContext, s: Params, _p: VoiceParams, t: number): SynthVoice {
  const out = gain(ctx, 1);
  const nz = noise(ctx);
  const g = gain(ctx, 0);
  nz.connect(filter(ctx, "bandpass", 2000, 1.2)).connect(g).connect(out);
  envelope(g, t, 0.002, s.decay_s ?? 0.1, 0.8);
  stopAll([nz], t + (s.decay_s ?? 0.1) * 3);
  return { output: out, update() {}, stop: (t2) => stopAll([nz], t2) };
}

/** Clatter: a handful of small random impacts (debris). */
function clatter(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SynthVoice {
  const out = gain(ctx, 1);
  const parts: SynthVoice[] = [];
  const hits = Math.round(s.hits ?? 5);
  const decay = s.decay_s ?? 0.8;
  for (let i = 0; i < hits; i++) {
    const at = t + Math.random() * decay;
    const hit = impact(
      ctx,
      { freq_hz: (s.freq_hz ?? 500) * (0.6 + Math.random() * 0.8), decay_s: 0.08, noise: 0.5, ring: 0.6, crack: 0.3 },
      { ...p, load: 0.3 },
      at,
    );
    hit.output.connect(gain(ctx, 0.5 * (1 - (at - t) / (decay + 0.01)))).connect(out);
    parts.push(hit);
  }
  return { output: out, update() {}, stop: (t2) => parts.forEach((x) => x.stop(t2)) };
}

/** Create a synth voice. Unknown names fall back to a quiet noise loop. */
export function makeSynth(
  ctx: BaseAudioContext,
  name: string,
  params: Params,
  p: VoiceParams,
  t: number,
): SynthVoice {
  switch (name) {
    case "engine":
      return engine(ctx, params, p);
    case "noise":
      return noiseLoop(ctx, params, p);
    case "whine":
    case "tone":
      return whine(ctx, params, p);
    case "squeal":
      return squeal(ctx, params, p);
    case "spin":
      return spin(ctx, params, p);
    case "horn":
      return horn(ctx, params);
    case "hiss":
      return hiss(ctx, params);
    case "siren":
      // Siren v2 packs (`driver: 1`) use the horn-speaker synth in siren.ts.
      return params.driver ? sirenV2(ctx, params, p, t) : siren(ctx, params, p);
    case "impact":
      return impact(ctx, params, p, t);
    case "glass":
      return glass(ctx, params, p, t);
    case "thump":
      return thump(ctx, params, p, t);
    case "blowoff":
      return blowoff(ctx, params, p, t);
    case "pop":
      return pop(ctx, params, p, t);
    case "squelch":
      return squelch(ctx, params, p, t);
    case "clatter":
      return clatter(ctx, params, p, t);
    default:
      return noiseLoop(ctx, { lowpass_hz: 800, highpass_hz: 100 }, p);
  }
}
