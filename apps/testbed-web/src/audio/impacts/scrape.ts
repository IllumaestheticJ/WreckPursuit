// Friction-driven scrape (owner row 74): replaces the plain noise loop / augments the recorded
// scrape loop when the director marks a scrape voice with `friction` (wp_audio director,
// `scrape_desires`). Per frame: `freq_hz` = stick-slip rate (rises with the sliding speed,
// Doppler included), `load` = scrape level (sliding speed), `pitch` = the recording's rate.
// Play-time params: `metal` (else plastic), `sparks` (metal on the road / a wall), `force`
// (contact pressure 0..1).
//
// Model: sliding contact sticks and slips; each slip is a small impulse into the panel, so a
// jittered pulse train at the stick-slip rate drives a few panel resonances (metal: bright,
// ringing; plastic: low, dull), over a grinding noise band whose centre and brightness follow
// speed × pressure. Sparks are sparse bright crackle. Cheap native nodes.

import type { SynthVoice } from "../synth";
import type { VoiceParams } from "../types";
import { asymShaper, biquad, clamp01, gainNode, noiseSrc, stopAll } from "./util";

type Params = Record<string, number>;

export function frictionScrape(ctx: BaseAudioContext, rec: AudioBuffer | null, s: Params, p: VoiceParams, t: number): SynthVoice {
  const metal = (s.metal ?? 1) >= 0.5;
  const force = clamp01(s.force ?? 0.5);
  const sparks = (s.sparks ?? 0) >= 0.5;
  const out = gainNode(ctx, 1);
  const sources: AudioScheduledSourceNode[] = [];

  // Recorded loop underneath (when there is one), following its pitch.
  let recSrc: AudioBufferSourceNode | null = null;
  if (rec) {
    recSrc = ctx.createBufferSource();
    recSrc.buffer = rec;
    recSrc.loop = true;
    recSrc.playbackRate.value = p.pitch || 1;
    recSrc.connect(gainNode(ctx, 0.6)).connect(out);
    recSrc.start(t, Math.random() * Math.max(0, rec.duration - 0.1));
    sources.push(recSrc);
  }

  // Stick-slip pulse train with a jittered rate (slips are irregular).
  const rate = Math.max(8, p.freq_hz || 40);
  const pulse = ctx.createOscillator();
  pulse.type = "sawtooth";
  pulse.frequency.value = rate;
  const jitterSrc = noiseSrc(ctx, t);
  const jitterLp = biquad(ctx, "lowpass", 35, 0.5);
  const jitter = gainNode(ctx, rate * 0.6);
  jitterSrc.connect(jitterLp).connect(jitter).connect(pulse.frequency);
  pulse.start(t);
  sources.push(pulse, jitterSrc);
  const slip = asymShaper(ctx, 0.8, 0.8);
  const slipHp = biquad(ctx, "highpass", metal ? 400 : 180, 0.7);
  const slipG = gainNode(ctx, 0);
  pulse.connect(slip).connect(slipHp);
  const modes = metal ? [820, 1730, 3050, 4600] : [310, 760, 1400];
  for (const [i, f] of modes.entries()) {
    const bp = biquad(ctx, "bandpass", f, metal ? 9 : 4);
    slipHp.connect(bp).connect(gainNode(ctx, (metal ? 1.4 : 1.1) / (1 + i * 0.5))).connect(slipG);
  }
  slipG.connect(out);

  // Grinding band.
  const grit = noiseSrc(ctx, t);
  const gritBp = biquad(ctx, "bandpass", 600, metal ? 1.4 : 1.0);
  const gritLp = biquad(ctx, "lowpass", 3000, 0.5);
  const gritG = gainNode(ctx, 0);
  grit.connect(gritBp).connect(gritLp).connect(gritG).connect(out);
  sources.push(grit);

  // Sparks: sparse bright crackle (metal on asphalt / concrete).
  let sparkG: GainNode | null = null;
  if (sparks) {
    const c = noiseSrc(ctx, t, "crackle");
    sparkG = gainNode(ctx, 0);
    c.connect(biquad(ctx, "highpass", 3500, 0.7)).connect(sparkG).connect(out);
    sources.push(c);
  }

  const apply = (np: VoiceParams, now: number, tau: number) => {
    const lvl = clamp01(np.load);
    const v = Math.max(8, np.freq_hz || rate);
    pulse.frequency.setTargetAtTime(v, now, tau);
    jitter.gain.setTargetAtTime(v * 0.6, now, tau);
    if (recSrc) recSrc.playbackRate.setTargetAtTime(np.pitch || 1, now, tau);
    const press = 0.4 + 0.6 * force;
    slipG.gain.setTargetAtTime(0.3 * press * (0.4 + 0.6 * lvl), now, tau);
    gritBp.frequency.setTargetAtTime((metal ? 500 : 250) + (metal ? 14 : 6) * v, now, tau);
    gritLp.frequency.setTargetAtTime(1200 + 9000 * lvl * press, now, tau);
    gritG.gain.setTargetAtTime(0.45 * press * (0.3 + 0.7 * lvl), now, tau);
    // Sparks track speed more than level: present from moderate grinding on.
    sparkG?.gain.setTargetAtTime(1.4 * lvl * press, now, tau);
  };
  apply(p, t, 0.005);
  return {
    output: out,
    update: (np, now) => apply(np, now, 0.025),
    stop: (at) => stopAll(sources, at),
  };
}
