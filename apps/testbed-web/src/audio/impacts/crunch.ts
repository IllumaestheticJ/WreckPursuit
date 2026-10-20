// `impact/crunch`: granular layering of the recorded metal crunches while a car's lattice is
// being crushed (owner row 74). The director keeps this loop on while the plastic deformation
// power is above a floor; `load` (0..1) is the log of that power and sets the grain density,
// grain size and brightness every frame, so a 90 km/h crash crumples densely for as long as
// the body folds and a parking-lot dent gives a few crackles.
//
// Grains are scheduled a little ahead of the audio clock on each `update` (one per frame);
// at most MAX_LIVE grains overlap. Without decoded recordings (procedural mode / not loaded
// yet) each grain is a band of noise instead.

import type { SynthVoice } from "../synth";
import type { VoiceParams } from "../types";
import { biquad, clamp01, gainNode, noiseBuffers, rng } from "./util";

type Params = Record<string, number>;

/** Grains scheduled ahead of the audio clock, s (a frame plus margin). */
const AHEAD_S = 0.05;
/** Overlapping grains at most (CPU bound). */
const MAX_LIVE = 24;

export function crunch(ctx: BaseAudioContext, buffers: AudioBuffer[], s: Params, p: VoiceParams, t: number): SynthVoice {
  const r = rng((s.seed ?? 0) + Math.floor(t * 1000));
  const out = gainNode(ctx, 1);
  const tone = biquad(ctx, "lowpass", 4000, 0.5);
  const body = biquad(ctx, "peaking", 220, 0.8, 4);
  tone.connect(body).connect(out);
  const dest: AudioNode = (s.interior ?? 0) >= 0.5 ? (() => {
    const lp = biquad(ctx, "lowpass", 3000, 0.6);
    lp.connect(tone);
    return lp;
  })() : tone;
  let level = clamp01(p.load);
  let pitch = p.pitch || 1;
  let next = t;
  let stopAt = Infinity;
  const live: { src: AudioBufferSourceNode; end: number }[] = [];
  const noise = noiseBuffers(ctx).white;

  const schedule = (now: number) => {
    // Retire finished grains.
    for (let i = live.length - 1; i >= 0; i--) if (live[i].end < now) live.splice(i, 1);
    const until = Math.min(now + AHEAD_S, stopAt);
    if (next < now) next = now;
    while (next < until && level > 0.01) {
      // Density 12..170 grains/s, longer grains for big crushes.
      const density = 12 + 158 * level * level;
      const dur = 0.018 + 0.05 * level * (0.5 + r());
      if (live.length < MAX_LIVE) {
        const src = ctx.createBufferSource();
        const g = gainNode(ctx, 0);
        let filt: AudioNode | null = null;
        if (buffers.length) {
          const buf = buffers[Math.floor(r() * buffers.length)];
          src.buffer = buf;
          src.playbackRate.value = pitch * (0.8 + 0.45 * r());
          const off = Math.max(0, (buf.duration - dur - 0.01) * r());
          src.connect(g);
          src.start(next, off, dur * 2);
        } else {
          src.buffer = noise;
          const bp = biquad(ctx, "bandpass", (350 + 2600 * r()) * pitch, 2.5);
          src.connect(bp).connect(g);
          filt = bp;
          src.start(next, r() * 1.8, dur * 2);
        }
        const amp = (0.25 + 0.5 * r()) * (0.5 + 0.5 * level);
        g.gain.setValueAtTime(0, next);
        g.gain.linearRampToValueAtTime(amp, next + Math.min(0.004, dur * 0.3));
        g.gain.setValueAtTime(amp, next + dur * 0.5);
        g.gain.linearRampToValueAtTime(0, next + dur);
        g.connect(dest);
        src.stop(next + dur + 0.005);
        src.onended = () => {
          src.disconnect();
          filt?.disconnect();
          g.disconnect();
        };
        live.push({ src, end: next + dur + 0.01 });
      }
      next += (1 / density) * (0.5 + r());
    }
  };
  const setTone = (now: number) => {
    tone.frequency.setTargetAtTime(1200 + 7000 * level, now, 0.02);
  };
  setTone(t);
  schedule(t);
  return {
    output: out,
    update(np: VoiceParams, now: number) {
      level = clamp01(np.load);
      pitch = np.pitch || 1;
      setTone(now);
      schedule(now);
    },
    stop(at: number) {
      stopAt = Math.min(stopAt, at);
      for (const g of live) {
        try {
          if (g.end > at) g.src.stop(at);
        } catch {
          // not started
        }
      }
    },
  };
}
