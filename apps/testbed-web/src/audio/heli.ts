// Police helicopter audio (PR #32), outside the Rust director:
//  - the pilot radio: lines starting "Air <n>" (wp_police's helicopter call sign) get a
//    rotor-chopped cockpit bed instead of the cars' static, through a narrower, harder-driven
//    radio EQ than the patrol cars, and (unless the line is tier 1, which already gets the
//    two-tone priority alert) a two-tone mic-key chirp, so it is obvious the helicopter is
//    talking. The lines themselves are paced and filtered by the director's priority tiers
//    ("Radio: All / Important only") like every other police line;
//  - the rotor in the world: one looped thump panned at the nearest helicopter, fading with
//    distance from the listener.
// Everything here is scaled by HELI_LOUDNESS.
//
// Audio-thread cost (PR #53, voices.ts): the rotor's panner position is a plain per-block
// `.value` write, never automated; only its gain glides.

import type { AudioMixer } from "./voices";

/** Helicopter level relative to the original mix (+12 %, playtest ask: "~10-15 % louder"). */
export const HELI_LOUDNESS = 1.12;

/** A line spoken by a helicopter pilot (`wp_police` starts every one with "Air <n>"). */
export function isAirLine(text: string): boolean {
  return /^Air \d+\b/.test(text);
}

/** Blade-pass rate of the rotor thump, Hz. */
const ROTOR_HZ = 11;
/** In-world rotor gain at the helicopter (before distance and HELI_LOUDNESS). */
const WORLD_ROTOR_GAIN = 0.35;
/** The in-world rotor stops after this long with no helicopter in range, s (per-frame count). */
const ROTOR_IDLE_STOP_S = 5;
/** Distance at which the in-world rotor fades out, m. */
const WORLD_ROTOR_RANGE_M = 700;

/** Pilot radio and in-world rotor (one per AudioSystem). */
export class HeliAudio {
  private readonly noise: AudioBuffer;
  /** Pilot radio EQ input (feeds the Police bus). */
  private readonly pilotIn: GainNode;
  private world: { gain: GainNode; panner: PannerNode; level: number; src: AudioBufferSourceNode; lfo: OscillatorNode; silentS: number } | null = null;
  /** Times the pilot keyed the mic (e2e). */
  keyUps = 0;

  constructor(
    private readonly ctx: BaseAudioContext,
    private readonly mixer: AudioMixer,
  ) {
    // Fixed noise (a seeded LCG, not Math.random: renders are repeatable).
    const n = ctx.sampleRate * 2;
    this.noise = ctx.createBuffer(1, n, ctx.sampleRate);
    const d = this.noise.getChannelData(0);
    let x = 0x2545f491;
    for (let i = 0; i < n; i++) {
      x = (Math.imul(x, 1664525) + 1013904223) >>> 0;
      d[i] = x / 0x80000000 - 1;
    }

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
      const v = (i / 255) * 2 - 1;
      curve[i] = Math.tanh(5 * v) / Math.tanh(5);
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

  /**
   * The pilot keys the mic: rotor-chopped cockpit bed, and the two-tone mic-key chirp when
   * `chirp` (tier 1 lines skip it: they already get the priority alert). Returns the un-key
   * function.
   */
  keyUp(chirp: boolean): () => void {
    const ctx = this.ctx;
    const t = ctx.currentTime;
    this.keyUps++;
    if (chirp) {
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
    }
    const bed = this.rotor(1100, 0.6);
    const level = ctx.createGain();
    level.gain.setValueAtTime(0, t);
    level.gain.linearRampToValueAtTime(0.22, t + 0.08);
    bed.out.connect(level).connect(this.pilotIn);
    return () => {
      const now = ctx.currentTime;
      level.gain.cancelScheduledValues(now);
      level.gain.setValueAtTime(level.gain.value, now);
      level.gain.linearRampToValueAtTime(0, now + 0.15);
      bed.src.stop(now + 0.3);
      bed.lfo.stop(now + 0.3);
    };
  }

  /**
   * In-world rotor: `helis` = helicopter positions, flat `[x, y, z, …]` every `stride` floats
   * (sim space; police_helicopters() rows), `listener` = camera position. Plays at the nearest
   * one; silent when none is in range.
   */
  /** Stop the in-world rotor (session reset); the next helicopter in range starts it again. */
  stopWorld(): void {
    const w = this.world;
    if (!w) return;
    const now = this.ctx.currentTime;
    w.gain.gain.cancelScheduledValues(now);
    w.gain.gain.setTargetAtTime(0, now, 0.05);
    w.src.stop(now + 0.3);
    w.lfo.stop(now + 0.3);
    this.world = null;
  }

  updateWorld(helis: ArrayLike<number>, stride: number, listener: number[]): void {
    let best = -1;
    let bestD = Infinity;
    for (let o = 0; o + 2 < helis.length; o += stride) {
      const d = Math.hypot(helis[o] - listener[0], helis[o + 1] - listener[1], helis[o + 2] - listener[2]);
      if (d < bestD) {
        bestD = d;
        best = o;
      }
    }
    const falloff = Math.max(0, 1 - bestD / WORLD_ROTOR_RANGE_M);
    const target = best >= 0 ? WORLD_ROTOR_GAIN * HELI_LOUDNESS * falloff ** 1.5 : 0;
    if (!this.world && target > 0) {
      const r = this.rotor(320, 0.75);
      const gain = this.ctx.createGain();
      gain.gain.value = 0;
      // Panning only: distance is folded into the gain above (like the director's voices).
      const panner = this.ctx.createPanner();
      panner.panningModel = "equalpower";
      panner.distanceModel = "linear";
      panner.rolloffFactor = 0;
      r.out.connect(gain).connect(panner).connect(this.mixer.input("World"));
      this.world = { gain, panner, level: 0, src: r.src, lfo: r.lfo, silentS: 0 };
    }
    const w = this.world;
    if (!w) return;
    // No helicopter in range for a while: stop the looped rotor instead of running it at
    // gain 0 for the rest of the session.
    w.silentS = target > 0 ? 0 : w.silentS + 1 / 60;
    if (w.silentS > ROTOR_IDLE_STOP_S) {
      this.stopWorld();
      return;
    }
    // Only re-automate on a real change (cheap path otherwise).
    if (Math.abs(target - w.level) > 0.002 || (target === 0 && w.level !== 0)) {
      w.gain.gain.setTargetAtTime(target, this.ctx.currentTime, 0.2);
      w.level = target;
    }
    if (best >= 0) {
      const p = w.panner;
      if (p.positionX) {
        p.positionX.value = helis[best];
        p.positionY.value = helis[best + 1];
        p.positionZ.value = helis[best + 2];
      } else {
        p.setPosition(helis[best], helis[best + 1], helis[best + 2]);
      }
    }
  }
}
