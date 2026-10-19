// Web Audio side of the voice model: mixer buses (user gain × duck → master) and a voice pool
// that executes wp_audio's Play / Set / Stop commands. One panner per voice (sources sit on
// their entity); distance attenuation and Doppler are already folded into the parameters by
// the Rust director, so panners only pan (rolloff 0).

import { engineInstrument, makeSynth, type EngineMix, type EngineSynth, type SynthVoice } from "./synth";
import { BUSES, voiceKey, type AudioLibrary, type Bus, type CueDef, type VoiceId, type VoiceParams } from "./types";

interface BusNodes {
  user: GainNode;
  duck: GainNode;
}

/** Mixer buses with per-bus user gain (debug sliders) and a duck stage driven by the director. */
export class AudioMixer {
  readonly master: GainNode;
  readonly buses: Record<Bus, BusNodes>;
  /** Radio EQ input (band-limited, slightly driven) feeding the Police bus. */
  readonly radioIn: AudioNode;

  constructor(readonly ctx: AudioContext) {
    this.master = ctx.createGain();
    this.master.gain.value = 0.8;
    // Gentle limiter so a 24-car pile-up cannot clip.
    const comp = ctx.createDynamicsCompressor();
    comp.threshold.value = -10;
    comp.ratio.value = 6;
    this.master.connect(comp).connect(ctx.destination);
    const buses = {} as Record<Bus, BusNodes>;
    for (const b of BUSES) {
      const user = ctx.createGain();
      const duck = ctx.createGain();
      user.connect(duck).connect(this.master);
      buses[b] = { user, duck };
    }
    this.buses = buses;
    const hp = ctx.createBiquadFilter();
    hp.type = "highpass";
    hp.frequency.value = 300;
    const lp = ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 3400;
    const mid = ctx.createBiquadFilter();
    mid.type = "peaking";
    mid.frequency.value = 1800;
    mid.gain.value = 6;
    const drive = ctx.createWaveShaper();
    const curve = new Float32Array(256);
    for (let i = 0; i < 256; i++) {
      const x = (i / 255) * 2 - 1;
      curve[i] = Math.tanh(3 * x) / Math.tanh(3);
    }
    drive.curve = curve;
    hp.connect(lp).connect(mid).connect(drive).connect(buses.Police.user);
    this.radioIn = hp;
  }

  input(bus: Bus): AudioNode {
    return this.buses[bus].user;
  }

  setUserGain(bus: Bus, v: number): void {
    this.buses[bus].user.gain.setTargetAtTime(v, this.ctx.currentTime, 0.02);
  }

  getUserGain(bus: Bus): number {
    return this.buses[bus].user.gain.value;
  }

  duck(bus: Bus, gain: number, fadeS: number): void {
    this.buses[bus].duck.gain.setTargetAtTime(gain, this.ctx.currentTime, Math.max(fadeS, 0.01) / 3);
  }
}

interface Voice {
  synth: SynthVoice | null;
  src: AudioBufferSourceNode | null;
  gain: GainNode;
  lp: BiquadFilterNode;
  panner: PannerNode;
  /** Audio time after which the voice is disconnected (one-shots and stopped loops). */
  endTime: number;
  /** Parameters last applied (skip automation calls when nothing changed). */
  last: VoiceParams;
}

/** wp_audio slot codes (crates/wp_audio/src/types.rs `slot`): engine layers and limiter. */
const SLOT_ENGINE_LAYER = 0x100;
const SLOT_ENGINE_LIMITER = 0x180;

/** One procedural engine layer (or the limiter) folded into its car's engine instrument. */
interface EngineMember {
  /** Commanded gain (distance and cue gain included). */
  gain: number;
  bright: number;
  off: number;
  noise: number;
  limiter: boolean;
}

/**
 * All procedural engine layers of one entity, played by a single `engineInstrument`.
 * The director still sends one voice per layer (sample packs need that: each recording has its
 * own root rpm); for synth layers the backend sums their energy into one gain and blends their
 * timbre, because separate synths at the same firing frequency interfere.
 */
interface EngineGroup extends Voice {
  synth: EngineSynth;
  members: Map<number, EngineMember>;
  /** Latest params from any member (frequency, load, position, air absorption). */
  pending: VoiceParams;
  mix: EngineMix;
  dirty: boolean;
}

/** Relative change above which a parameter is re-automated. */
const EPS = 0.004;
const changed = (a: number, b: number) => Math.abs(a - b) > EPS * Math.max(Math.abs(a), Math.abs(b), 1e-3);
const moved = (a: number[], b: number[]) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]) > 0.03;

/** Executes voice commands. */
export class VoicePool {
  private voices = new Map<string, Voice>();
  /** Procedural engine instruments by entity. */
  private engines = new Map<number, EngineGroup>();
  /** Released instruments still fading out. */
  private dying: Voice[] = [];
  private samples = new Map<string, AudioBuffer | "loading" | "failed">();
  /** Voices created / released (debug). */
  created = 0;

  constructor(
    private ctx: AudioContext,
    private mixer: AudioMixer,
    private lib: AudioLibrary,
  ) {
    this.preloadSamples();
  }

  /** Playing voices, counting each engine layer folded into an instrument. */
  get size(): number {
    let n = this.voices.size;
    for (const g of this.engines.values()) n += g.members.size;
    return n;
  }

  private cue(pack: string, cue: string): CueDef | undefined {
    return this.lib.packs[pack]?.cues[cue];
  }

  private sampleUrl(pack: string, file: string): string {
    return `data/audio/packs/${pack}/${file}`;
  }

  /** Fetch every recorded file referenced by a pack (none ship yet; synths cover them). */
  private preloadSamples(): void {
    for (const [packId, pack] of Object.entries(this.lib.packs)) {
      for (const c of Object.values(pack.cues)) {
        if (!("Samples" in c.source)) continue;
        for (const f of c.source.Samples.files) {
          const url = this.sampleUrl(packId, f);
          if (this.samples.has(url)) continue;
          this.samples.set(url, "loading");
          fetch(url)
            .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(String(r.status)))))
            .then((b) => this.ctx.decodeAudioData(b))
            .then((buf) => this.samples.set(url, buf))
            .catch(() => this.samples.set(url, "failed"));
        }
      }
    }
  }

  private setPanner(p: PannerNode, pos: [number, number, number], t: number, smooth: boolean): void {
    if (p.positionX) {
      if (smooth) {
        p.positionX.setTargetAtTime(pos[0], t, 0.02);
        p.positionY.setTargetAtTime(pos[1], t, 0.02);
        p.positionZ.setTargetAtTime(pos[2], t, 0.02);
      } else {
        p.positionX.setValueAtTime(pos[0], t);
        p.positionY.setValueAtTime(pos[1], t);
        p.positionZ.setValueAtTime(pos[2], t);
      }
    } else {
      p.setPosition(pos[0], pos[1], pos[2]);
    }
  }

  play(voice: VoiceId, bus: Bus, pack: string, cue: string, looped: boolean, params: VoiceParams, synth: Record<string, number>): void {
    const def = this.cue(pack, cue);
    if (this.isEngineSynth(voice, bus, def)) {
      this.enginePlay(voice, def!, params, synth);
      return;
    }
    const key = voiceKey(voice);
    const old = this.voices.get(key);
    if (old) this.release(old, this.ctx.currentTime, 0.02);
    const t = this.ctx.currentTime;
    const gain = this.ctx.createGain();
    gain.gain.setValueAtTime(looped ? 0 : params.gain, t);
    if (looped) gain.gain.setTargetAtTime(params.gain, t, 0.02);
    const lp = this.ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = Math.min(params.lowpass_hz || 18000, 18000);
    const panner = this.ctx.createPanner();
    panner.panningModel = "equalpower";
    panner.distanceModel = "linear";
    panner.rolloffFactor = 0;
    this.setPanner(panner, params.pos, t, false);
    const dest = this.lib.packs[pack]?.kind === "Radio" ? this.mixer.radioIn : this.mixer.input(bus);
    gain.connect(lp).connect(panner).connect(dest);

    let src: AudioBufferSourceNode | null = null;
    let syn: SynthVoice | null = null;
    const source = def?.source;
    const file = source && "Samples" in source ? source.Samples.files[Math.floor(Math.random() * source.Samples.files.length)] : null;
    const buf = file ? this.samples.get(this.sampleUrl(pack, file)) : undefined;
    if (buf instanceof AudioBuffer) {
      src = this.ctx.createBufferSource();
      src.buffer = buf;
      src.loop = looped;
      src.playbackRate.value = params.pitch;
      src.connect(gain);
      src.start(t);
    } else {
      const name = source && "Synth" in source ? source.Synth.synth : "noise";
      const sp = { ...(source && "Synth" in source ? source.Synth.params : {}), ...synth };
      syn = makeSynth(this.ctx, name, sp, params, t);
      syn.output.connect(gain);
    }
    const dur = def?.duration_s ?? 1;
    this.voices.set(key, { synth: syn, src, gain, lp, panner, endTime: looped ? Infinity : t + dur + 0.5, last: params });
    this.created++;
  }

  set(voice: VoiceId, params: VoiceParams): void {
    const g = this.engines.get(voice.entity);
    const m = g?.members.get(voice.slot);
    if (g && m) {
      m.gain = params.gain;
      g.pending = params;
      g.dirty = true;
      return;
    }
    const v = this.voices.get(voiceKey(voice));
    if (!v) return;
    this.applyParams(v, params);
  }

  /** Re-automate whatever moved since it was last applied. */
  private applyParams(v: Voice, params: VoiceParams): void {
    const t = this.ctx.currentTime;
    // `l` holds the values last *applied*, field by field. Comparing against the previous
    // frame instead would drop every slow change for good: an engine climbing < 0.4 % per
    // frame (2nd gear onwards) froze its pitch and layer gains at the last shift (playtest #19).
    const l = v.last;
    if (changed(params.gain, l.gain)) {
      v.gain.gain.setTargetAtTime(params.gain, t, 0.02);
      l.gain = params.gain;
    }
    if (changed(params.lowpass_hz, l.lowpass_hz)) {
      v.lp.frequency.setTargetAtTime(Math.min(params.lowpass_hz || 18000, 18000), t, 0.05);
      l.lowpass_hz = params.lowpass_hz;
    }
    if (moved(params.pos, l.pos)) {
      this.setPanner(v.panner, params.pos, t, true);
      l.pos = params.pos;
    }
    const pitchMoved = changed(params.pitch, l.pitch);
    if (v.src && pitchMoved) v.src.playbackRate.setTargetAtTime(params.pitch, t, 0.02);
    // Sirens follow their own clock and must see every phase; others only on change.
    const synthChanged =
      changed(params.freq_hz, l.freq_hz) || pitchMoved || changed(params.load, l.load) || params.phase !== l.phase;
    if (v.synth && synthChanged) v.synth.update(params, t);
    if (synthChanged) {
      l.freq_hz = params.freq_hz;
      l.pitch = params.pitch;
      l.load = params.load;
      l.phase = params.phase;
    }
    l.doppler = params.doppler;
  }

  private isEngineSynth(voice: VoiceId, bus: Bus, def: CueDef | undefined): boolean {
    return (
      bus === "Engine" &&
      voice.slot >= SLOT_ENGINE_LAYER &&
      voice.slot <= SLOT_ENGINE_LIMITER &&
      !!def &&
      "Synth" in def.source &&
      def.source.Synth.synth === "engine"
    );
  }

  private enginePlay(voice: VoiceId, def: CueDef, params: VoiceParams, synth: Record<string, number>): void {
    const sp = { ...("Synth" in def.source ? def.source.Synth.params : {}), ...synth };
    const member: EngineMember = {
      gain: params.gain,
      bright: sp.bright ?? 0.5,
      off: sp.off ? 1 : 0,
      noise: sp.noise ?? 0.1,
      limiter: !!sp.chop_hz,
    };
    let g = this.engines.get(voice.entity);
    if (!g) {
      const t = this.ctx.currentTime;
      const mix = { bright: member.bright, off: member.off, noise: member.noise, chop: member.limiter ? 1 : 0 };
      const syn = engineInstrument(this.ctx, sp, params, mix);
      const gain = this.ctx.createGain();
      gain.gain.setValueAtTime(0, t);
      const lp = this.ctx.createBiquadFilter();
      lp.type = "lowpass";
      lp.frequency.value = Math.min(params.lowpass_hz || 18000, 18000);
      const panner = this.ctx.createPanner();
      panner.panningModel = "equalpower";
      panner.distanceModel = "linear";
      panner.rolloffFactor = 0;
      this.setPanner(panner, params.pos, t, false);
      syn.output.connect(gain).connect(lp).connect(panner).connect(this.mixer.input("Engine"));
      g = {
        synth: syn,
        src: null,
        gain,
        lp,
        panner,
        endTime: Infinity,
        // Gain starts at 0 and is ramped in by the first flush.
        last: { ...params, gain: 0, pos: [...params.pos] as [number, number, number] },
        members: new Map(),
        pending: params,
        mix,
        dirty: true,
      };
      this.engines.set(voice.entity, g);
      this.created++;
    }
    g.members.set(voice.slot, member);
    g.pending = params;
    g.dirty = true;
  }

  /** Fold the members' gains and timbres into the instrument (once per frame). */
  private flushEngine(g: EngineGroup): void {
    let e = 0;
    let bright = 0;
    let off = 0;
    let noise = 0;
    let chop = 0;
    for (const m of g.members.values()) {
      const w = m.gain * m.gain;
      e += w;
      bright += w * m.bright;
      off += w * m.off;
      noise += w * m.noise;
      if (m.limiter) chop += w;
    }
    if (e > 1e-12) {
      const mix = { bright: bright / e, off: off / e, noise: noise / e, chop: chop / e };
      const c = g.mix;
      if (
        Math.abs(mix.bright - c.bright) > 0.004 ||
        Math.abs(mix.off - c.off) > 0.004 ||
        Math.abs(mix.noise - c.noise) > 0.001 ||
        Math.abs(mix.chop - c.chop) > 0.004
      ) {
        g.synth.setMix(mix, this.ctx.currentTime);
        g.mix = mix;
      }
    }
    this.applyParams(g, { ...g.pending, gain: Math.sqrt(e) });
    g.dirty = false;
  }

  stop(voice: VoiceId, fadeS: number): void {
    const g = this.engines.get(voice.entity);
    if (g && g.members.delete(voice.slot)) {
      if (g.members.size === 0) {
        this.engines.delete(voice.entity);
        this.release(g, this.ctx.currentTime, fadeS);
        this.dying.push(g);
      } else {
        g.dirty = true;
      }
      return;
    }
    const key = voiceKey(voice);
    const v = this.voices.get(key);
    if (!v) return;
    this.release(v, this.ctx.currentTime, fadeS);
  }

  private release(v: Voice, t: number, fadeS: number): void {
    v.gain.gain.cancelScheduledValues(t);
    v.gain.gain.setTargetAtTime(0, t, Math.max(fadeS, 0.005) / 3);
    const end = t + fadeS + 0.05;
    v.synth?.stop(end);
    try {
      v.src?.stop(end);
    } catch {
      // not started
    }
    v.endTime = Math.min(v.endTime, end + 0.05);
  }

  /** Disconnect finished voices. Call once per frame. */
  collect(): void {
    for (const g of this.engines.values()) if (g.dirty) this.flushEngine(g);
    const t = this.ctx.currentTime;
    for (const [k, v] of this.voices) {
      if (v.endTime <= t) {
        v.panner.disconnect();
        this.voices.delete(k);
      }
    }
    this.dying = this.dying.filter((v) => {
      if (v.endTime > t) return true;
      v.panner.disconnect();
      return false;
    });
  }

  stopAll(): void {
    const t = this.ctx.currentTime;
    for (const v of this.voices.values()) this.release(v, t, 0.05);
    for (const g of this.engines.values()) {
      this.release(g, t, 0.05);
      this.dying.push(g);
    }
    this.engines.clear();
  }

  /** Play a cue outside the director (scanner squelch / static). Returns a stop function. */
  playLocal(pack: string, cue: string, gain: number): () => void {
    const id: VoiceId = { entity: 0xfffffffe, slot: Math.floor(Math.random() * 0x7fffffff) };
    const def = this.cue(pack, cue);
    const params: VoiceParams = { gain, pitch: 1, freq_hz: 0, load: 0, phase: 0, lowpass_hz: 18000, doppler: 1, pos: [0, 0, 0] };
    this.play(id, "Police", pack, cue, def?.looped ?? false, params, {});
    return () => this.stop(id, 0.15);
  }
}

/** Point the Web Audio listener at the camera (sim world space). */
export function setListener(ctx: AudioContext, pos: number[], fwd: number[], up: number[]): void {
  const l = ctx.listener;
  const t = ctx.currentTime;
  if (l.positionX) {
    l.positionX.setTargetAtTime(pos[0], t, 0.01);
    l.positionY.setTargetAtTime(pos[1], t, 0.01);
    l.positionZ.setTargetAtTime(pos[2], t, 0.01);
    l.forwardX.setTargetAtTime(fwd[0], t, 0.01);
    l.forwardY.setTargetAtTime(fwd[1], t, 0.01);
    l.forwardZ.setTargetAtTime(fwd[2], t, 0.01);
    l.upX.setTargetAtTime(up[0], t, 0.01);
    l.upY.setTargetAtTime(up[1], t, 0.01);
    l.upZ.setTargetAtTime(up[2], t, 0.01);
  } else {
    l.setPosition(pos[0], pos[1], pos[2]);
    l.setOrientation(fwd[0], fwd[1], fwd[2], up[0], up[1], up[2]);
  }
}
