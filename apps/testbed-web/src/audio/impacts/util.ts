// Small node-graph helpers shared by the impact / crunch / scrape synths (owner row 74).
// Kept separate from synth.ts (the engine synth's file, machine J) on purpose.

/** Deterministic LCG in [0, 1): the director sends a cosmetic seed per hit. */
export function rng(seed: number): () => number {
  let s = (Math.floor(seed) * 2654435761 + 12345) >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

export const clamp01 = (x: number) => Math.min(1, Math.max(0, x || 0));
export const clampF = (f: number, lo = 20, hi = 18000) => Math.min(hi, Math.max(lo, f || lo));

export function gainNode(ctx: BaseAudioContext, v: number): GainNode {
  const g = ctx.createGain();
  g.gain.value = v;
  return g;
}

export function biquad(ctx: BaseAudioContext, type: BiquadFilterType, f: number, q = 0.7, gainDb = 0): BiquadFilterNode {
  const b = ctx.createBiquadFilter();
  b.type = type;
  b.frequency.value = clampF(f, 10, ctx.sampleRate / 2 - 10);
  b.Q.value = q;
  b.gain.value = gainDb;
  return b;
}

const noiseCache = new WeakMap<BaseAudioContext, { white: AudioBuffer; crackle: AudioBuffer }>();

/** 2 s of white noise and of sparse crackle (spark / grit texture), built once per context. */
export function noiseBuffers(ctx: BaseAudioContext): { white: AudioBuffer; crackle: AudioBuffer } {
  let b = noiseCache.get(ctx);
  if (b) return b;
  const len = Math.floor(ctx.sampleRate * 2);
  const white = ctx.createBuffer(1, len, ctx.sampleRate);
  const crackle = ctx.createBuffer(1, len, ctx.sampleRate);
  const r = rng(77);
  const w = white.getChannelData(0);
  const c = crackle.getChannelData(0);
  let last = 0;
  for (let i = 0; i < len; i++) {
    w[i] = r() * 2 - 1;
    // Sparks: rare sharp spikes with a very short decay.
    last = r() < 0.0025 ? (r() * 2 - 1) : last * 0.6;
    c[i] = last;
  }
  b = { white, crackle };
  noiseCache.set(ctx, b);
  return b;
}

/** A noise source starting at `t` (random offset so voices do not phase). */
export function noiseSrc(ctx: BaseAudioContext, t: number, kind: "white" | "crackle" = "white", loop = true, off = Math.random() * 1.8): AudioBufferSourceNode {
  const src = ctx.createBufferSource();
  src.buffer = noiseBuffers(ctx)[kind];
  src.loop = loop;
  src.start(t, off);
  return src;
}

/** Linear attack then exponential decay (time constant `tau`) on a gain param. */
export function hitEnv(param: AudioParam, t: number, attack: number, peak: number, tau: number): void {
  param.setValueAtTime(0, t);
  param.linearRampToValueAtTime(peak, t + attack);
  param.setTargetAtTime(0, t + attack, Math.max(tau, 0.002));
}

export function stopAll(nodes: AudioScheduledSourceNode[], t: number): void {
  for (const n of nodes) {
    try {
      n.stop(t);
    } catch {
      // already stopped
    }
  }
}

const curveCache = new Map<string, Float32Array<ArrayBuffer>>();

/**
 * Asymmetric soft clip: the positive half saturates harder than the negative one, which adds
 * even harmonics (the "buzz" of a loose panel or a horn diaphragm hitting its pole piece).
 */
export function asymShaper(ctx: BaseAudioContext, drive: number, asym: number): WaveShaperNode {
  const key = `${Math.round(drive * 20)}:${Math.round(asym * 20)}`;
  let curve = curveCache.get(key);
  if (!curve) {
    const n = 1024;
    curve = new Float32Array(n);
    const kp = 1 + Math.round(drive * 20) / 20 * 6;
    const kn = kp * (1 - Math.round(asym * 20) / 20 * 0.6);
    for (let i = 0; i < n; i++) {
      const x = (i / (n - 1)) * 2 - 1;
      curve[i] = x >= 0 ? Math.tanh(kp * x) / Math.tanh(kp) : Math.tanh(kn * x) / Math.tanh(kp);
    }
    curveCache.set(key, curve);
  }
  const s = ctx.createWaveShaper();
  s.curve = curve;
  return s;
}
