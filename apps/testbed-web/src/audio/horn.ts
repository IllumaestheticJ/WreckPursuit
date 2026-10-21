// Physically modelled horns (owner row 74, "better horns"). Cue synths `disc_horn` (cars) and
// `air_horn` (trucks, police); with impact / horn synthesis on, recorded horn cues play these
// too (their procedural twin's parameters), the recordings stay as the fallback setting.
//
// disc_horn — a car's electromagnetic horn pair. Each horn is a steel diaphragm driven by an
// electromagnet through a contact breaker: the diaphragm slams into the pole piece every cycle,
// so the waveform is buzzy and asymmetric (strong even harmonics), and the horn's resonator
// adds a formant around 2–3 kHz. Two horns ~ a major third apart (cue `f1`, `f2`: 420 / 500 Hz
// by default) beat against each other. The diaphragm needs a few cycles to reach full swing
// (short attack, the pitch settles from a little flat) and the stop is the voice's release.
//
// air_horn — a compressed-air reed in a trumpet: a bright, sawtooth-like reed tone per trumpet
// (`f1`, `f2`, optional `f3`), trumpet formants, a breath of air noise and a slower attack as
// the pressure builds.
//
// Damage (`load` = 0..1 from the director: front end / battery): weaker, lower, and rattly —
// the diaphragm chatters (amplitude chopped at a low rate) and one horn of the pair drops out
// of tune.

import type { SynthVoice } from "./synth";
import type { VoiceParams } from "./types";
import { asymShaper, biquad, clamp01, gainNode, noiseSrc, stopAll } from "./impacts/util";

type Params = Record<string, number>;

const waves = new WeakMap<BaseAudioContext, { disc: PeriodicWave; reed: PeriodicWave }>();

/** Harmonic recipes: disc = buzzy with strong even harmonics; reed = bright sawtooth-ish. */
function hornWaves(ctx: BaseAudioContext): { disc: PeriodicWave; reed: PeriodicWave } {
  let w = waves.get(ctx);
  if (w) return w;
  const N = 28;
  const mk = (amp: (n: number) => number, phase: (n: number) => number) => {
    const re = new Float32Array(N + 1);
    const im = new Float32Array(N + 1);
    for (let n = 1; n <= N; n++) {
      const a = amp(n);
      re[n] = a * Math.sin(phase(n));
      im[n] = a * Math.cos(phase(n));
    }
    return ctx.createPeriodicWave(re, im, { disableNormalization: false });
  };
  // Disc: odd and even harmonics (asymmetric slap), a gentle roll-off, a dip around the 4th.
  const disc = mk(
    (n) => (n % 2 ? 1 : 0.7) / Math.pow(n, 0.85) * (n === 4 ? 0.6 : 1),
    (n) => (n % 2 ? 0 : 0.9) + 0.15 * n,
  );
  const reed = mk(
    (n) => 1 / Math.pow(n, 0.95),
    () => 0,
  );
  w = { disc, reed };
  waves.set(ctx, w);
  return w;
}

export function hornSynth(ctx: BaseAudioContext, kind: "disc" | "air", s: Params, p: VoiceParams, t: number): SynthVoice {
  const air = kind === "air";
  const w = hornWaves(ctx);
  const out = gainNode(ctx, 1);
  const sources: AudioScheduledSourceNode[] = [];
  const freqs = [s.f1 ?? (air ? 233 : 420), s.f2 ?? (air ? 294 : 500)];
  if (s.f3) freqs.push(s.f3);
  let damage = clamp01(p.load);
  const dop = p.pitch || 1;
  let lastPitch = dop;

  // Damage stage: overall level and a chatter (diaphragm hitting a bent bracket).
  const dmg = gainNode(ctx, 1);
  const chop = gainNode(ctx, 1);
  const chat = ctx.createOscillator();
  chat.type = "square";
  chat.frequency.value = air ? 17 : 23;
  const chatDepth = gainNode(ctx, 0);
  chat.connect(chatDepth).connect(chop.gain);
  chat.start(t);
  sources.push(chat);
  const sum = gainNode(ctx, 1);
  // Resonator formants and the drive's asymmetric saturation.
  const shaper = asymShaper(ctx, air ? 0.3 : 0.55, air ? 0.2 : 0.75);
  const formA = biquad(ctx, "peaking", air ? 1250 : 2400, air ? 1.2 : 1.6, air ? 5 : 8);
  const formB = biquad(ctx, "peaking", air ? 2900 : 3600, 2, air ? 4 : 3);
  const lp = biquad(ctx, "lowpass", air ? 5200 : 6500, 0.6);
  const hp = biquad(ctx, "highpass", air ? 120 : 220, 0.7);
  sum.connect(shaper).connect(hp).connect(formA).connect(formB).connect(lp).connect(dmg).connect(chop).connect(out);

  // Level: a held horn sits where the previous procedural horn sat (≈ −20 dBFS at 6 m in
  // e2e/audio_roster.mjs, recorded fallback within 6 dB); air horns a touch louder.
  const level = air ? 0.48 : 0.42;
  const attack = air ? 0.045 : 0.012;
  const voices: { o: OscillatorNode; base: number; g: GainNode }[] = [];
  freqs.forEach((f0, i) => {
    const o = ctx.createOscillator();
    o.setPeriodicWave(air ? w.reed : w.disc);
    const f = f0 * dop;
    // The diaphragm / reed settles in from flat.
    o.frequency.setValueAtTime(f * (air ? 0.9 : 0.96), t);
    o.frequency.setTargetAtTime(f, t, air ? 0.03 : 0.008);
    // Slight vibrato and drift: real horns are never perfectly steady.
    const lfo = ctx.createOscillator();
    lfo.frequency.value = (air ? 4.1 : 6.3) + i * 0.7;
    const lfoG = gainNode(ctx, f * (air ? 0.004 : 0.0025));
    lfo.connect(lfoG).connect(o.frequency);
    const g = gainNode(ctx, 0);
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(level / freqs.length * (i === 0 ? 1.1 : 0.9), t + attack);
    o.connect(g).connect(sum);
    o.start(t);
    lfo.start(t);
    sources.push(o, lfo);
    voices.push({ o, base: f0, g });
  });
  if (air) {
    // Breath: air rushing through the reed.
    const n = noiseSrc(ctx, t);
    const ng = gainNode(ctx, 0);
    n.connect(biquad(ctx, "bandpass", 1400, 0.8)).connect(ng).connect(sum);
    ng.gain.setValueAtTime(0, t);
    ng.gain.linearRampToValueAtTime(0.03, t + 0.02);
    sources.push(n);
  }

  const applyDamage = (d: number, now: number, tau: number) => {
    dmg.gain.setTargetAtTime(1 - 0.6 * d, now, tau);
    // Chops between 1 − 0.6 d and 1 (never silent: −8 dB at d = 0.9).
    chatDepth.gain.setTargetAtTime(0.3 * d, now, tau);
    chop.gain.setTargetAtTime(1 - 0.3 * d, now, tau);
    // One horn of the pair sags flat (bent mount / low voltage).
    const v = voices[voices.length - 1];
    if (voices.length > 1) v.o.detune.setTargetAtTime(-90 * d, now, tau);
  };
  applyDamage(damage, t, 0.002);

  return {
    output: out,
    update(np: VoiceParams, now: number) {
      const d = clamp01(np.load);
      if (Math.abs(d - damage) > 0.01) {
        damage = d;
        applyDamage(d, now, 0.05);
      }
      // Doppler (the director folds it into `pitch`).
      const dp = np.pitch || 1;
      if (Math.abs(dp - lastPitch) > 0.002 * lastPitch) {
        lastPitch = dp;
        for (const v of voices) v.o.frequency.setTargetAtTime(v.base * dp, now, 0.03);
      }
    },
    stop: (at) => stopAll(sources, at),
  };
}
