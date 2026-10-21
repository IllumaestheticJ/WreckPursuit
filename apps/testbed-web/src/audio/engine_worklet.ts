// Physically informed engine synthesizer (playtest row 73): one AudioWorklet processes every
// modelled engine (the player car and up to six nearby cars), sample-accurately.
//
// Per engine, per sample:
//   crank angle advances with rpm → each combustion event (angle, bank, header length and a
//   fixed per-cylinder trim from wp_audio::engine_model::EngineVoiceSpec) starts an exhaust
//   blowdown pulse whose strength follows load (weak pumping pulses on overrun, none on a
//   rev-limiter cut or a misfire) → per-bank header waveguide (comb) → collector (single
//   exhaust) or separate pipes (dual) → muffler (low-pass + chamber resonance + straight-
//   through leak) → tailpipe waveguide (quarter-wave resonances) → open-end radiation (d/dt).
//   Plus intake (airbox resonance excited by the pulses + throttle hiss), mechanical noise
//   (valvetrain ticks per event, broadband), gearbox and differential whine (gear, road speed),
//   turbo whistle / blow-off and supercharger whine, and overrun / limiter / misfire crackle
//   (afterfire pops injected into the tailpipe).
// Interior perspective: exhaust heard through the body (low-passed, cabin boom), intake and
//   mechanical noise forward; exterior: exhaust dominant. Distance, air absorption and panning
//   stay with the voice strip (VoicePool); Doppler scales the engine's time base.
//
// The whole processor lives inside `engineWorkletMain` so it can be shipped to the audio thread
// as a Blob (`engineWorkletSource()`): the function only references its own locals. Called on
// the main thread / in Node with `null` it just returns the DSP class (offline renders, tests).
// Audio never feeds the simulation; the synthesizer's randomness is its own (seeded per car).

/**
 * Engines one worklet node can hold: the director models at most 7 at a time; the rest are
 * spares for voices still fading out when the director re-plays (a reset or source switch
 * re-plays every loop at once). Idle slots cost nothing.
 */
export const ENGINE_SLOTS = 16;

/** Per-slot fields of the frame block posted every frame (`EngineSynthHost.flush`). */
export const EF = {
  active: 0,
  rpm: 1,
  throttle: 2,
  pedal: 3,
  load: 4,
  gear: 5,
  speed: 6,
  boost: 7,
  health: 8,
  blowoffs: 9,
  doppler: 10,
  interior: 11,
} as const;
/** Floats per slot in the frame block. */
export const EF_COUNT = 12;

/** Worklet processor name. */
export const ENGINE_PROCESSOR = "wp-engine";

/** What the audio thread's global scope offers (subset). */
interface WorkletScope {
  registerProcessor?: (name: string, ctor: unknown) => void;
  AudioWorkletProcessor?: new () => { port: MessagePort };
  sampleRate?: number;
}

/** One engine's DSP state (see the file comment). */
export interface EngineDspApi {
  /** Set the frame targets (same layout as one slot of the frame block). */
  setFrame(f: ArrayLike<number>, samplesPerFrame: number): void;
  /** Render `n` samples into `out` starting at `off` (overwrites). */
  render(out: Float32Array, off: number, n: number): void;
  /** Counters for tests: events fired, misfires, pops, limiter cuts. */
  readonly counters: { events: number; misfires: number; pops: number; cuts: number; blowoffs: number };
  /** Sample index (since this engine's first render) of each pop's onset, first 256 (tests). */
  readonly popAt: number[];
}

/** Constructor of [`EngineDspApi`]. */
export type EngineDspCtor = new (sampleRate: number, spec: Record<string, number>) => EngineDspApi;

/**
 * The engine processor. With an AudioWorklet scope it registers `wp-engine`; it always returns
 * the DSP class. Must stay self-contained (it is stringified into the worklet).
 */
export function engineWorkletMain(scope: WorkletScope | null): { EngineDsp: EngineDspCtor } {
  const TAU = 2 * Math.PI;
  const SLOTS = 16;
  const FIELDS = 12;
  // Field indices (EF).
  const ACTIVE = 0,
    RPM = 1,
    THROTTLE = 2,
    PEDAL = 3,
    LOAD = 4,
    GEAR = 5,
    SPEED = 6,
    BOOST = 7,
    HEALTH = 8,
    BLOWOFFS = 9,
    DOPPLER = 10,
    INTERIOR = 11;
  const MAX_PULSES = 32;
  const MAX_POPS = 6;
  const PULSE_TABLE = 256;
  const DELAY = 8192; // power of two
  const MASK = DELAY - 1;
  /**
   * Output scale, in two parts around the voice's soft safety limit (linear to ±0.8):
   * `OUT_DRIVE` sets the level into the limit, `OUT_MAKEUP` the level out of it (their product,
   * 1.8, is what the `engine_physical` cue gain is calibrated against, recorded vs modelled
   * within 1.5 dB). The loudest WOT pull (flat-plane V8) peaks at 0.39 into the limit: 6 dB under
   * its knee, the same headroom the master stage keeps under its soft clip (docs/integration/
   * audio.md "Mix headroom"), so only a fault or stacked bangs ever reach it. The total was
   * 3.6 into the limit, which drove every V8 / I6 into the tanh knee at full throttle (4-11 %
   * of samples: harshness, playtest "engine sounds are bad"); then 1.8 into the limit, which
   * left the loudest V8 0.2 dB under the knee.
   */
  const OUT_DRIVE = 0.9;
  const OUT_MAKEUP = 2.0;
  /**
   * Open-end radiation: the far field follows d/dt of the tailpipe flow above ~RAD_HZ and is
   * flat below it (a leaky differentiator). A pure d/dt (the old model) took the firing order
   * 6 dB/oct down against its harmonics, so the I4's firing order sat ~10 dB under its 2nd-3rd
   * harmonics where real recordings have it on top.
   */
  const RAD_HZ = 250;
  /**
   * Header / tailpipe end reflections. Real pipes are lossy; the old -0.42 / -0.55 made a
   * comb whose peaks and notches swept through the firing harmonics as the rpm rose: up to
   * 7 dB holes in a WOT pull (detrended level ripple std 2.1-2.8 dB, now 0.7-0.9).
   */
  const HDR_R = -0.22;
  const TAIL_R = -0.25;
  /**
   * Turbulent flow noise riding on each blowdown pulse (pulse × (1 + noise), noise low-passed
   * twice at FLOW_LP ≈ 1.2 kHz, deeper with load): the old pulses were perfectly periodic
   * (spectral flatness -26 to -33 dB vs -10 to -23 dB in the repo's engine recordings), the
   * "buzzer" character. Kept below 2 kHz so it adds rasp, not hiss.
   */
  const FLOW_NOISE = 1.5;
  const FLOW_LP = 0.15;
  /** One-pole high-pass coefficient of the flow noise (≈ 2π·300 Hz / 48 kHz). */
  const FLOW_HP = 0.04;
  /**
   * Phase dispersion of the exhaust (allpasses, no change of the spectrum): with every
   * harmonic phase-aligned a pulse train is peaky (crest ~13 dB once the radiation keeps the low
   * end); a real muffler smears the phases. Keeps the peaks off the soft limit at the old loudness.
   */
  const DISPERSION_HZ = [140, 420, 1100];
  /** Afterfire pop level into the tailpipe (see `pop` in `render`). */
  const POP_GAIN = 0.6;
  /** Cycle-to-cycle combustion variation (± half of it), at idle / above. */
  const CYCLE_VAR_IDLE = 0.24;
  const CYCLE_VAR = 0.14;

  // Exhaust blowdown pulse: fast rise (first 22 %), smooth fall. The rise is a quintic
  // smoothstep (continuous slope and curvature at the onset): after the open end's d/dt a
  // curvature jump would be an edge, heard as a tick on every firing at low rpm.
  const ATTACK = 0.22;
  const pulseTable = new Float32Array(PULSE_TABLE + 2);
  for (let i = 0; i <= PULSE_TABLE + 1; i++) {
    const x = Math.min(i / PULSE_TABLE, 1);
    const a = x / ATTACK;
    const v = x < ATTACK ? a * a * a * (a * (a * 6 - 15) + 10) : Math.pow(0.5 + 0.5 * Math.cos((Math.PI * (x - ATTACK)) / (1 - ATTACK)), 2.2);
    pulseTable[i] = i > PULSE_TABLE ? 0 : v;
  }

  const clamp = (x: number, lo: number, hi: number) => (x < lo ? lo : x > hi ? hi : x);

  /** Direct-form-I biquad (RBJ cookbook). */
  class Biquad {
    b0 = 1;
    b1 = 0;
    b2 = 0;
    a1 = 0;
    a2 = 0;
    x1 = 0;
    x2 = 0;
    y1 = 0;
    y2 = 0;
    private set(b0: number, b1: number, b2: number, a0: number, a1: number, a2: number) {
      this.b0 = b0 / a0;
      this.b1 = b1 / a0;
      this.b2 = b2 / a0;
      this.a1 = a1 / a0;
      this.a2 = a2 / a0;
    }
    lowpass(f: number, q: number, sr: number) {
      const w = (TAU * clamp(f, 10, sr * 0.45)) / sr;
      const c = Math.cos(w);
      const al = Math.sin(w) / (2 * q);
      this.set((1 - c) / 2, 1 - c, (1 - c) / 2, 1 + al, -2 * c, 1 - al);
    }
    bandpass(f: number, q: number, sr: number) {
      // Constant 0 dB peak gain.
      const w = (TAU * clamp(f, 10, sr * 0.45)) / sr;
      const c = Math.cos(w);
      const al = Math.sin(w) / (2 * q);
      this.set(al, 0, -al, 1 + al, -2 * c, 1 - al);
    }
    allpass(f: number, q: number, sr: number) {
      const w = (TAU * clamp(f, 10, sr * 0.45)) / sr;
      const c = Math.cos(w);
      const al = Math.sin(w) / (2 * q);
      this.set(1 - al, -2 * c, 1 + al, 1 + al, -2 * c, 1 - al);
    }
    peaking(f: number, q: number, db: number, sr: number) {
      const w = (TAU * clamp(f, 10, sr * 0.45)) / sr;
      const c = Math.cos(w);
      const A = Math.pow(10, db / 40);
      const al = Math.sin(w) / (2 * q);
      this.set(1 + al * A, -2 * c, 1 - al * A, 1 + al / A, -2 * c, 1 - al / A);
    }
    run(x: number): number {
      const y = this.b0 * x + this.b1 * this.x1 + this.b2 * this.x2 - this.a1 * this.y1 - this.a2 * this.y2;
      this.x2 = this.x1;
      this.x1 = x;
      this.y2 = this.y1;
      this.y1 = y;
      return y;
    }
    flush() {
      if (Math.abs(this.y1) < 1e-25) this.y1 = 0;
      if (Math.abs(this.y2) < 1e-25) this.y2 = 0;
      if (Math.abs(this.x1) < 1e-25) this.x1 = 0;
      if (Math.abs(this.x2) < 1e-25) this.x2 = 0;
    }
    reset() {
      this.x1 = this.x2 = this.y1 = this.y2 = 0;
    }
  }

  /**
   * Tube waveguide folded into a comb: y[n] = x[n] + r · loss(y[n − D]), D = round trip
   * 2L/c in samples (fractional, linearly interpolated). r < 0 for an open end: resonances
   * at odd multiples of c / 4L. `loss` is a one-pole low-pass (wall friction, radiation).
   */
  class Tube {
    buf = new Float32Array(DELAY);
    w = 0;
    d = 10;
    r = -0.5;
    lp = 0;
    k = 0.5;
    run(x: number): number {
      const rp = this.w - this.d;
      const i = Math.floor(rp);
      const f = rp - i;
      const a = this.buf[i & MASK];
      const b = this.buf[(i + 1) & MASK];
      const back = a + (b - a) * f;
      this.lp += (back - this.lp) * this.k;
      const y = x + this.r * this.lp;
      this.buf[this.w & MASK] = y;
      this.w = (this.w + 1) & MASK;
      return y;
    }
    flush() {
      if (Math.abs(this.lp) < 1e-25) this.lp = 0;
    }
  }

  class EngineDsp implements EngineDspApi {
    readonly counters = { events: 0, misfires: 0, pops: 0, cuts: 0, blowoffs: 0 };
    readonly popAt: number[] = [];
    /** Samples rendered so far (pop onsets for tests). */
    private clock = 0;
    private readonly sr: number;
    // ----- spec
    private readonly n: number;
    private readonly gap: Float64Array; // degrees from event i to i+1
    private readonly bank: Uint8Array;
    private readonly hdrS: Float64Array; // header travel time, s
    private readonly trim: Float64Array;
    /** Strength factor from the gap since the same bank's previous event (see constructor). */
    private readonly bankGapK: Float64Array;
    private readonly dual: boolean;
    private readonly rotary: boolean;
    private readonly s: Record<string, number>;
    // ----- smoothed frame state
    private rpm = 0;
    private rpmStep = 0;
    private rpmLeft = 0;
    private cur = new Float64Array(FIELDS);
    private tgt = new Float64Array(FIELDS);
    private blowoffSeen = -1;
    private first = true;
    // ----- engine state
    private toNext = 0;
    private ev = 0;
    private rngState: number;
    private noiseState: number;
    private pAmp = new Float64Array(MAX_PULSES);
    private pPos = new Float64Array(MAX_PULSES);
    private pInc = new Float64Array(MAX_PULSES);
    private pWait = new Float64Array(MAX_PULSES);
    private pBank = new Uint8Array(MAX_PULSES);
    private pCount = 0;
    private popWait = new Float64Array(MAX_POPS);
    private popAmp = new Float64Array(MAX_POPS);
    private popEnv = new Float64Array(MAX_POPS);
    /** Samples since the pop started (attack ramp). */
    private popAge = new Float64Array(MAX_POPS);
    private popNoise = 0;
    private popBank = new Uint8Array(MAX_POPS);
    private popCount = 0;
    private tick = 0;
    private liftT = 10;
    private limPhase = 0;
    private limOn = false;
    private gearPh = 0;
    private diffPh = 0;
    private scPh = 0;
    private turboPh = 0;
    private turboF = 1500;
    private blowEnv = 0;
    private blowF = 2600;
    // ----- filters / waveguides (index = bank)
    private header = [new Tube(), new Tube()];
    private tail = [new Tube(), new Tube()];
    private mufLp = [new Biquad(), new Biquad()];
    private mufRes = [new Biquad(), new Biquad()];
    private radPrev = [0, 0];
    /** Leaky-differentiator pole of the radiation (RAD_HZ). */
    private radLeak = 0;
    /** Flow-noise low-pass states per bank (two poles). */
    private flow = [0, 0];
    private flow2 = [0, 0];
    private flowHp = [0, 0];
    private radLp = [new Biquad(), new Biquad()];
    /** Muffler / pipe dispersion: allpasses that smear the pulse phases (see DISPERSION_HZ). */
    private disp = [new Biquad(), new Biquad(), new Biquad()];
    private dualDelay = new Float32Array(64);
    private dualW = 0;
    private airbox = new Biquad();
    private intakeHiss = new Biquad();
    private tickBp = new Biquad();
    private mechBp = new Biquad();
    private turboBp = new Biquad();
    private blowBp = new Biquad();
    private cabinLp = new Biquad();
    private cabinLp2 = new Biquad();
    private firewallLp = new Biquad();
    private dcX = 0;
    private dcY = 0;
    private fixedDoppler = -1;
    private interior = 0;

    constructor(sampleRate: number, spec: Record<string, number>) {
      this.sr = sampleRate;
      this.s = spec;
      const n = Math.max(1, Math.min(16, Math.round(spec.n ?? 4)));
      this.n = n;
      const ang = new Float64Array(n);
      this.bank = new Uint8Array(n);
      this.hdrS = new Float64Array(n);
      this.trim = new Float64Array(n);
      const cHot = spec.c_hot || 480;
      for (let i = 0; i < n; i++) {
        ang[i] = spec[`a${i}`] ?? (i * 720) / n;
        this.bank[i] = (spec[`b${i}`] ?? 0) > 0.5 ? 1 : 0;
        this.hdrS[i] = (spec[`h${i}`] ?? 0.5) / cHot;
        this.trim[i] = spec[`k${i}`] ?? 1;
      }
      this.gap = new Float64Array(n);
      for (let i = 0; i < n; i++) this.gap[i] = (i + 1 < n ? ang[i + 1] : ang[0] + 720) - ang[i];
      // Pulse interaction: a blowdown soon after the previous one in the same bank meets the
      // residual collector pressure and comes out weaker; after a long gap, stronger. Even-
      // firing banks are unaffected; a cross-plane V8's 90-180-270-180° banks get their lope.
      this.bankGapK = new Float64Array(n);
      const perBank = [0, 0];
      for (let i = 0; i < n; i++) perBank[this.bank[i]]++;
      for (let i = 0; i < n; i++) {
        let prev = -1;
        for (let k = 1; k <= n; k++) {
          const j = (i - k + n) % n;
          if (this.bank[j] === this.bank[i]) {
            prev = j;
            break;
          }
        }
        let g = ang[i] - ang[prev];
        if (g <= 0) g += 720;
        const mean = 720 / Math.max(1, perBank[this.bank[i]]);
        this.bankGapK[i] = Math.pow(g / mean, 0.45);
      }
      this.dual = (spec.dual ?? 0) > 0.5;
      this.rotary = (spec.rotary ?? 0) > 0.5;
      this.rngState = ((spec.seed ?? 1) | 0) || 1;
      this.noiseState = (this.rngState ^ 0x5bd1e995) | 0 || 1;
      this.toNext = 0;
      for (const t of this.header) {
        t.r = HDR_R;
        t.k = 0.55;
      }
      for (const t of this.tail) {
        t.r = TAIL_R;
        t.k = 0.4;
      }
      this.cur[DOPPLER] = this.tgt[DOPPLER] = 1;
      this.cur[HEALTH] = this.tgt[HEALTH] = 1;
    }

    /** Flow noise in [-1, 1) from its own xorshift32 stream (the event randomness — pops,
     * misfires — keeps its own sequence). */
    private noise(): number {
      let x = this.noiseState;
      x ^= x << 13;
      x ^= x >>> 17;
      x ^= x << 5;
      this.noiseState = x;
      return (x >>> 0) / 2147483648 - 1;
    }

    /** xorshift32 → [0, 1). */
    private rand(): number {
      let x = this.rngState;
      x ^= x << 13;
      x ^= x >>> 17;
      x ^= x << 5;
      this.rngState = x;
      return (x >>> 0) / 4294967296;
    }

    setFrame(f: ArrayLike<number>, samplesPerFrame: number): void {
      for (let i = 0; i < FIELDS; i++) this.tgt[i] = f[i];
      const target = Math.max(0, f[RPM]);
      if (this.first) {
        this.first = false;
        for (let i = 0; i < FIELDS; i++) this.cur[i] = f[i];
        this.rpm = target;
        this.rpmLeft = 0;
        this.blowoffSeen = f[BLOWOFFS];
        this.interior = f[INTERIOR];
        return;
      }
      // Linear ramp to the new rpm over one frame: the crank never jumps.
      const nS = Math.max(1, samplesPerFrame);
      this.rpmStep = (target - this.rpm) / nS;
      this.rpmLeft = nS;
      if (f[BLOWOFFS] > this.blowoffSeen) {
        this.blowoffSeen = f[BLOWOFFS];
        this.blowEnv = 1;
        this.blowF = 2800;
        this.counters.blowoffs++;
      }
    }

    /** Block-rate parameter smoothing and filter coefficients. */
    private block(n: number) {
      const a = 1 - Math.exp(-n / (this.sr * 0.012));
      for (let i = 0; i < FIELDS; i++) if (i !== RPM) this.cur[i] += (this.tgt[i] - this.cur[i]) * a;
      this.interior += (this.tgt[INTERIOR] - this.interior) * (1 - Math.exp(-n / (this.sr * 0.08)));
      const dop = clamp(this.cur[DOPPLER], 0.5, 2);
      const sr = this.sr / dop; // the engine's own time base under Doppler
      const s = this.s;
      if (Math.abs(dop - this.fixedDoppler) > 0.002) {
        this.fixedDoppler = dop;
        const cTail = s.c_tail || 410;
        const tailD = ((2 * (s.tail_m ?? 2)) / cTail) * sr;
        const meanHdr = this.hdrS.reduce((x, y) => x + y, 0) / this.n;
        const hdrD = 2 * meanHdr * sr;
        for (let b = 0; b < 2; b++) {
          this.header[b].d = clamp(hdrD, 2, DELAY - 4);
          // The second pipe of a dual system runs a little longer (crossover routing).
          this.tail[b].d = clamp(tailD * (b === 1 ? 1.12 : 1), 2, DELAY - 4);
          const muffler = clamp(s.muffler ?? 0.6, 0, 1);
          const fc = 380 + 2400 * Math.pow(1 - muffler, 1.6);
          this.mufLp[b].lowpass(fc, 0.65, sr);
          // Chamber resonance: broad and moderate (was Q 1.8 / +5 dB, which with the pipe
          // combs made harmonics jump in and out as the rpm swept).
          this.mufRes[b].peaking(s.muffler_hz ?? 200, 1.2, 3, sr);
          // Pipe walls and the open end's directivity roll the radiated highs off.
          this.radLp[b].lowpass(5500, 0.6, sr);
          this.radLeak = Math.exp((-TAU * RAD_HZ) / sr);
        }
        this.airbox.bandpass(s.intake_hz ?? 150, 1.4, sr); // was Q 2.2 (same reason)
        for (let i = 0; i < this.disp.length; i++) this.disp[i].allpass(DISPERSION_HZ[i], 0.8, sr);
        this.tickBp.bandpass(4300, 7, sr);
        this.mechBp.bandpass(1900, 0.8, sr);
        this.cabinLp.lowpass(260, 1.25, sr);
        this.cabinLp2.lowpass(900, 0.7, sr);
        this.firewallLp.lowpass(1700, 0.7, sr);
      }
      const rf = clamp(this.rpm / (s.redline_rpm || 6500), 0, 1.2);
      this.intakeHiss.bandpass(500 + 1500 * rf, 0.9, sr);
      if (s.turbo) {
        const boost = clamp(this.cur[BOOST], 0, 1);
        this.turboF += (1600 + 5600 * Math.pow(boost, 0.8) - this.turboF) * 0.5;
        this.turboBp.bandpass(this.turboF, 5, sr);
      }
      if (this.blowEnv > 1e-4) {
        this.blowF = Math.max(700, this.blowF * Math.exp(-n / (sr * 0.35)));
        this.blowBp.bandpass(this.blowF, 1.6, sr);
      }
      return sr;
    }

    /** A combustion event of cylinder `i`, `frac` of a sample late. */
    private fire(i: number, frac: number, degPerSample: number, sr: number) {
      const s = this.s;
      this.counters.events++;
      const load = clamp(this.cur[LOAD], 0, 1);
      const thr = clamp(this.cur[THROTTLE], 0, 1);
      const redline = s.redline_rpm || 6500;
      const rf = this.rpm / redline;
      // Combustion strength: load sets cylinder pressure; overrun leaves weak pumping pulses.
      // Mass flow per pulse rises with rpm: a WOT pull gets louder (~3 dB/oct) as the
      // radiation is no longer a full d/dt.
      let amp = (0.08 + 0.92 * Math.pow(load, 1.3)) * (0.45 + 0.55 * clamp(rf, 0, 1.1));
      // Cycle-to-cycle variation (more at idle).
      amp *= 1 + (this.rand() - 0.5) * (rf < 0.25 ? CYCLE_VAR_IDLE : CYCLE_VAR);
      amp *= this.trim[i] * this.bankGapK[i];
      let unburnt = 0;
      // Rev limiter: fuel cut pattern while the driver holds it on the limiter.
      if (this.limOn && this.limPhase < 0.55) {
        amp = 0.05;
        unburnt = 0.12;
        this.counters.cuts++;
      }
      // Misfires on a damaged engine (same curve as wp_audio::engine_model::misfire_probability).
      const h = clamp(this.cur[HEALTH], 0, 1);
      const x = clamp((0.7 - h) / 0.7, 0, 1);
      if (this.rand() < 0.35 * x * Math.sqrt(x)) {
        amp = 0.06;
        unburnt = 0.45;
        this.counters.misfires++;
      }
      // Overrun: fuel reaching the hot exhaust after a lift (sporty cars crackle).
      const crackle = clamp(s.crackle ?? 0, 0, 1);
      if (thr < 0.06 && rf > 0.35 && unburnt === 0) {
        // ≈ 5 pops/s right after a lift at 5000 rpm for a sporty V8, fading over ~1 s.
        const p = crackle * (0.006 + 0.07 * Math.exp(-this.liftT / 0.7)) * (0.4 + rf);
        if (this.rand() < p) unburnt = 0.7;
      }
      if (unburnt > 0 && this.rand() < unburnt * (0.3 + 0.6 * crackle)) {
        this.pop(this.bank[i], (0.35 + 0.65 * this.rand()) * (0.5 + crackle) * unburnt, sr * (0.002 + 0.02 * this.rand()));
      }
      // Pulse: narrower (brighter) at high load.
      if (this.pCount < MAX_PULSES) {
        const k = this.pCount++;
        const width = ((s.pulse_deg ?? 70) * (1.25 - 0.45 * load)) / Math.max(degPerSample, 1e-6);
        const inc = PULSE_TABLE / Math.max(width, 3);
        this.pAmp[k] = amp;
        this.pInc[k] = inc;
        this.pPos[k] = frac * inc;
        this.pWait[k] = this.hdrS[i] * sr - frac;
        this.pBank[k] = this.bank[i];
      }
      // Valvetrain: a tick per event (valve seating), louder with rpm.
      this.tick += (s.mech_gain ?? 1) * (0.25 + 0.75 * clamp(rf, 0, 1)) * (0.8 + 0.4 * this.rand());
    }

    private pop(bank: number, amp: number, wait: number) {
      if (this.popCount >= MAX_POPS) return;
      const k = this.popCount++;
      this.popBank[k] = this.dual ? bank : 0;
      this.popAmp[k] = amp;
      this.popWait[k] = wait;
      this.popEnv[k] = 1;
      this.popAge[k] = 0;
      this.counters.pops++;
      if (this.popAt.length < 256) this.popAt.push(this.clock + Math.round(wait));
    }

    render(out: Float32Array, off: number, n: number): void {
      const sr = this.block(n);
      const s = this.s;
      const c = this.cur;
      const thr = clamp(c[THROTTLE], 0, 1);
      const load = clamp(c[LOAD], 0, 1);
      const redline = s.redline_rpm || 6500;
      const revLimit = s.rev_limit_rpm || redline * 1.05;
      const gear = Math.round(c[GEAR]);
      const speed = Math.max(0, c[SPEED]);
      const boost = s.turbo ? clamp(c[BOOST], 0, 1) : 0;
      const dual = this.dual;
      const exhaustGain = s.exhaust_gain ?? 1;
      const muffler = clamp(s.muffler ?? 0.6, 0, 1);
      const leak = (1 - muffler) * 0.9;
      const mufScale = 1 - 0.55 * muffler;
      const intakeGain = s.intake_gain ?? 1;
      const mechGain = s.mech_gain ?? 1;
      const whine = s.whine_gain ?? 0.3;
      const gearAmp = gear < 0 ? 1 : gear === 0 ? 0 : [0, 0.8, 0.5, 0.33, 0.22, 0.16, 0.13, 0.11][Math.min(gear, 7)];
      const moving = clamp(speed / 3, 0, 1);
      const pIn = this.interior;
      const pEx = 1 - pIn;
      const diffHz = speed * (s.diff_hz_per_mps ?? 0);
      const scOn = (s.supercharger ?? 0) > 0.5;
      // Throttle lift timer (overrun crackle is likeliest right after a lift).
      if (thr < 0.06) this.liftT += n / this.sr;
      else this.liftT = 0;
      const limPhaseInc = 13 / this.sr;
      const noiseScaleI = intakeGain * 0.05 * Math.pow(thr, 1.2);
      const blowGain = 0.9;

      for (let j = 0; j < n; j++) {
        // Crank.
        if (this.rpmLeft > 0) {
          this.rpm += this.rpmStep;
          this.rpmLeft--;
        }
        const rpm = this.rpm;
        const rf = rpm / redline;
        this.limOn = rpm >= revLimit - 60 && c[PEDAL] > 0.3;
        this.limPhase += limPhaseInc;
        if (this.limPhase >= 1) this.limPhase -= 1;
        const degPerSample = (rpm * 6) / sr;
        if (rpm > 30) {
          this.toNext -= degPerSample;
          let guard = 0;
          while (this.toNext <= 0 && guard++ < 16) {
            this.fire(this.ev, -this.toNext / degPerSample, degPerSample, sr);
            this.toNext += this.gap[this.ev];
            this.ev = this.ev + 1 < this.n ? this.ev + 1 : 0;
          }
        }
        // Exhaust pulses into the bank headers.
        let x0 = 0;
        let x1 = 0;
        for (let k = 0; k < this.pCount; k++) {
          if (this.pWait[k] > 0) {
            this.pWait[k] -= 1;
            continue;
          }
          const p = this.pPos[k];
          const ii = p | 0;
          const v = pulseTable[ii] + (pulseTable[ii + 1] - pulseTable[ii]) * (p - ii);
          const y = v * this.pAmp[k];
          if (this.pBank[k] === 0) x0 += y;
          else x1 += y;
          this.pPos[k] = p + this.pInc[k];
          if (this.pPos[k] >= PULSE_TABLE) {
            const last = --this.pCount;
            this.pAmp[k] = this.pAmp[last];
            this.pPos[k] = this.pPos[last];
            this.pInc[k] = this.pInc[last];
            this.pWait[k] = this.pWait[last];
            this.pBank[k] = this.pBank[last];
            k--;
          }
        }
        // Flow noise on the pulses (see FLOW_NOISE).
        {
          const fl = FLOW_NOISE * (0.35 + 0.65 * load);
          this.flow[0] += (this.noise() - this.flow[0]) * FLOW_LP;
          this.flow[1] += (this.noise() - this.flow[1]) * FLOW_LP;
          this.flow2[0] += (this.flow[0] - this.flow2[0]) * FLOW_LP;
          this.flow2[1] += (this.flow[1] - this.flow2[1]) * FLOW_LP;
          // High-passed (FLOW_HP): slow noise would modulate whole pulses, i.e. add random
          // sub-harmonics (half orders) to an even-firing engine.
          this.flowHp[0] += (this.flow2[0] - this.flowHp[0]) * FLOW_HP;
          this.flowHp[1] += (this.flow2[1] - this.flowHp[1]) * FLOW_HP;
          x0 += fl * x0 * (this.flow2[0] - this.flowHp[0]);
          x1 += fl * x1 * (this.flow2[1] - this.flowHp[1]);
        }
        // Afterfire pops: sharp noisy bangs entering the tailpipe.
        let pop0 = 0;
        let pop1 = 0;
        for (let k = 0; k < this.popCount; k++) {
          if (this.popWait[k] > 0) {
            this.popWait[k] -= 1;
            continue;
          }
          // A bang: ~1 ms raised-cosine attack, ~6 ms decay, rough (low-passed noise) texture.
          // The attack shapes the output only; `popEnv` is the decay alone. (It used to be
          // overwritten with attack × decay: at age 0 the attack is 0, so every pop died on its
          // first sample — the counters counted crackle, misfire and limiter bangs nobody heard.)
          const age = this.popAge[k]++;
          const att = age < 48 ? 0.5 - 0.5 * Math.cos((Math.PI * age) / 48) : 1;
          const e = this.popEnv[k];
          this.popNoise += (this.noise() - this.popNoise) * 0.1;
          const y = POP_GAIN * this.popAmp[k] * att * e * (0.6 + 1.6 * this.popNoise);
          if (this.popBank[k] === 0) pop0 += y;
          else pop1 += y;
          this.popEnv[k] = e * 0.9965;
          if (this.popEnv[k] < 0.01) {
            const last = --this.popCount;
            this.popAmp[k] = this.popAmp[last];
            this.popWait[k] = this.popWait[last];
            this.popEnv[k] = this.popEnv[last];
            this.popAge[k] = this.popAge[last];
            this.popBank[k] = this.popBank[last];
            k--;
          }
        }
        // Headers → (collector) → muffler → tailpipe → radiation.
        const h0 = this.header[0].run(x0);
        const h1 = this.header[1].run(x1);
        let exhaust: number;
        if (dual) {
          const r0 = this.pipe(0, h0, pop0, leak, mufScale);
          const r1 = this.pipe(1, h1, pop1, leak, mufScale);
          // The far pipe reaches the listener ~1 ms later.
          const back = this.dualDelay[(this.dualW - 52) & 63];
          this.dualDelay[this.dualW & 63] = r1;
          this.dualW = (this.dualW + 1) & 63;
          exhaust = 0.75 * (r0 + 0.92 * back);
        } else {
          exhaust = this.pipe(0, h0 + h1, pop0 + pop1, leak, mufScale);
        }
        exhaust *= exhaustGain;
        for (const ap of this.disp) exhaust = ap.run(exhaust);
        const pulses = x0 + x1;

        // Intake: airbox resonance excited by the pulses, opened by the throttle; hiss.
        const nz = this.rand() * 2 - 1;
        const intake =
          intakeGain * 0.22 * (0.12 + 0.88 * thr) * this.airbox.run(pulses) + noiseScaleI * (0.3 + 0.7 * rf) * this.intakeHiss.run(nz);
        // Mechanical: valvetrain ticks, broadband rattle, gear and diff whine.
        const tk = this.tick;
        this.tick = 0;
        let mech = 0.05 * this.tickBp.run(tk) + mechGain * 0.006 * (0.2 + 0.8 * rf) * this.mechBp.run(this.rand() * 2 - 1);
        if (gearAmp > 0 && moving > 0) {
          this.gearPh += (TAU * (rpm / 60) * 23) / sr;
          if (this.gearPh > TAU) this.gearPh -= TAU;
          mech += whine * 0.012 * gearAmp * moving * (0.3 + 0.7 * load) * Math.sin(this.gearPh);
        }
        if (diffHz > 20) {
          this.diffPh += (TAU * diffHz) / sr;
          if (this.diffPh > TAU) this.diffPh -= TAU;
          mech += whine * 0.006 * clamp(speed / 30, 0, 1) * Math.sin(this.diffPh);
        }
        // Forced induction.
        let fi = 0;
        if (boost > 0.01) {
          this.turboPh += (TAU * this.turboF) / sr;
          if (this.turboPh > TAU) this.turboPh -= TAU;
          fi += boost * boost * (0.3 + 0.7 * thr) * (0.012 * Math.sin(this.turboPh) + 0.05 * this.turboBp.run(nz));
        }
        if (this.blowEnv > 1e-4) {
          fi += blowGain * this.blowEnv * this.blowBp.run(this.rand() * 2 - 1);
          this.blowEnv *= 0.99985;
        }
        if (scOn) {
          this.scPh += (TAU * (rpm / 60) * 15) / sr;
          if (this.scPh > TAU) this.scPh -= TAU;
          fi += 0.01 * clamp(rf, 0, 1) * (0.3 + 0.7 * thr) * (Math.sin(this.scPh) + 0.3 * Math.sin(2 * this.scPh));
        }

        // Perspective.
        let y = 0;
        if (pEx > 0.001) y += pEx * (exhaust + 0.35 * intake + 0.3 * mech + 0.6 * fi);
        if (pIn > 0.001) {
          const body = this.cabinLp.run(exhaust) * 0.75 + this.cabinLp2.run(exhaust) * 0.12;
          y += pIn * (body + 0.9 * this.firewallLp.run(intake) + 1.0 * mech + 0.7 * fi);
        }
        // DC blocker (~15 Hz).
        const dc = y - this.dcX + 0.998 * this.dcY;
        this.dcX = y;
        this.dcY = dc;
        let o = dc * OUT_DRIVE;
        // Soft safety limit: linear to ±0.8, then a continuous tanh knee towards ±1.
        if (o > 0.8) o = 0.8 + 0.2 * Math.tanh((o - 0.8) * 5);
        else if (o < -0.8) o = -0.8 - 0.2 * Math.tanh((-0.8 - o) * 5);
        if (o > -1e-30 && o < 1e-30) o = 0;
        out[off + j] = o * OUT_MAKEUP;
        this.clock++;
      }
      // Keep state finite and free of denormals.
      if (!Number.isFinite(this.dcY) || !Number.isFinite(this.tail[0].lp) || !Number.isFinite(this.tail[1].lp)) this.hardReset(out, off, n);
      for (const b of [this.mufLp, this.mufRes, this.radLp, this.disp]) for (const q of b) q.flush();
      for (const q of [this.airbox, this.intakeHiss, this.tickBp, this.mechBp, this.turboBp, this.blowBp, this.cabinLp, this.cabinLp2, this.firewallLp]) q.flush();
      for (const t of this.header) t.flush();
      for (const t of this.tail) t.flush();
      if (Math.abs(this.dcY) < 1e-25) this.dcY = 0;
    }

    /** Muffler + tailpipe + radiation of one pipe. */
    private pipe(b: number, x: number, pop: number, leak: number, scale: number): number {
      const lp = this.mufLp[b].run(x);
      const m = (this.mufRes[b].run(lp) + leak * (x - lp)) * scale;
      const t = this.tail[b].run(m + pop);
      const rad = t - this.radLeak * this.radPrev[b];
      this.radPrev[b] = t;
      return this.radLp[b].run(rad) * 6;
    }

    private hardReset(out: Float32Array, off: number, n: number) {
      for (let j = 0; j < n; j++) out[off + j] = 0;
      for (const b of [this.mufLp, this.mufRes, this.radLp, this.disp]) for (const q of b) q.reset();
      for (const q of [this.airbox, this.intakeHiss, this.tickBp, this.mechBp, this.turboBp, this.blowBp, this.cabinLp, this.cabinLp2, this.firewallLp]) q.reset();
      for (const t of [...this.header, ...this.tail]) {
        t.buf.fill(0);
        t.lp = 0;
      }
      this.radPrev = [0, 0];
      this.flow = [0, 0];
      this.flow2 = [0, 0];
      this.flowHp = [0, 0];
      this.dcX = this.dcY = 0;
      this.pCount = this.popCount = 0;
    }
  }

  if (scope && scope.registerProcessor && scope.AudioWorkletProcessor) {
    const Base = scope.AudioWorkletProcessor;
    const sr = scope.sampleRate ?? 48000;
    class EngineProcessor extends Base {
      private dsp: (EngineDsp | null)[] = new Array(SLOTS).fill(null);
      private frame = new Float32Array(SLOTS * FIELDS);
      private sinceFrame = 0;
      private perFrame = sr / 60;
      constructor() {
        super();
        this.port.onmessage = (e: MessageEvent) => {
          const m = e.data as { t: string; slot?: number; spec?: Record<string, number>; data?: Float32Array };
          if (m.t === "spec" && m.slot !== undefined && m.spec) {
            this.dsp[m.slot] = new EngineDsp(sr, m.spec);
          } else if (m.t === "off" && m.slot !== undefined) {
            this.dsp[m.slot] = null;
          } else if (m.t === "stats") {
            // Counters per slot (tests): events, misfires, pops, limiter cuts, blow-offs.
            this.port.postMessage({ t: "stats", counters: this.dsp.map((d) => (d ? { ...d.counters, popAt: d.popAt.slice() } : null)) });
          } else if (m.t === "frame" && m.data) {
            // Samples between frames (smoothed): the rpm ramps over one frame interval.
            if (this.sinceFrame > 0) this.perFrame += (clamp(this.sinceFrame, 128, sr * 0.1) - this.perFrame) * 0.3;
            this.sinceFrame = 0;
            this.frame.set(m.data);
            for (let i = 0; i < SLOTS; i++) {
              const d = this.dsp[i];
              if (d && this.frame[i * FIELDS + ACTIVE] > 0.5) d.setFrame(this.frame.subarray(i * FIELDS, (i + 1) * FIELDS), this.perFrame);
            }
          }
        };
      }
      process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
        const n = outputs[0]?.[0]?.length ?? 128;
        this.sinceFrame += n;
        for (let i = 0; i < SLOTS; i++) {
          const d = this.dsp[i];
          const ch = outputs[i]?.[0];
          if (!d || !ch) continue;
          d.render(ch, 0, n);
        }
        return true;
      }
    }
    // Literal: the function is stringified into the worklet (keep equal to ENGINE_PROCESSOR).
    scope.registerProcessor("wp-engine", EngineProcessor);
  }
  return { EngineDsp };
}

/** JavaScript source of the worklet module (for a Blob URL). */
export function engineWorkletSource(): string {
  return `(${engineWorkletMain.toString()})(globalThis);\n`;
}
