// Helicopter (Air-1) audio, outside the Rust director:
//  - the pilot radio filter: lines starting "Air-<n>" (wp_police's helicopter call sign) get a
//    two-tone mic-key chirp and a rotor-chopped cockpit bed through a narrower, harder-driven
//    radio EQ than the patrol cars, so it is obvious the helicopter is talking;
//  - the rotor in the world: one looped thump panned at the nearest helicopter, fading with
//    distance from the listener.
// Everything here is scaled by HELI_LOUDNESS.

import type { AudioMixer } from "./voices";

/** Helicopter level relative to the original mix (+12%, playtest ask: "~10-15% louder"). */
export const HELI_LOUDNESS = 1.12;

/** A line spoken by the helicopter pilot (`wp_police` starts every one with "Air-<n>"). */
export function isAirLine(text: string): boolean {
  return /^Air-\d/.test(text);
}

/** Blade-pass rate of the rotor thump, Hz. */
const ROTOR_HZ = 11;
/** In-world rotor gain at the helicopter (before distance and HELI_LOUDNESS). */
const WORLD_ROTOR_GAIN = 0.35;
/** Distance at which the in-world rotor fades out, m. */
const WORLD_ROTOR_RANGE_M = 700;

export class HeliAudio {
  private readonly noise: AudioBuffer;
  /** Pilot radio EQ input (feeds the Police bus). */
  private readonly pilotIn: GainNode;
  private world: { gain: GainNode; panner: PannerNode } | null = null;

  constructor(
    private readonly ctx: AudioContext,
    private readonly mixer: AudioMixer,
  ) {
    const n = ctx.sampleRate * 2;
    this.noise = ctx.createBuffer(1, n, ctx.sampleRate);
    const d = this.noise.getChannelData(0);
    for (let i = 0; i < n; i++) d[i] = Math.random() * 2 - 1;

    // Narrower band and more drive than the cars' radio EQ (voices.ts): a headset mic over
    // turbine noise.
    this.pilotIn = ctx.createGain();
    const hp = ctx.createBiquadFilter();
    hp.type = "highpass";
    hp.frequency.value = 550;
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 2600;
    const mid = ctx.createBiquadFilter();
    mid.type = "peaking";
    mid.frequency.value = 1300;
    mid.gain.value = 8;
    const drive = ctx.createWaveShaper();
    const curve = new Float32Array(256);
    for (let i = 0; i < 256; i++) {
      const x = (i / 255) * 2 - 1;
      curve[i] = Math.tanh(5 * x) / Math.tanh(5);
    }
    drive.curve = curve;
    const out = ctx.createGain();
    out.gain.value = HELI_LOUDNESS;
    this.pilotIn.connect(hp).connect(lp).connect(mid).connect(drive).connect(out).connect(mixer.input("Police"));
  }

  /** Noise source → lowpass → gain whose level is chopped at the rotor rate. */
  private rotor(lowpassHz: number, depth: number): { src: AudioBufferSourceNode; lfo: OscillatorNode; out: GainNode } {
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this.noise;
    src.loop = true;
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = lowpassHz;
    const out = ctx.createGain();
    out.gain.value = 1 - depth;
    const lfo = ctx.createOscillator();
    lfo.type = "triangle";
    lfo.frequency.value = ROTOR_HZ;
    const lfoGain = ctx.createGain();
    lfoGain.gain.value = depth;
    lfo.connect(lfoGain).connect(out.gain);
    src.connect(lp).connect(out);
    src.start();
    lfo.start();
    return { src, lfo, out };
  }

  /** Pilot keys the mic: chirp + rotor-chopped cockpit bed. Returns the un-key function. */
  keyUp(): () => void {
    const ctx = this.ctx;
    const t = ctx.currentTime;
    // Two-tone mic-key chirp (the cars only get the squelch click).
    for (const [i, hz] of [1400, 1900].entries()) {
      const o = ctx.createOscillator();
      o.frequency.value = hz;
      const g = ctx.createGain();
      g.gain.setValueAtTime(0, t + i * 0.06);
      g.gain.linearRampToValueAtTime(0.12, t + i * 0.06 + 0.005);
      g.gain.setValueAtTime(0.12, t + i * 0.06 + 0.045);
      g.gain.linearRampToValueAtTime(0, t + i * 0.06 + 0.055);
      o.connect(g).connect(this.pilotIn);
      o.start(t + i * 0.06);
      o.stop(t + i * 0.06 + 0.06);
    }
    const bed = this.rotor(1100, 0.6);
    const level = ctx.createGain();
    level.gain.setValueAtTime(0, t);
    level.gain.linearRampToValueAtTime(0.22, t + 0.08);
    bed.out.connect(level).connect(this.pilotIn);
    return () => {
      const now = ctx.currentTime;
      level.gain.setTargetAtTime(0, now, 0.05);
      bed.src.stop(now + 0.3);
      bed.lfo.stop(now + 0.3);
    };
  }

  /**
   * In-world rotor: `helis` = helicopter positions [x, y, z] (sim space), `listener` = camera
   * position. Plays at the nearest one; silent when none is in range.
   */
  updateWorld(helis: number[][], listener: number[]): void {
    let best: number[] | null = null;
    let bestD = Infinity;
    for (const h of helis) {
      const d = Math.hypot(h[0] - listener[0], h[1] - listener[1], h[2] - listener[2]);
      if (d < bestD) {
        bestD = d;
        best = h;
      }
    }
    const falloff = Math.max(0, 1 - bestD / WORLD_ROTOR_RANGE_M);
    const target = best ? WORLD_ROTOR_GAIN * HELI_LOUDNESS * falloff ** 1.5 : 0;
    if (!this.world && target > 0) {
      const r = this.rotor(320, 0.75);
      const gain = this.ctx.createGain();
      gain.gain.value = 0;
      // Panning only: distance is folded into the gain above (like the director's voices).
      const panner = new PannerNode(this.ctx, { panningModel: "equalpower", rolloffFactor: 0 });
      r.out.connect(gain).connect(panner).connect(this.mixer.input("World"));
      this.world = { gain, panner };
    }
    if (!this.world) return;
    const t = this.ctx.currentTime;
    this.world.gain.gain.setTargetAtTime(target, t, 0.2);
    if (best) {
      this.world.panner.positionX.setTargetAtTime(best[0], t, 0.05);
      this.world.panner.positionY.setTargetAtTime(best[1], t, 0.05);
      this.world.panner.positionZ.setTargetAtTime(best[2], t, 0.05);
    }
  }
}
