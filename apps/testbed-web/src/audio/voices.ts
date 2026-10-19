// Web Audio side of the voice model: mixer buses (user gain × duck → master) and a voice pool
// that executes wp_audio's Play / Set / Stop commands. One panner per voice (sources sit on
// their entity); distance attenuation and Doppler are already folded into the parameters by
// the Rust director, so panners only pan (rolloff 0).

import { makeSynth, type SynthVoice } from "./synth";
import {
  BUSES,
  voiceKey,
  type AudioLibrary,
  type Bus,
  type CueDef,
  type SampleStatus,
  type SoundSource,
  type VoiceId,
  type VoiceParams,
} from "./types";

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
  /** Null for local (non-spatial) voices: scanner sounds, ambience. */
  panner: PannerNode | null;
  /** Audio time after which the voice is disconnected (one-shots and stopped loops). */
  endTime: number;
  /** Parameters last applied (skip automation calls when nothing changed). */
  last: VoiceParams;
}

/** Entity id of voices played outside the director (non-spatial). */
const LOCAL_ENTITY = 0xfffffffe;

/** Relative change above which a parameter is re-automated. */
const EPS = 0.004;
const changed = (a: number, b: number) => Math.abs(a - b) > EPS * Math.max(Math.abs(a), Math.abs(b), 1e-3);
const moved = (a: number[], b: number[]) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]) > 0.03;

/** Executes voice commands. */
export class VoicePool {
  private voices = new Map<string, Voice>();
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

  get size(): number {
    return this.voices.size;
  }

  /** Recorded samples (default) or each pack's procedural `fallback`. */
  source: SoundSource = "recorded";

  private cue(pack: string, cue: string): CueDef | undefined {
    return this.lib.packs[pack]?.cues[cue];
  }

  /**
   * The cue to perform for `pack`/`cue`: in procedural mode (or while a recorded file is not
   * decoded yet / failed to load) the pack's `fallback` cue of the same name, if it has one.
   */
  private resolve(pack: string, cue: string): { def: CueDef | undefined; buf: AudioBuffer | null; pack: string } {
    const def = this.cue(pack, cue);
    const fb = this.lib.packs[pack]?.fallback;
    const fbDef = fb ? this.cue(fb, cue) : undefined;
    if (this.source === "procedural" && fbDef) return { def: fbDef, buf: null, pack: fb! };
    const src = def?.source;
    if (src && "Samples" in src) {
      const files = src.Samples.files;
      const file = files[Math.floor(Math.random() * files.length)];
      const buf = this.samples.get(this.sampleUrl(pack, file));
      if (buf instanceof AudioBuffer) return { def, buf, pack };
      if (fbDef) return { def: fbDef, buf: null, pack: fb! };
    }
    return { def, buf: null, pack };
  }

  /** Whether `pack` defines `cue` (optional cues such as the city ambience). */
  has(pack: string, cue: string): boolean {
    return !!this.cue(pack, cue);
  }

  /** Relative URL (works on the dev server and the published link): `data/audio/packs/<pack>/<file>`. */
  private sampleUrl(pack: string, file: string): string {
    const parts: string[] = [];
    for (const seg of `${pack}/${file}`.split("/")) {
      if (seg === "..") parts.pop();
      else if (seg && seg !== ".") parts.push(seg);
    }
    return `data/audio/packs/${parts.join("/")}`;
  }

  /** Fetch and decode every recorded file referenced by a pack (shared files load once). */
  private preloadSamples(): void {
    for (const [packId, pack] of Object.entries(this.lib.packs)) {
      for (const c of Object.values(pack.cues)) {
        if (!("Samples" in c.source)) continue;
        for (const f of c.source.Samples.files) {
          const url = this.sampleUrl(packId, f);
          if (this.samples.has(url)) continue;
          this.samples.set(url, "loading");
          fetch(url)
            .then((r) => (r.ok ? r.arrayBuffer() : Promise.reject(new Error(`HTTP ${r.status}`))))
            .then((b) => this.ctx.decodeAudioData(b))
            .then((buf) => this.samples.set(url, buf))
            .catch((e) => {
              console.warn("audio sample failed:", url, e);
              this.samples.set(url, "failed");
            });
        }
      }
    }
  }

  /** Loading progress of the recorded samples. */
  sampleStatus(): SampleStatus {
    const st: SampleStatus = { total: this.samples.size, loaded: 0, loading: 0, failed: [] };
    for (const [url, v] of this.samples) {
      if (v instanceof AudioBuffer) st.loaded++;
      else if (v === "loading") st.loading++;
      else st.failed.push(url);
    }
    return st;
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
    const key = voiceKey(voice);
    const old = this.voices.get(key);
    if (old) this.release(old, this.ctx.currentTime, 0.02);
    const t = this.ctx.currentTime;
    const { def, buf, pack: played } = this.resolve(pack, cue);
    const gain = this.ctx.createGain();
    gain.gain.setValueAtTime(looped ? 0 : params.gain, t);
    if (looped) gain.gain.setTargetAtTime(params.gain, t, 0.02);
    const lp = this.ctx.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = Math.min(params.lowpass_hz || 18000, 18000);
    const dest = this.lib.packs[played]?.kind === "Radio" ? this.mixer.radioIn : this.mixer.input(bus);
    let panner: PannerNode | null = null;
    if (voice.entity === LOCAL_ENTITY) {
      gain.connect(lp).connect(dest);
    } else {
      panner = this.ctx.createPanner();
      panner.panningModel = "equalpower";
      panner.distanceModel = "linear";
      panner.rolloffFactor = 0;
      this.setPanner(panner, params.pos, t, false);
      gain.connect(lp).connect(panner).connect(dest);
    }

    let src: AudioBufferSourceNode | null = null;
    let syn: SynthVoice | null = null;
    const source = def?.source;
    if (buf) {
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
    // A recorded one-shot can outlast the director's estimate: keep the voice until it ends.
    const dur = Math.max(def?.duration_s ?? 1, buf && !looped ? buf.duration / Math.max(params.pitch, 0.25) : 0);
    this.voices.set(key, { synth: syn, src, gain, lp, panner, endTime: looped ? Infinity : t + dur + 0.5, last: params });
    this.created++;
  }

  set(voice: VoiceId, params: VoiceParams): void {
    const v = this.voices.get(voiceKey(voice));
    if (!v) return;
    const t = this.ctx.currentTime;
    const l = v.last;
    if (changed(params.gain, l.gain)) v.gain.gain.setTargetAtTime(params.gain, t, 0.02);
    if (changed(params.lowpass_hz, l.lowpass_hz)) {
      v.lp.frequency.setTargetAtTime(Math.min(params.lowpass_hz || 18000, 18000), t, 0.05);
    }
    if (v.panner && moved(params.pos, l.pos)) this.setPanner(v.panner, params.pos, t, true);
    if (v.src && changed(params.pitch, l.pitch)) v.src.playbackRate.setTargetAtTime(params.pitch, t, 0.02);
    // Sirens follow their own clock and must see every phase; others only on change.
    const synthChanged =
      changed(params.freq_hz, l.freq_hz) || changed(params.pitch, l.pitch) || changed(params.load, l.load) || params.phase !== l.phase;
    if (v.synth && synthChanged) v.synth.update(params, t);
    v.last = params;
  }

  stop(voice: VoiceId, fadeS: number): void {
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
    const t = this.ctx.currentTime;
    for (const [k, v] of this.voices) {
      if (v.endTime <= t) {
        (v.panner ?? v.lp).disconnect();
        this.voices.delete(k);
      }
    }
  }

  stopAll(): void {
    const t = this.ctx.currentTime;
    for (const v of this.voices.values()) this.release(v, t, 0.05);
  }

  /**
   * Play a cue outside the director (scanner squelch / static, city ambience), non-spatial at
   * the listener. Returns a stop function.
   */
  playLocal(pack: string, cue: string, gain: number, bus: Bus = "Police"): () => void {
    const id: VoiceId = { entity: LOCAL_ENTITY, slot: Math.floor(Math.random() * 0x7fffffff) };
    const def = this.cue(pack, cue);
    const params: VoiceParams = { gain, pitch: 1, freq_hz: 0, load: 0, phase: 0, lowpass_hz: 18000, doppler: 1, pos: [0, 0, 0] };
    this.play(id, bus, pack, cue, def?.looped ?? false, params, {});
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
