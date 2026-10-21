// Flat-tyre flap synth (`flap`, playtest row 132): the loose carcass of a deflated tyre slaps
// the road once per wheel revolution. `freq_hz` = revolutions per second (wp_audio::flat_tyre),
// `load` = deflation (looser = duller, heavier slap). An oscillator with a one-sided pulse wave
// drives the gain of a band-passed noise slap and a low thump, so the slap *rate* follows wheel
// speed while the slap itself keeps its pitch; above ~12 rev/s the slaps merge into a thrum.

import type { SynthVoice } from "./synth";
import type { VoiceParams } from "./types";

type Params = Record<string, number>;

const pulses = new WeakMap<BaseAudioContext, { wave: PeriodicWave; noise: AudioBuffer }>();

function shared(ctx: BaseAudioContext) {
  let s = pulses.get(ctx);
  if (s) return s;
  // One-sided decaying pulse per period (attack 2 %, decay τ = 9 % of the period), as Fourier
  // coefficients of the periodic wave.
  const n = 64;
  const N = 2048;
  const shape = new Float32Array(N);
  for (let i = 0; i < N; i++) {
    const t = i / N;
    shape[i] = t < 0.02 ? t / 0.02 : Math.exp(-(t - 0.02) / 0.09);
  }
  const re = new Float32Array(n);
  const im = new Float32Array(n);
  for (let k = 1; k < n; k++) {
    let a = 0;
    let b = 0;
    for (let i = 0; i < N; i++) {
      const ph = (2 * Math.PI * k * i) / N;
      a += shape[i] * Math.cos(ph);
      b += shape[i] * Math.sin(ph);
    }
    re[k] = (2 * a) / N;
    im[k] = (2 * b) / N;
  }
  const wave = ctx.createPeriodicWave(re, im, { disableNormalization: false });
  // Deterministic noise (same LCG idea as synth.ts).
  const len = Math.floor(ctx.sampleRate);
  const noise = ctx.createBuffer(1, len, ctx.sampleRate);
  const d = noise.getChannelData(0);
  let x = 4242;
  for (let i = 0; i < len; i++) {
    x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
    d[i] = (x / 4294967296) * 2 - 1;
  }
  s = { wave, noise };
  pulses.set(ctx, s);
  return s;
}

/** Rectify-and-square: the bipolar pulse oscillator → a 0..1 envelope. */
function rectifier(ctx: BaseAudioContext): WaveShaperNode {
  const c = new Float32Array(1024);
  for (let i = 0; i < c.length; i++) {
    const x = (i / (c.length - 1)) * 2 - 1;
    // The DC-free pulse peaks near +0.8 and idles around −0.2: everything under 0 is silence.
    const y = Math.max(0, x);
    c[i] = Math.min(1, y * y * 1.6);
  }
  const s = ctx.createWaveShaper();
  s.curve = c;
  return s;
}

const clampHz = (f: number) => Math.min(40, Math.max(0.2, f || 0.2));

export function flapSynth(ctx: BaseAudioContext, s: Params, p: VoiceParams): SynthVoice {
  const { wave, noise } = shared(ctx);
  const t = ctx.currentTime;
  const out = ctx.createGain();
  out.gain.value = s.level ?? 1.4;
  // Envelope: pulse oscillator at the revolution rate → rectifier → gate gain.
  const lfo = ctx.createOscillator();
  lfo.setPeriodicWave(wave);
  lfo.frequency.value = clampHz(p.freq_hz);
  const gate = ctx.createGain();
  gate.gain.value = 0;
  lfo.connect(rectifier(ctx)).connect(gate.gain);
  // The slap: band-passed noise (rubber on asphalt) + a low thump (the wheel landing on it).
  const src = ctx.createBufferSource();
  src.buffer = noise;
  src.loop = true;
  const band = ctx.createBiquadFilter();
  band.type = "bandpass";
  band.frequency.value = 520;
  band.Q.value = 0.9;
  const tone = ctx.createBiquadFilter();
  tone.type = "lowpass";
  tone.frequency.value = 900 + 900 * (1 - (p.load || 0));
  const thump = ctx.createOscillator();
  thump.type = "sine";
  thump.frequency.value = 72;
  const thumpGain = ctx.createGain();
  thumpGain.gain.value = 0.7;
  src.connect(band).connect(tone).connect(gate);
  thump.connect(thumpGain).connect(gate);
  gate.connect(out);
  lfo.start(t);
  src.start(t, Math.random() * 0.9);
  thump.start(t);
  return {
    output: out,
    update(p2, now) {
      lfo.frequency.setTargetAtTime(clampHz(p2.freq_hz), now, 0.04);
      tone.frequency.setTargetAtTime(900 + 900 * (1 - (p2.load || 0)), now, 0.1);
    },
    stop(end) {
      for (const n of [lfo, src, thump]) {
        try {
          n.stop(end);
        } catch {
          // already stopped
        }
      }
    },
  };
}

/** Offline render of the flap at `revHz` revolutions per second (e2e). */
export async function renderFlap(revHz: number, seconds: number, load = 0.8, sampleRate = 48000): Promise<Float32Array> {
  const ctx = new OfflineAudioContext(1, Math.ceil(seconds * sampleRate), sampleRate);
  const p: VoiceParams = { gain: 1, pitch: 1, freq_hz: revHz, load, phase: 0, lowpass_hz: 18000, doppler: 1, pos: [0, 0, 0] };
  const v = flapSynth(ctx, {}, p);
  v.output.connect(ctx.destination);
  return (await ctx.startRendering()).getChannelData(0);
}
