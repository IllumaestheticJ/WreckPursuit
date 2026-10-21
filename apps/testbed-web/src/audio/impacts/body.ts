// `impact/body`: one synthesised hit on top of the recorded impact layers (owner row 74).
// Parameters come from wp_audio::impact::{body_params, glass_params} (see README.md here).
//
//   contact excitation ─┐
//   modal panel bank ───┼─► (plastic dulling, damage buzz) ─► (cabin low-pass) ─► out
//   chassis thump ──────┤
//   bass transient ─────┘   + a sidechain duck of the other buses for heavy hits
//
// Everything is scheduled at creation (a one-shot): ≤ 6 oscillators for the modes, 2 for the
// thump, 1 for the transient and two noise bursts — cheap native nodes, no worklet needed.

import type { SynthVoice } from "../synth";
import type { VoiceParams } from "../types";
import { asymShaper, biquad, clamp01, gainNode, hitEnv, noiseSrc, rng, stopAll } from "./util";

type Params = Record<string, number>;

/** Mode frequency ratios of a simply supported plate of aspect ≈ 1.3, curvature-stiffened
 * (fundamental raised, so the series is a little compressed): (1,1) (2,1) (1,2) (2,2) (3,1) (3,2). */
const PLATE = [1, 1.94, 2.62, 3.6, 4.1, 5.3];
/** Relative mode amplitudes for a hit near the panel's middle. */
const AMP = [1, 0.62, 0.5, 0.36, 0.3, 0.22];

/** What the impact synths need from the backend. */
export interface ImpactHost {
  /** Briefly duck the other buses by `depth` (0..1) for `holdS`. */
  sidechain(depth: number, holdS: number, t: number): void;
}

export function impactBody(ctx: BaseAudioContext, host: ImpactHost, s: Params, p: VoiceParams, t: number): SynthVoice {
  if ((s.kind ?? 0) >= 1) return glassBreak(ctx, s, p, t);
  const r = rng(s.seed ?? 1);
  const S = clamp01(s.strength ?? 0.5);
  const B = clamp01(s.bright ?? 0.5);
  const plastic = clamp01(s.plastic ?? 0);
  const buzz = clamp01(s.buzz ?? 0);
  const interior = (s.interior ?? 0) >= 0.5;
  const dop = p.pitch || 1;
  const out = gainNode(ctx, 1);
  const sources: AudioScheduledSourceNode[] = [];
  const end = t + 2.5;

  // Air-borne part (excitation + panel ring): the cabin takes its top off for the own car.
  const air = gainNode(ctx, 1);
  if (interior) air.connect(biquad(ctx, "lowpass", 2400, 0.6)).connect(out);
  else air.connect(out);

  // 1. Contact excitation: a noise burst as long as the contact, bright for hard / fast hits.
  const cms = Math.max(1.5, s.contact_ms ?? 5) / 1000;
  const ex = noiseSrc(ctx, t, "white", false, r() * 1.5);
  const exLp = biquad(ctx, "lowpass", 500 + 9000 * B * B, 0.5);
  const exG = gainNode(ctx, 0);
  ex.connect(exLp).connect(exG).connect(air);
  hitEnv(exG.gain, t, 0.0008, (0.25 + 0.45 * S) * (1 - 0.3 * plastic), cms * 0.8);
  ex.stop(t + cms * 6 + 0.02);
  sources.push(ex);

  // 2. Modal panel bank. Sine modes with exponential decay = the impulse response of a bank of
  // resonators; a struck plate's higher modes decay faster and are excited more by hard hits.
  const n = Math.max(2, Math.min(PLATE.length, Math.round(s.modes ?? 6)));
  const f0 = (s.f0 ?? 150) * dop;
  const ring = Math.max(0.01, s.ring_s ?? 0.15);
  const inharm = s.inharm ?? 0.02;
  const modal = gainNode(ctx, 1);
  let modalOut: AudioNode = modal;
  if (plastic > 0) {
    // Plastic covers: heavily damped, dull, with a crack.
    const lp = biquad(ctx, "lowpass", 1400 + 1800 * (1 - plastic), 0.5);
    modalOut = modal.connect(lp);
  }
  const modeLevel = (0.45 + 0.4 * S) * (1 - 0.55 * plastic);
  for (let i = 0; i < n; i++) {
    const f = f0 * PLATE[i] * (1 + inharm * (r() * 2 - 1) * (i === 0 ? 0.3 : 1));
    if (f > ctx.sampleRate * 0.45) break;
    const o = ctx.createOscillator();
    o.type = "sine";
    // Hit stiffens the panel for a moment (oil-can): the pitch settles from slightly sharp.
    o.frequency.setValueAtTime(f * (1 + 0.025 * S), t);
    o.frequency.exponentialRampToValueAtTime(f, t + 0.04);
    const tau = ring * Math.pow(PLATE[0] / PLATE[i], 0.6);
    const amp = modeLevel * AMP[i] * (i === 0 ? 1 : Math.pow(0.35 + 0.65 * B, i * 0.5));
    const g = gainNode(ctx, 0);
    o.connect(g).connect(modal);
    hitEnv(g.gain, t, 0.0015, amp * 0.5, tau);
    o.start(t);
    o.stop(t + Math.min(2.2, tau * 7 + 0.05));
    sources.push(o);
  }
  if (buzz > 0.02) {
    // A dented / loose panel buzzes: the ring is clipped asymmetrically (edges slapping) and
    // chopped at a low rattle rate.
    const sh = asymShaper(ctx, 0.4 + 0.6 * buzz, 0.7);
    const pre = gainNode(ctx, 1 + 3 * buzz);
    const chop = gainNode(ctx, 1 - 0.45 * buzz);
    const lfo = ctx.createOscillator();
    lfo.type = "square";
    lfo.frequency.value = 26 + 22 * r();
    const depth = gainNode(ctx, 0.45 * buzz);
    lfo.connect(depth).connect(chop.gain);
    lfo.start(t);
    lfo.stop(t + Math.min(2.2, ring * 6 + 0.1));
    sources.push(lfo);
    const wet = gainNode(ctx, 0.5 * buzz);
    const dry = gainNode(ctx, 1 - 0.5 * buzz);
    modalOut.connect(dry).connect(air);
    modalOut.connect(pre).connect(sh).connect(chop).connect(wet).connect(air);
  } else {
    modalOut.connect(air);
  }
  if (plastic > 0.3) {
    // Plastic crack: a short band of noise around 1.5–3 kHz.
    const cr = noiseSrc(ctx, t, "white", false, r() * 1.5);
    const bp = biquad(ctx, "bandpass", 1500 + 1500 * B, 1.1);
    const g = gainNode(ctx, 0);
    cr.connect(bp).connect(g).connect(air);
    hitEnv(g.gain, t, 0.0006, 0.5 * plastic * (0.4 + 0.6 * S), 0.018);
    cr.stop(t + 0.15);
    sources.push(cr);
  }

  // 3. Chassis thump: the whole body taking the hit, pitch-dropping, felt more than heard.
  const thump = clamp01(s.thump ?? 0) * (interior ? 1.5 : 1);
  if (thump > 0.01) {
    const fT = (s.thump_hz ?? 50) * dop;
    for (const [mul, lvl] of [
      [1, 1],
      [1.5, 0.35],
    ]) {
      const o = ctx.createOscillator();
      o.type = "sine";
      o.frequency.setValueAtTime(fT * mul * 1.8, t);
      o.frequency.exponentialRampToValueAtTime(fT * mul, t + 0.07);
      const g = gainNode(ctx, 0);
      o.connect(g).connect(out);
      hitEnv(g.gain, t, 0.004, 0.7 * thump * lvl, 0.07 + 0.06 * thump);
      o.start(t);
      o.stop(t + 0.7);
      sources.push(o);
    }
  }

  // 4. Big crash: a short, bass-boosted transient (the "crack" that makes it feel heavy) and a
  // brief duck of the engine / police / music buses, scaled by impulse and by how loud the hit
  // is where the listener is.
  const tr = clamp01(s.transient ?? 0);
  if (tr > 0.01) {
    const nz = noiseSrc(ctx, t, "white", false, r() * 1.5);
    const shelf = biquad(ctx, "lowshelf", 160, 0.7, 12);
    const lp = biquad(ctx, "lowpass", 3000, 0.6);
    const g = gainNode(ctx, 0);
    nz.connect(shelf).connect(lp).connect(g).connect(out);
    hitEnv(g.gain, t, 0.0007, 0.42 * tr, 0.012);
    nz.stop(t + 0.12);
    const o = ctx.createOscillator();
    o.type = "sine";
    o.frequency.setValueAtTime(110 * dop, t);
    o.frequency.exponentialRampToValueAtTime(48 * dop, t + 0.05);
    const og = gainNode(ctx, 0);
    o.connect(og).connect(out);
    hitEnv(og.gain, t, 0.002, 0.5 * tr, 0.035);
    o.start(t);
    o.stop(t + 0.3);
    sources.push(nz, o);
    const audible = clamp01((p.gain || 0) / 0.5);
    const depth = clamp01(s.duck ?? 0) * audible;
    if (depth > 0.02) host.sidechain(depth, s.duck_s ?? 0.15, t);
  }

  stopAll(sources, end);
  return { output: out, update() {}, stop: (t2) => stopAll(sources, t2) };
}

/** Broken glass: a crack onset, the shatter burst and a tinkling debris tail. */
function glassBreak(ctx: BaseAudioContext, s: Params, p: VoiceParams, t: number): SynthVoice {
  const r = rng(s.seed ?? 3);
  const S = clamp01(s.strength ?? 1);
  const bright = clamp01(s.bright ?? 0.8);
  const out = gainNode(ctx, 1);
  const dest: AudioNode = (s.interior ?? 0) >= 0.5 ? (() => {
    const lp = biquad(ctx, "lowpass", 7000, 0.6);
    lp.connect(out);
    return lp;
  })() : out;
  const sources: AudioScheduledSourceNode[] = [];
  const dop = p.pitch || 1;
  // Onset: a very short, very bright crack.
  const cr = noiseSrc(ctx, t, "white", false, r() * 1.5);
  const crG = gainNode(ctx, 0);
  cr.connect(biquad(ctx, "highpass", 2500, 0.7)).connect(crG).connect(dest);
  hitEnv(crG.gain, t, 0.0004, 0.9 * (0.5 + 0.5 * S), 0.004);
  cr.stop(t + 0.06);
  // Shatter body: the pane bursting (band noise, ~0.1 s).
  const sh = noiseSrc(ctx, t, "white", false, r() * 1.5);
  const shG = gainNode(ctx, 0);
  sh.connect(biquad(ctx, "bandpass", 3500 + 2500 * bright, 0.6)).connect(shG).connect(dest);
  hitEnv(shG.gain, t + 0.002, 0.003, 0.55 * (0.5 + 0.5 * S), 0.045);
  sh.stop(t + 0.5);
  sources.push(cr, sh);
  // Debris tail: fragments falling and bouncing, dense first, sparse and quieter later. Each
  // fragment rings two inharmonic plate modes (a small glass shard: ratio ≈ 2.76).
  const n = Math.max(4, Math.min(40, Math.round(s.tinkles ?? 20)));
  const tail = Math.max(0.2, s.tail_s ?? 1.2);
  for (let i = 0; i < n; i++) {
    const at = t + 0.01 + tail * Math.pow(r(), 1.7);
    const age = (at - t) / tail;
    const f = (2600 + 6500 * r()) * dop;
    const lvl = (0.08 + 0.18 * r()) * Math.exp(-2.2 * age) * (0.6 + 0.4 * S);
    for (const [mul, a] of [
      [1, 1],
      [2.76, 0.45],
    ]) {
      if (f * mul > ctx.sampleRate * 0.45) continue;
      const o = ctx.createOscillator();
      o.type = "sine";
      o.frequency.value = f * mul;
      const g = gainNode(ctx, 0);
      o.connect(g).connect(dest);
      hitEnv(g.gain, at, 0.0005, lvl * a, 0.012 + 0.03 * r());
      o.start(at);
      o.stop(at + 0.25);
      sources.push(o);
    }
  }
  return { output: out, update() {}, stop: (t2) => stopAll(sources, t2) };
}
