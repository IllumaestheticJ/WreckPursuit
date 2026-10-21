// Impact / crunch / scrape / horn synthesis glue (owner row 74). The voice pool asks
// `ImpactSynths.build` before its own synths; everything here lives outside synth.ts so the
// engine synth (machine J) and this work do not touch the same file.
//
// Setting `enabled` (audio panel "impacts & horns: synth / recorded", default synth):
//   synth    — `impact/body` and `impact/crunch` play; recorded scrape loops get the friction
//              layer; recorded horn cues play the modelled horn of their procedural twin.
//   recorded — the previous behaviour: recorded layers only (`impact/body` / `impact/crunch`
//              are skipped), recorded horns and scrape loops as they were.
// Procedural cues that name these synths (`disc_horn`, `air_horn`, `friction`) always use them.

import type { SynthVoice } from "../synth";
import type { AudioLibrary, CueDef, VoiceId, VoiceParams } from "../types";
import { hornSynth } from "../horn";
import { impactBody, type ImpactHost } from "./body";
import { crunch } from "./crunch";
import { frictionScrape } from "./scrape";

/** wp_audio `slot::HORN`. */
const SLOT_HORN = 0x310;

/** Cues whose recordings feed the crunch grains (world packs). */
const CRUNCH_SOURCES = ["impact/structural_crunch", "impact/panel_dent"];

/** A SynthVoice that wants `update` every frame even when nothing changed (grain scheduler). */
export interface ClockedSynth extends SynthVoice {
  everyFrame?: boolean;
}

/** Timing of impact voices (e2e: contact → voice latency). */
export interface ImpactLogEntry {
  /** `performance.now()` when the voice was built. */
  wall: number;
  /** Audio-clock start time, s. */
  audio: number;
  /** Audio clock when the play call happened, s. */
  now: number;
  name: string;
  entity: number;
  strength: number;
  /** Main-thread time to build the voice's node graph, ms. */
  buildMs: number;
}

export class ImpactSynths implements ImpactHost {
  /** Impact / horn synthesis on (default) or recorded samples only. */
  enabled = true;
  /** Recent impact voices, newest last (bounded). */
  readonly log: ImpactLogEntry[] = [];
  /** Voices built per synth name (debug / e2e). */
  readonly built: Record<string, number> = {};

  constructor(
    private readonly ctx: BaseAudioContext,
    private readonly lib: AudioLibrary,
    /** Decoded recordings of a pack's cue (empty while loading / in procedural mode). */
    private readonly buffers: (pack: string, cue: string) => AudioBuffer[],
    /** Sidechain stage of the mixer (other buses). */
    private readonly duck: (depth: number, holdS: number, t: number) => void,
  ) {}

  sidechain(depth: number, holdS: number, t: number): void {
    this.duck(depth, holdS, t);
  }

  private synthOf(def: CueDef | undefined): { name: string; params: Record<string, number> } | null {
    return def && "Synth" in def.source ? { name: def.source.Synth.synth, params: def.source.Synth.params } : null;
  }

  /**
   * A synth for this play, `"skip"` (play nothing), or null (the pool's normal path).
   * `def` is the cue actually performed (after recorded → fallback resolution) and `buf` its
   * decoded recording, if any; `pack`/`cue` the commanded cue.
   */
  build(
    voice: VoiceId,
    pack: string,
    cue: string,
    def: CueDef | undefined,
    buf: AudioBuffer | null,
    params: VoiceParams,
    synth: Record<string, number>,
    t: number,
  ): ClockedSynth | "skip" | null {
    const own = this.synthOf(def);
    const sp = { ...(own?.params ?? {}), ...synth };
    const name = own?.name;
    if (name === "impact_body" || name === "crunch") {
      if (!this.enabled) return "skip";
      return this.make(name, voice, pack, sp, params, t);
    }
    if (name === "disc_horn" || name === "air_horn" || name === "friction") return this.make(name, voice, pack, sp, params, t, buf);
    if (!this.enabled) return null;
    if (voice.slot === SLOT_HORN) {
      // A recorded horn: play its procedural twin's model.
      const fb = this.lib.packs[pack]?.fallback;
      const twin = this.synthOf(fb ? this.lib.packs[fb]?.cues[cue] : undefined);
      const twinDef = fb ? this.lib.packs[fb]?.cues[cue] : undefined;
      if (twin && twinDef && (twin.name === "disc_horn" || twin.name === "air_horn")) {
        const v = this.make(twin.name, voice, pack, { ...twin.params, ...synth }, params, t);
        // The commanded gain carries the recording's cue gain: play at the twin's own level.
        const recGain = def?.gain ?? 1;
        const scale = recGain > 0 ? twinDef.gain / recGain : 1;
        const g = this.ctx.createGain();
        g.gain.value = scale;
        v.output.connect(g);
        return { ...v, output: g, update: v.update, stop: v.stop };
      }
      return null;
    }
    if (synth.friction) return this.make("friction", voice, pack, sp, params, t, buf);
    return null;
  }

  private make(
    name: string,
    voice: VoiceId,
    pack: string,
    sp: Record<string, number>,
    p: VoiceParams,
    t: number,
    buf: AudioBuffer | null = null,
  ): ClockedSynth {
    this.built[name] = (this.built[name] ?? 0) + 1;
    switch (name) {
      case "impact_body": {
        const wall = performance.now();
        const v = impactBody(this.ctx, this, sp, p, t);
        const buildMs = performance.now() - wall;
        this.log.push({ wall, audio: t, now: this.ctx.currentTime, name, entity: voice.entity, strength: sp.strength ?? 0, buildMs });
        if (this.log.length > 64) this.log.shift();
        return v;
      }
      case "crunch": {
        const bufs = CRUNCH_SOURCES.flatMap((c) => this.buffers(pack, c));
        const v: ClockedSynth = crunch(this.ctx, bufs, { ...sp, seed: voice.entity }, p, t);
        v.everyFrame = true;
        return v;
      }
      case "friction":
        return frictionScrape(this.ctx, buf, sp, p, t);
      case "air_horn":
        return hornSynth(this.ctx, "air", sp, p, t);
      default:
        return hornSynth(this.ctx, "disc", sp, p, t);
    }
  }
}
