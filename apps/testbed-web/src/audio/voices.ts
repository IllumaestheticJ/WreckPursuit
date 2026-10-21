// Web Audio side of the voice model: mixer buses (user gain × duck → master) and a voice pool
// that executes wp_audio's Play / Set / Stop commands. One panner per voice (sources sit on
// their entity); distance attenuation and Doppler are already folded into the parameters by
// the Rust director, so panners only pan (rolloff 0).

import type { EngineSynthHost } from "./engine_host";
import { engineInstrument, makeSynth, type EngineMix, type EngineSynth, type SynthVoice } from "./synth";
// Owner row 74: modelled impacts, crunch, friction scrapes and horns (docs/integration/audio-impacts.md).
import { ImpactSynths, type ClockedSynth } from "./impacts";
import {
  BUSES,
  voiceKey,
  type AudioLibrary,
  type Bus,
  type CueDef,
  type MasterMix,
  type SampleStatus,
  type SoundSource,
  type VoiceId,
  type VoiceParams,
} from "./types";

interface BusNodes {
  user: GainNode;
  /** Impact sidechain: a big crash briefly ducks the other buses (owner row 74). */
  side: GainNode;
  duck: GainNode;
  /** Fixed headroom trim (`MasterMix::bus_gain_db`); its output feeds the master. */
  trim: GainNode;
}

/** Buses an impact's sidechain ducks (the impact itself plays on World). */
const SIDECHAIN_BUSES: Bus[] = ["Engine", "Police", "Music"];

/**
 * Shipped master stage (mirror of `data/audio/audio_config.json` → `master`; used when a caller
 * has no library at hand; e2e/audio_mix.mjs fails if the two drift apart).
 */
export const DEFAULT_MASTER: MasterMix = {
  bus_gain_db: { Engine: -6, World: -6, Police: -6, Ui: -6, Music: -6 },
  compressor: { threshold_db: -8, knee_db: 6, ratio: 4, attack_s: 0.003, release_s: 0.3 },
  trim_db: 0,
  clip_knee: 0.7,
  clip_ceiling: 0.89,
};

const dbToLin = (db: number) => 10 ** (db / 20);
const linToDb = (x: number) => 20 * Math.log10(x);

/**
 * The automatic make-up gain a DynamicsCompressorNode applies with these settings (linear).
 * Web Audio leaves it unspecified and it cannot be switched off; Chromium, Firefox and Safari
 * all run the same compressor kernel (WebKit's DynamicsCompressorKernel), which scales its
 * output by `(1 / curve(0 dBFS)) ^ 0.6`, where `curve` is the static compression curve: a
 * soft knee `thr + (1 − e^(−k·(x − thr))) / k` from the threshold to threshold + knee (k chosen
 * so the knee ends at slope 1 / ratio), then a constant ratio. Ported from that kernel
 * (`kneeCurve`, `saturate`, `kAtSlope`, `updateStaticCurveParameters`), float for float.
 * A gentle `threshold −10 dB / knee 30 dB` gives +0.34 dB, a hard `−24 / 0 / 12` gives +12 dB:
 * every signal below threshold got that much louder, and pushed the next stage harder.
 */
export function compressorMakeupGain(thresholdDb: number, kneeDb: number, ratio: number): number {
  const linThr = dbToLin(thresholdDb);
  const knee = (x: number, k: number) => (x < linThr ? x : linThr + (1 - Math.exp(-k * (x - linThr))) / k);
  const slopeAt = (x: number, k: number) => {
    if (x < linThr) return 1;
    const x2 = x * 1.001;
    return (linToDb(knee(x2, k)) - linToDb(knee(x, k))) / (linToDb(x2) - linToDb(x));
  };
  // kAtSlope: bisect k (geometric mean) so the knee's slope at its end is 1 / ratio.
  const xKnee = dbToLin(thresholdDb + kneeDb);
  let minK = 0.1;
  let maxK = 10000;
  let k = 5;
  for (let i = 0; i < 15; i++) {
    if (slopeAt(xKnee, k) < 1 / ratio) maxK = k;
    else minK = k;
    k = Math.sqrt(minK * maxK);
  }
  const kneeThrDb = thresholdDb + kneeDb;
  const yKneeThrDb = linToDb(knee(dbToLin(kneeThrDb), k));
  // saturate(1, k): the curve's output for a full-scale input.
  const full = 1 < dbToLin(kneeThrDb) ? knee(1, k) : dbToLin(yKneeThrDb + (0 - kneeThrDb) / ratio);
  return (1 / full) ** 0.6;
}

/**
 * Soft clip transfer curve for a WaveShaper fed through a 0.5 pre-gain (curve input ±1 = signal
 * ±2): identity below `knee`, then a tanh knee that approaches `ceiling` and never reaches it.
 */
export function softClipCurve(knee: number, ceiling: number, n = 4096): Float32Array<ArrayBuffer> {
  const c = new Float32Array(n);
  const w = Math.max(ceiling - knee, 1e-3);
  for (let i = 0; i < n; i++) {
    const x = ((i / (n - 1)) * 2 - 1) * 2;
    const a = Math.abs(x);
    c[i] = a < knee ? x : Math.sign(x) * (knee + w * Math.tanh((a - knee) / w));
  }
  return c;
}

/**
 * Mixer buses with per-bus user gain (debug sliders / game menu), a duck stage driven by the
 * director and a fixed headroom trim, summed into the master stage.
 *
 * Gain staging (docs/integration/audio.md "Mix headroom"): sources are calibrated against each
 * other (engine pack levels, cue gains); the bus trims give the sum its headroom, so a busy
 * chase peaks a few dB under full scale with the user's master at its default 1.0. The master
 * compressor only catches peaks (crash transients, several sirens at once) and its automatic
 * make-up gain is divided out, so below threshold the master stage is unity gain. The tanh soft
 * clip after it is a safety net that keeps sample peaks under `clip_ceiling` (−1 dBFS).
 */
export class AudioMixer {
  readonly master: GainNode;
  readonly buses: Record<Bus, BusNodes>;
  /** Master compressor (e2e reads its `reduction`). */
  readonly comp: DynamicsCompressorNode;
  /** Radio EQ input (band-limited, slightly driven) feeding the Police bus. */
  readonly radioIn: AudioNode;
  /** Last node before the destination (offline renders tap it). */
  readonly out: AudioNode;
  /** Master stage settings in use. */
  readonly settings: MasterMix;

  constructor(
    readonly ctx: AudioContext,
    settings?: MasterMix,
  ) {
    const m = (this.settings = settings ?? DEFAULT_MASTER);
    this.master = ctx.createGain();
    this.master.gain.value = 1.0;
    const comp = (this.comp = ctx.createDynamicsCompressor());
    comp.threshold.value = m.compressor.threshold_db;
    comp.knee.value = m.compressor.knee_db;
    comp.ratio.value = m.compressor.ratio;
    comp.attack.value = m.compressor.attack_s;
    comp.release.value = m.compressor.release_s;
    // Undo the compressor's automatic make-up gain (read back the clamped AudioParam values),
    // then the final trim.
    const post = ctx.createGain();
    post.gain.value = dbToLin(m.trim_db) / compressorMakeupGain(comp.threshold.value, comp.knee.value, comp.ratio.value);
    // Safety soft clip (owner row 74): a crash transient can still overshoot the compressor's
    // attack; tanh keeps it under the ceiling instead of hard clipping. Input range ±2 via the
    // 0.5 pre-gain.
    const pre = ctx.createGain();
    pre.gain.value = 0.5;
    const clip = ctx.createWaveShaper();
    clip.curve = softClipCurve(m.clip_knee, m.clip_ceiling);
    this.master.connect(comp).connect(post).connect(pre).connect(clip).connect(ctx.destination);
    this.out = clip;
    const buses = {} as Record<Bus, BusNodes>;
    for (const b of BUSES) {
      const user = ctx.createGain();
      const side = ctx.createGain();
      const duck = ctx.createGain();
      const trim = ctx.createGain();
      trim.gain.value = dbToLin(m.bus_gain_db[b] ?? 0);
      user.connect(side).connect(duck).connect(trim).connect(this.master);
      buses[b] = { user, side, duck, trim };
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

  /** A bus's output where it enters the master stage (offline renders tap it). */
  busOut(bus: Bus): AudioNode {
    return this.buses[bus].trim;
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

  /** Audio time until which the current sidechain duck holds, and its depth. */
  private sideUntil = 0;
  private sideDepth = 0;

  /**
   * Impact sidechain: duck the other buses by `depth` (0..1) within ~5 ms, hold `holdS`, recover
   * over ~0.3 s. A weaker hit during a deeper duck changes nothing.
   */
  sidechain(depth: number, holdS: number, t: number): void {
    if (t < this.sideUntil && depth <= this.sideDepth) return;
    this.sideDepth = depth;
    this.sideUntil = t + holdS;
    for (const b of SIDECHAIN_BUSES) {
      const g = this.buses[b].side.gain;
      g.cancelScheduledValues(t);
      g.setTargetAtTime(1 - depth, t, 0.002);
      g.setTargetAtTime(1, t + holdS, 0.1);
    }
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
  /** Commanded gain → played gain (a stand-in cue's own gain; see `resolve`). */
  scale: number;
  /** Commanded pitch → playback rate of a recording (a siren unit's own rate; see `sirenLoopVariation`). */
  rate: number;
}

/**
 * How one unit plays a recorded siren loop. Real units never run in lockstep: each one's
 * oscillator runs a little fast or slow and it switches on somewhere in its cycle. A synth
 * siren gets that from the director's per-unit spread (its cadence parameters are already
 * scaled); a recording gets the same spread as a playback-rate factor (`synth.lo_hz` over the
 * configured cadence's `lo_hz`) and a start point in the loop from the unit seed
 * (`synth.unit`). Neutral (rate 1, offset 0) for anything else. Presentation only.
 */
export function sirenLoopVariation(
  lib: AudioLibrary,
  pack: string,
  cue: string,
  synth: Record<string, number>,
): { rate: number; offset: number } {
  const neutral = { rate: 1, offset: 0 };
  if (lib.packs[pack]?.kind !== "Siren" || synth.unit === undefined) return neutral;
  const name = { "siren/wail": "Wail", "siren/yelp": "Yelp", "siren/phaser": "Phaser", "siren/hilo": "HiLo" }[cue];
  const cfg = lib.config as { sirens?: { cadence?: Record<string, { lo_hz?: number }> } };
  const lo = name ? cfg.sirens?.cadence?.[name]?.lo_hz : undefined;
  const r = lo && synth.lo_hz ? synth.lo_hz / lo : 1;
  // Golden-ratio sequence over the unit seed: neighbouring ids start far apart in the loop.
  const offset = (synth.unit * 0.6180339887) % 1;
  return { rate: Number.isFinite(r) ? Math.min(1.1, Math.max(0.9, r)) : 1, offset };
}

/**
 * Exact length of an Ogg Vorbis file: the end-of-stream page's granule position (samples) and
 * the identification header's sample rate; null for anything else. Chromium's decodeAudioData
 * ignores this end trim and returns the whole last Vorbis block, up to ~20 ms of decaying codec
 * tail past the real end, which a looped voice played as a dropout at every wrap.
 */
export function oggVorbisLength(bytes: ArrayBuffer): { samples: number; rate: number } | null {
  const u = new Uint8Array(bytes);
  const isPage = (i: number) => u[i] === 0x4f && u[i + 1] === 0x67 && u[i + 2] === 0x67 && u[i + 3] === 0x53 && u[i + 4] === 0;
  if (u.length < 64 || !isPage(0)) return null;
  const dv = new DataView(bytes);
  const p = 27 + u[26]; // first packet: after the header and its segment table
  if (p + 16 > u.length || u[p] !== 1 || String.fromCharCode(...u.subarray(p + 1, p + 7)) !== "vorbis") return null;
  const rate = dv.getUint32(p + 12, true);
  // The last page (end-of-stream flag) holds the final granule position.
  for (let i = u.length - 27; i > 0; i--) {
    if (!isPage(i) || !(u[i + 5] & 0x04)) continue;
    const samples = dv.getUint32(i + 10, true) * 2 ** 32 + dv.getUint32(i + 6, true);
    return samples > 0 && rate > 0 ? { samples, rate } : null;
  }
  return null;
}

/** `buf` cut to the file's real length (see `oggVorbisLength`); unchanged when already right. */
function trimDecoded(ctx: BaseAudioContext, buf: AudioBuffer, len: { samples: number; rate: number } | null): AudioBuffer {
  if (!len) return buf;
  const n = Math.round((len.samples * buf.sampleRate) / len.rate);
  // Only a short codec tail is cut; anything else means the header was not what we think.
  if (n >= buf.length || n < buf.length - 0.1 * buf.sampleRate) return buf;
  const out = ctx.createBuffer(buf.numberOfChannels, n, buf.sampleRate);
  for (let ch = 0; ch < buf.numberOfChannels; ch++) out.copyToChannel(buf.getChannelData(ch).subarray(0, n), ch);
  return out;
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
  /** Commanded gain → member gain (see `Voice.scale`). */
  scale: number;
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

/** A looped voice waiting for its recording to decode (see `VoicePool.waiting`). */
interface Waiting {
  voice: VoiceId;
  bus: Bus;
  pack: string;
  cue: string;
  synth: Record<string, number>;
  /** Latest commanded parameters. */
  params: VoiceParams;
}

/** Entity id of voices played outside the director (non-spatial). */
const LOCAL_ENTITY = 0xfffffffe;

/** Relative change above which a parameter is re-automated. */
const EPS = 0.004;
const changed = (a: number, b: number) => Math.abs(a - b) > EPS * Math.max(Math.abs(a), Math.abs(b), 1e-3);
const moved = (a: number[], b: number[]) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]) > 0.03;

/** gain → lowpass (→ panner) → bus: the per-voice node chain, pooled across voices. */
interface Strip {
  gain: GainNode;
  lp: BiquadFilterNode;
  panner: PannerNode | null;
}
/** Free strips kept per kind (spatial / local); more are dropped for the GC. */
const STRIP_POOL_MAX = 24;

/** Executes voice commands. */
export class VoicePool {
  private voices = new Map<string, Voice>();
  /** Procedural engine instruments by entity. */
  private engines = new Map<number, EngineGroup>();
  /** Released instruments still fading out. */
  private dying: Voice[] = [];
  /** `collect`'s clock for `reapVoice`. */
  private collectT = 0;
  /** Recycle a voice that has ended (`collect`, through `Map.forEach`). */
  private readonly reapVoice = (v: Voice, k: string): void => {
    if (v.endTime <= this.collectT) {
      this.recycle(v);
      this.voices.delete(k);
    }
  };
  private samples = new Map<string, AudioBuffer | "loading" | "failed">();
  /**
   * Looped voices playing a pack's procedural fallback only because their recording was not
   * decoded yet. When it decodes they are re-played from the recording (playtest #47: before,
   * an engine that started early stayed on the fallback synth for good).
   */
  private waiting = new Map<string, Waiting>();
  /** A sample finished decoding since the last `collect()`. */
  private decoded = false;
  /** Voices created / released (debug). */
  created = 0;
  /** Plays performed from a recording / by a synth (debug, e2e). */
  readonly played = { samples: 0, synth: 0, upgraded: 0 };
  /** Strips taken from the pool instead of creating three nodes (debug, perf checks). */
  stripsReused = 0;
  /** Finished voices' node chains, ready for the next one-shot (browser perf pass: a crash or
   * a skid starts several voices a second, each three new nodes). */
  private readonly freeStrips: { spatial: Strip[]; local: Strip[] } = { spatial: [], local: [] };

  /** Modelled impacts / crunch / friction scrapes / horns (owner row 74); `enabled` = setting. */
  readonly impacts: ImpactSynths;

  constructor(
    private ctx: AudioContext,
    private mixer: AudioMixer,
    private lib: AudioLibrary,
  ) {
    this.impacts = new ImpactSynths(ctx, lib, (pack, cue) => this.decodedBuffers(pack, cue), (d, h, t) => mixer.sidechain(d, h, t));
    this.preloadSamples();
  }

  /** Decoded recordings of `pack`/`cue` (empty in procedural mode or while loading). */
  private decodedBuffers(pack: string, cue: string): AudioBuffer[] {
    if (this.source !== "recorded") return [];
    const src = this.cue(pack, cue)?.source;
    if (!src || !("Samples" in src)) return [];
    return src.Samples.files.map((f) => this.samples.get(this.sampleUrl(pack, f))).filter((b): b is AudioBuffer => b instanceof AudioBuffer);
  }

  /** Playing voices, counting each engine layer folded into an instrument. */
  get size(): number {
    let n = this.voices.size;
    for (const g of this.engines.values()) n += g.members.size;
    return n;
  }

  /**
   * Looped director voices playing (engine-instrument members included; local ambience and
   * fading voices not). Equals the director's `looped` count when nothing is stale.
   */
  get looping(): number {
    let n = 0;
    for (const v of this.voices.values()) if (v.endTime === Infinity && v.panner) n++;
    for (const g of this.engines.values()) n += g.members.size;
    return n;
  }

  /** Recorded samples (default) or each pack's procedural `fallback`. */
  source: SoundSource = "recorded";

  /** Optional per-pack routing (siren v2: siren packs go through the siren rack, HRTF-panned). */
  router: { dest(pack: string): AudioNode | null; panning(pack: string): PanningModelType } | null = null;
  /** Engine synthesizer (engine_worklet.ts) for `engine_physical` cues, once loaded. */
  engineHost: EngineSynthHost | null = null;

  private cue(pack: string, cue: string): CueDef | undefined {
    return this.lib.packs[pack]?.cues[cue];
  }

  /**
   * The cue to perform for `pack`/`cue`: in procedural mode (or while a recorded file is not
   * decoded yet / failed to load) the pack's `fallback` cue of the same name, if it has one.
   */
  private resolve(
    pack: string,
    cue: string,
  ): { def: CueDef | undefined; buf: AudioBuffer | null; pack: string; loading: boolean; scale: number } {
    const def = this.cue(pack, cue);
    const fb = this.lib.packs[pack]?.fallback;
    const fbDef = fb ? this.cue(fb, cue) : undefined;
    // The director folds the commanded cue's gain into every gain it sends; a stand-in cue
    // plays at its own gain instead (recorded engine layers carry a level boost the synths
    // must not get).
    const scale = def && fbDef && def.gain > 0 ? fbDef.gain / def.gain : 1;
    if (this.source === "procedural" && fbDef) return { def: fbDef, buf: null, pack: fb!, loading: false, scale };
    const src = def?.source;
    if (src && "Samples" in src) {
      const files = src.Samples.files;
      // Prefer a decoded file; pick among them at random (round robin variety).
      const bufs = files.map((f) => this.samples.get(this.sampleUrl(pack, f))).filter((b) => b instanceof AudioBuffer);
      if (bufs.length) return { def, buf: bufs[Math.floor(Math.random() * bufs.length)] as AudioBuffer, pack, loading: false, scale: 1 };
      const loading = files.some((f) => this.samples.get(this.sampleUrl(pack, f)) === "loading");
      if (fbDef) return { def: fbDef, buf: null, pack: fb!, loading, scale };
      // No fallback and nothing decoded: nothing sensible to play.
      return { def: undefined, buf: null, pack, loading, scale: 1 };
    }
    return { def, buf: null, pack, loading: false, scale: 1 };
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
            .then((b) => {
              // Read before decoding: decodeAudioData detaches the buffer.
              const len = oggVorbisLength(b);
              return this.ctx.decodeAudioData(b).then((buf) => trimDecoded(this.ctx, buf, len));
            })
            .then((buf) => {
              this.samples.set(url, buf);
              this.decoded = true;
            })
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
    const { def, buf, pack: played, loading, scale } = this.resolve(pack, cue);
    const key = voiceKey(voice);
    const commanded = params;
    params = { ...params, gain: params.gain * scale };
    // Whatever holds this slot now goes, whichever path it took: a slot can move between the
    // car's engine instrument (synth) and a sample voice, and a stale one would play on, stuck.
    this.free(voice, 0.02);
    if (looped && loading) this.waiting.set(key, { voice, bus, pack, cue, synth, params: { ...commanded } });
    if (!def) return;
    // The physically modelled engine plays in the engine worklet; its slot output feeds the
    // usual strip below. No host or no free slot: silent (the director only asks for it once
    // the host runs).
    let modelled: SynthVoice | null = null;
    if ("Synth" in def.source && def.source.Synth.synth === "engine_physical") {
      modelled = this.engineHost?.acquire(voice.entity, { ...def.source.Synth.params, ...synth }, params) ?? null;
      if (!modelled) return;
    }
    if (this.isEngineSynth(voice, bus, def)) {
      this.played.synth++;
      this.enginePlay(voice, def, params, synth, scale);
      return;
    }
    const t = this.ctx.currentTime;
    const ext = this.impacts.build(voice, pack, cue, def, buf, params, synth, t);
    if (ext === "skip") return;
    const dest =
      this.router?.dest(played) ?? (this.lib.packs[played]?.kind === "Radio" ? this.mixer.radioIn : this.mixer.input(bus));
    const { gain, lp, panner } = this.takeStrip(voice.entity !== LOCAL_ENTITY, t);
    if (panner) panner.panningModel = this.router?.panning(played) ?? "equalpower";
    gain.gain.setValueAtTime(looped ? 0 : params.gain, t);
    if (looped) gain.gain.setTargetAtTime(params.gain, t, 0.02);
    lp.frequency.setValueAtTime(Math.min(params.lowpass_hz || 18000, 18000), t);
    if (panner) this.setPanner(panner, params.pos, t, false);
    (panner ?? lp).connect(dest);

    let src: AudioBufferSourceNode | null = null;
    let syn: SynthVoice | null = null;
    let rate = 1;
    const source = def?.source;
    if (ext) {
      this.played.synth++;
      syn = ext;
      syn.output.connect(gain);
    } else if (buf) {
      this.played.samples++;
      src = this.ctx.createBufferSource();
      src.buffer = buf;
      src.loop = looped;
      // A recorded siren loop plays at its unit's rate from its unit's point in the loop.
      const vary = looped ? sirenLoopVariation(this.lib, played, cue, synth) : null;
      rate = vary?.rate ?? 1;
      src.playbackRate.value = params.pitch * rate;
      src.connect(gain);
      src.start(t, vary ? vary.offset * buf.duration : 0);
    } else {
      this.played.synth++;
      const name = source && "Synth" in source ? source.Synth.synth : "noise";
      const sp = { ...(source && "Synth" in source ? source.Synth.params : {}), ...synth };
      syn = modelled ?? makeSynth(this.ctx, name, sp, params, t);
      syn.output.connect(gain);
    }
    // A recorded one-shot can outlast the director's estimate: keep the voice until it ends.
    const dur = Math.max(def?.duration_s ?? 1, buf && !looped ? buf.duration / Math.max(params.pitch, 0.25) : 0);
    this.voices.set(key, { synth: syn, src, gain, lp, panner, endTime: looped ? Infinity : t + dur + 0.5, last: params, scale, rate });
    this.created++;
  }

  set(voice: VoiceId, params: VoiceParams): void {
    const w = this.waiting.get(voiceKey(voice));
    if (w) w.params = params;
    const g = this.engines.get(voice.entity);
    const m = g?.members.get(voice.slot);
    if (g && m) {
      m.gain = params.gain * m.scale;
      g.pending = params;
      g.dirty = true;
      return;
    }
    const v = this.voices.get(voiceKey(voice));
    if (!v) return;
    this.applyParams(v, v.scale === 1 ? params : { ...params, gain: params.gain * v.scale });
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
    if (v.panner && moved(params.pos, l.pos)) {
      this.setPanner(v.panner, params.pos, t, true);
      l.pos = params.pos;
    }
    const pitchMoved = changed(params.pitch, l.pitch);
    if (v.src && pitchMoved) v.src.playbackRate.setTargetAtTime(params.pitch * v.rate, t, 0.02);
    // Sirens schedule their pitch curve ahead on each update (their phase moves every frame);
    // others only on change.
    const synthChanged =
      changed(params.freq_hz, l.freq_hz) || pitchMoved || changed(params.load, l.load) || params.phase !== l.phase;
    // The modelled engine and clocked synths (impacts, horns) read every frame.
    if (v.synth && (synthChanged || params.engine || (v.synth as ClockedSynth).everyFrame)) v.synth.update(params, t);
    if (synthChanged) {
      l.freq_hz = params.freq_hz;
      // Only once applied: a siren's phase changes every frame, and taking the pitch here on
      // every phase change kept a recording's playback rate from ever following a slow pitch
      // drift (Doppler of a receding siren stuck ~5 % sharp).
      if (pitchMoved) l.pitch = params.pitch;
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

  private enginePlay(voice: VoiceId, def: CueDef, params: VoiceParams, synth: Record<string, number>, scale: number): void {
    const sp = { ...("Synth" in def.source ? def.source.Synth.params : {}), ...synth };
    const member: EngineMember = {
      gain: params.gain,
      bright: sp.bright ?? 0.5,
      off: sp.off ? 1 : 0,
      noise: sp.noise ?? 0.1,
      limiter: !!sp.chop_hz,
      scale,
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
        scale: 1,
        rate: 1,
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
    this.waiting.delete(voiceKey(voice));
    this.free(voice, fadeS);
  }

  /** Release whatever plays `voice`: an engine-instrument member and / or a voice. */
  private free(voice: VoiceId, fadeS: number): void {
    const g = this.engines.get(voice.entity);
    if (g && g.members.delete(voice.slot)) {
      if (g.members.size === 0) {
        this.engines.delete(voice.entity);
        this.release(g, this.ctx.currentTime, fadeS);
        this.dying.push(g);
      } else {
        g.dirty = true;
      }
    }
    const key = voiceKey(voice);
    const v = this.voices.get(key);
    if (!v) return;
    this.release(v, this.ctx.currentTime, fadeS);
    // Keep it until its fade ends, but under a key nothing addresses any more.
    this.voices.delete(key);
    this.dying.push(v);
  }

  /** Re-play voices whose recording has decoded since they started on the fallback. */
  private upgrade(): void {
    this.decoded = false;
    if (this.source !== "recorded") return;
    for (const [key, w] of [...this.waiting]) {
      const r = this.resolve(w.pack, w.cue);
      if (r.buf) {
        this.waiting.delete(key);
        this.play(w.voice, w.bus, w.pack, w.cue, true, w.params, w.synth);
        this.played.upgraded++;
      } else if (!r.loading) {
        this.waiting.delete(key); // failed: stays on the fallback
      }
    }
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

  /** A node chain: from the pool (automation cleared) or new. Not connected to a bus yet. */
  private takeStrip(spatial: boolean, t: number): Strip {
    const free = spatial ? this.freeStrips.spatial : this.freeStrips.local;
    const s = free.pop();
    if (s) {
      this.stripsReused++;
      s.gain.gain.cancelScheduledValues(t);
      s.lp.frequency.cancelScheduledValues(t);
      if (s.panner?.positionX) for (const p of [s.panner.positionX, s.panner.positionY, s.panner.positionZ]) p.cancelScheduledValues(t);
      return s;
    }
    const gain = this.ctx.createGain();
    const lp = this.ctx.createBiquadFilter();
    lp.type = "lowpass";
    gain.connect(lp);
    let panner: PannerNode | null = null;
    if (spatial) {
      panner = this.ctx.createPanner();
      panner.panningModel = "equalpower";
      panner.distanceModel = "linear";
      panner.rolloffFactor = 0;
      lp.connect(panner);
    }
    return { gain, lp, panner };
  }

  /** A finished voice: detach its source and bus, keep its node chain for the next voice. */
  private recycle(v: Voice): void {
    v.src?.disconnect();
    v.synth?.output.disconnect();
    (v.panner ?? v.lp).disconnect();
    const free = v.panner ? this.freeStrips.spatial : this.freeStrips.local;
    if (free.length < STRIP_POOL_MAX) free.push({ gain: v.gain, lp: v.lp, panner: v.panner });
  }

  /** Disconnect finished voices. Call once per frame. */
  collect(): void {
    if (this.decoded) this.upgrade();
    for (const g of this.engines.values()) if (g.dirty) this.flushEngine(g);
    const t = this.ctx.currentTime;
    // Per frame: no [key, voice] entry arrays, no filtered copy of `dying` (allocation pass
    // 2026-10-20). Same order and effect as the for-of / filter it replaces.
    this.collectT = t;
    this.voices.forEach(this.reapVoice);
    const dying = this.dying;
    let keep = 0;
    for (let i = 0; i < dying.length; i++) {
      const v = dying[i];
      if (v.endTime > t) dying[keep++] = v;
      else (v.panner ?? v.lp).disconnect();
    }
    dying.length = keep;
    this.engineHost?.flush();
  }

  stopAll(): void {
    const t = this.ctx.currentTime;
    for (const v of this.voices.values()) {
      this.release(v, t, 0.05);
      this.dying.push(v);
    }
    this.voices.clear();
    for (const g of this.engines.values()) {
      this.release(g, t, 0.05);
      this.dying.push(g);
    }
    this.engines.clear();
    this.waiting.clear();
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
