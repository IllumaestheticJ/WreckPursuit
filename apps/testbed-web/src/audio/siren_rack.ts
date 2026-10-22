// Siren v2 mixing (playtest row 82): every siren voice (siren packs) is HRTF-panned into the
// siren sub-bus instead of straight into the Police bus. The sub-bus carries the shared
// acoustics computed by wp_audio::siren_space each frame:
//
//   sirens ─► in ─┬──────────────────────────────► duck ─► Police bus (settings volume)
//                 ├─► slap L / R delays (cross-fed: street flutter) ─┘
//                 └─► tunnel convolver ──────────────────────────────┘
//
// plus per-voice occlusion / interior muffle (folded into each voice's gain and low-pass), a
// duck while a radio line plays, and the engine-bus duck under a loud siren.

import type { AudioMixer } from "./voices";
import type { AudioLibrary, SirenRoom, SirenSpaceFrame, VoiceId, VoiceParams } from "./types";

/** Deterministic noise for the tunnel impulse response. */
function lcg(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

/** Stereo exponentially decaying noise (RT60 `rt60` s) with a sparse early part. */
function tunnelIr(ctx: BaseAudioContext, rt60: number): AudioBuffer {
  const len = Math.ceil(ctx.sampleRate * Math.min(4, Math.max(0.3, rt60)));
  const buf = ctx.createBuffer(2, len, ctx.sampleRate);
  for (let ch = 0; ch < 2; ch++) {
    const r = lcg(77 + ch);
    const d = buf.getChannelData(ch);
    for (let i = 0; i < len; i++) {
      const t = i / ctx.sampleRate;
      // -60 dB at rt60: exp(-6.91 t / rt60). Early reflections are sparse spikes.
      const env = Math.exp((-6.91 * t) / rt60);
      const early = t < 0.06 ? (r() < 0.004 ? 1 : 0) * (r() * 2 - 1) * 2 : 0;
      d[i] = ((r() * 2 - 1) * (t < 0.01 ? t / 0.01 : 1) + early) * env;
    }
  }
  return buf;
}

/** wp_audio slot codes: siren loops `SIREN + pattern` (0x300..0x303). */
const isLoopSlot = (slot: number) => slot >= 0x300 && slot < 0x304;
const voiceKey = (v: VoiceId) => `${v.entity}:${v.slot}`;

/** Per-entity occlusion / interior from the last space frame. */
interface VoiceFx {
  gain: number;
  lowpass: number;
  pattern: string;
}

export class SirenRack {
  /** Siren voices connect here (see `VoicePool.router`). */
  readonly input: GainNode;
  private readonly duck: GainNode;
  private readonly slap: { delay: DelayNode; gain: GainNode; fb: GainNode }[];
  private readonly tunnelSend: GainNode;
  private readonly convolver: ConvolverNode;
  private rt60 = 0;
  private fx = new Map<number, VoiceFx>();
  /** Voices playing from a siren pack (only these get the per-entity siren acoustics). */
  private voices = new Set<string>();
  private radio = false;
  private engineDuck = 1;
  private radioDuck = 0.5;
  /** Lab override of the room (null = the game's). */
  labRoom: SirenRoom | null = null;
  /** Last space frame (debug panel, e2e). */
  last: SirenSpaceFrame | null = null;
  /** Patterns heard per entity since start (e2e: mode changes are heard). */
  readonly heard = new Map<number, Set<string>>();
  /** Smoothed cost of `frame()`, ms. */
  frameMs = 0;

  constructor(
    private ctx: AudioContext,
    private mixer: AudioMixer,
    private lib: AudioLibrary,
  ) {
    this.input = ctx.createGain();
    this.duck = ctx.createGain();
    this.input.connect(this.duck).connect(mixer.input("Police"));
    // Two facade echoes, cross-fed so sound bounces across the street (flutter).
    this.slap = [-0.75, 0.75].map((pan) => {
      const delay = ctx.createDelay(0.5);
      delay.delayTime.value = 0.03;
      const lp = ctx.createBiquadFilter();
      lp.type = "lowpass";
      lp.frequency.value = 3200;
      const gain = ctx.createGain();
      gain.gain.value = 0;
      const fb = ctx.createGain();
      fb.gain.value = 0;
      const pn = ctx.createStereoPanner();
      pn.pan.value = pan;
      this.input.connect(delay);
      delay.connect(lp);
      lp.connect(gain).connect(pn).connect(this.duck);
      lp.connect(fb);
      return { delay, gain, fb };
    });
    this.slap[0].fb.connect(this.slap[1].delay);
    this.slap[1].fb.connect(this.slap[0].delay);
    this.tunnelSend = ctx.createGain();
    this.tunnelSend.gain.value = 0;
    this.convolver = ctx.createConvolver();
    this.input.connect(this.tunnelSend).connect(this.convolver).connect(this.duck);
    this.setRt60(1.8);
  }

  private setRt60(rt60: number): void {
    if (Math.abs(rt60 - this.rt60) < 0.05) return;
    this.rt60 = rt60;
    this.convolver.buffer = tunnelIr(this.ctx, rt60);
  }

  /** `VoicePool.router`: siren packs go through the rack, HRTF-panned. */
  dest(pack: string): AudioNode | null {
    return this.lib.packs[pack]?.kind === "Siren" ? this.input : null;
  }

  panning(pack: string): PanningModelType {
    return this.lib.packs[pack]?.kind === "Siren" ? "HRTF" : "equalpower";
  }

  /** A radio line started / ended (sirens duck under the voice). */
  setRadio(on: boolean): void {
    this.radio = on;
    this.applyDuck();
  }

  private applyDuck(): void {
    this.duck.gain.setTargetAtTime(this.radio ? this.radioDuck : 1, this.ctx.currentTime, 0.08);
  }

  /** Apply this frame's siren acoustics (call before the frame's commands). */
  frame(space: SirenSpaceFrame | null | undefined): void {
    const t0 = performance.now();
    const t = this.ctx.currentTime;
    if (space) {
      this.last = space;
      this.fx.clear();
      for (const v of space.voices) {
        this.fx.set(v.entity, { gain: v.gain, lowpass: v.lowpass_hz, pattern: v.pattern });
        let h = this.heard.get(v.entity);
        if (!h) this.heard.set(v.entity, (h = new Set()));
        h.add(v.pattern);
      }
      if (Math.abs(space.engine_duck - this.engineDuck) > 0.01) {
        this.engineDuck = space.engine_duck;
        this.mixer.duck("Engine", space.engine_duck, 0.3);
      }
      if (space.radio_duck !== this.radioDuck) {
        this.radioDuck = space.radio_duck;
        this.applyDuck();
      }
    }
    const room = this.labRoom ?? space?.room;
    if (room) this.applyRoom(room, t);
    this.frameMs += (performance.now() - t0 - this.frameMs) * 0.1;
  }

  private applyRoom(room: SirenRoom, t: number): void {
    const sides: [number, number][] = [
      [room.slap_left_s, room.slap_gain[0]],
      [room.slap_right_s, room.slap_gain[1]],
    ];
    sides.forEach(([delay, gain], i) => {
      const s = this.slap[i];
      if (delay > 0) s.delay.delayTime.setTargetAtTime(Math.min(0.45, delay), t, 0.1);
      s.gain.gain.setTargetAtTime(delay > 0 ? gain : 0, t, 0.1);
      s.fb.gain.setTargetAtTime(Math.min(0.6, room.slap_feedback), t, 0.1);
    });
    this.tunnelSend.gain.setTargetAtTime(room.tunnel, t, 0.15);
    if (room.rt60_s > 0) this.setRt60(room.rt60_s);
  }

  /** A voice starts (`Play`): remember it if it plays from a siren pack. */
  noteVoice(voice: VoiceId, pack: string): void {
    if (this.lib.packs[pack]?.kind === "Siren") this.voices.add(voiceKey(voice));
    else this.voices.delete(voiceKey(voice));
  }

  /** A voice stops. */
  forget(voice: VoiceId): void {
    this.voices.delete(voiceKey(voice));
  }

  /** A siren voice's parameters with occlusion / interior muffle folded in (others untouched). */
  adjust(voice: VoiceId, params: VoiceParams): VoiceParams {
    if (!this.voices.has(voiceKey(voice))) return params;
    const fx = this.fx.get(voice.entity);
    if (!fx) return params;
    return { ...params, gain: params.gain * fx.gain, lowpass_hz: Math.min(params.lowpass_hz || 18000, fx.lowpass) };
  }

  /** Whether `voice` is a siren loop of an entity the director reports. */
  isSirenLoop(voice: VoiceId): boolean {
    return isLoopSlot(voice.slot) && this.fx.has(voice.entity);
  }

  /** Debug / e2e read-out. */
  stats(): { playing: number; patterns: Record<number, string[]>; room: SirenRoom | null; engineDuck: number; frameMs: number } {
    const patterns: Record<number, string[]> = {};
    for (const [e, s] of this.heard) patterns[e] = [...s];
    return {
      playing: this.last?.voices.length ?? 0,
      patterns,
      room: this.labRoom ?? this.last?.room ?? null,
      engineDuck: this.engineDuck,
      frameMs: this.frameMs,
    };
  }
}
