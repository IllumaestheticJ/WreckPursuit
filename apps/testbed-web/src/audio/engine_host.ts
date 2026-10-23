// Main-thread side of the engine synthesizer (engine_worklet.ts): one AudioWorkletNode with
// ENGINE_SLOTS mono outputs. Each `engine/physical` voice the director plays gets a slot; the
// slot's output feeds the voice's usual strip (gain → air-absorption low-pass → panner → Engine
// bus) in VoicePool, so distance, panning and bus gains work as for every other voice. The
// per-frame engine state of every slot goes to the audio thread in one message per frame
// (`flush`, called from VoicePool.collect).

import { EF, EF_COUNT, ENGINE_PROCESSOR, ENGINE_SLOTS, engineWorkletSource } from "./engine_worklet";
import type { SynthVoice } from "./synth";
import type { VoiceParams } from "./types";

/** Per-engine test counters from the worklet (see `EngineDspApi`). */
export type EngineCounters = Record<string, number> & { popAt?: number[] };

interface Slot {
  /** Entity the slot plays (0 = free). */
  entity: number;
  /** Fixed node fed by the worklet output; the voice strip connects to it. */
  out: GainNode;
  /** Audio time after which a released slot can be reused. */
  freeAt: number;
  releasing: boolean;
}

/** Load the worklet module into `ctx` (once per context). */
export async function loadEngineWorklet(ctx: BaseAudioContext): Promise<void> {
  const url = URL.createObjectURL(new Blob([engineWorkletSource()], { type: "application/javascript" }));
  try {
    await ctx.audioWorklet.addModule(url);
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** Owns the engine worklet node and its slots. */
export class EngineSynthHost {
  readonly node: AudioWorkletNode;
  private readonly slots: Slot[] = [];
  private readonly frame = new Float32Array(ENGINE_SLOTS * EF_COUNT);
  private dirty = false;
  /** Entity heard from inside (interior camera) and how much (0 = outside, 1 = cockpit). */
  private focus = 0;
  private interior = 0;
  /** Slots handed out / refused (debug, e2e). */
  readonly stats = { acquired: 0, refused: 0 };

  private constructor(private readonly ctx: BaseAudioContext) {
    this.node = new AudioWorkletNode(ctx, ENGINE_PROCESSOR, {
      numberOfInputs: 0,
      numberOfOutputs: ENGINE_SLOTS,
      outputChannelCount: new Array(ENGINE_SLOTS).fill(1),
    });
    for (let i = 0; i < ENGINE_SLOTS; i++) {
      const out = ctx.createGain();
      this.node.connect(out, i);
      this.slots.push({ entity: 0, out, freeAt: 0, releasing: false });
    }
  }

  /** Load the worklet and create the host; rejects if AudioWorklet is unavailable. */
  static async create(ctx: BaseAudioContext): Promise<EngineSynthHost> {
    await loadEngineWorklet(ctx);
    return new EngineSynthHost(ctx);
  }

  /** Engines currently playing. */
  get playing(): number {
    return this.slots.filter((s) => s.entity !== 0 && !s.releasing).length;
  }

  /**
   * A synth voice for `entity`'s engine with the car's static description `spec` (the
   * `synth` map of the Play: wp_audio::engine_model::EngineVoiceSpec). A slot still fading
   * out is taken over when none is free; null only if every slot holds a live engine (the
   * director models at most 7, so that means a bug).
   */
  acquire(entity: number, spec: Record<string, number>, p: VoiceParams): SynthVoice | null {
    const t = this.ctx.currentTime;
    let idx = this.slots.findIndex((s) => s.entity === 0 && s.freeAt <= t);
    if (idx < 0) idx = this.slots.findIndex((s) => s.releasing && s.freeAt <= t);
    if (idx < 0) {
      // Every slot busy: take over the fade-out that ends first (cut a little short).
      let best = -1;
      for (let i = 0; i < this.slots.length; i++) {
        const s = this.slots[i];
        if (s.releasing && (best < 0 || s.freeAt < this.slots[best].freeAt)) best = i;
      }
      idx = best;
    }
    if (idx < 0) {
      this.stats.refused++;
      return null;
    }
    const slot = this.slots[idx];
    slot.entity = entity;
    slot.releasing = false;
    this.stats.acquired++;
    this.node.port.postMessage({ t: "spec", slot: idx, spec });
    this.write(idx, p);
    const host = this;
    return {
      output: slot.out,
      update(params: VoiceParams) {
        if (slot.entity === entity && !slot.releasing) host.write(idx, params);
      },
      stop(end: number) {
        if (slot.entity !== entity) return;
        // The strip fades the gain; the slot keeps rendering until then.
        slot.releasing = true;
        slot.freeAt = end + 0.05;
      },
    };
  }

  /** Interior perspective: `entity`'s engine heard from inside the car with weight `amount`. */
  setInterior(entity: number, amount: number): void {
    if (entity === this.focus && Math.abs(amount - this.interior) < 1e-3) return;
    this.focus = entity;
    this.interior = amount;
    for (let i = 0; i < ENGINE_SLOTS; i++) {
      const s = this.slots[i];
      this.frame[i * EF_COUNT + EF.interior] = s.entity !== 0 && s.entity === entity ? amount : 0;
    }
    this.dirty = true;
  }

  private write(i: number, p: VoiceParams): void {
    const e = p.engine;
    const f = this.frame;
    const o = i * EF_COUNT;
    f[o + EF.active] = 1;
    if (e) {
      f[o + EF.rpm] = e.rpm;
      f[o + EF.throttle] = e.throttle;
      f[o + EF.pedal] = e.pedal;
      f[o + EF.gear] = e.gear;
      f[o + EF.speed] = e.speed_mps;
      f[o + EF.boost] = e.boost;
      f[o + EF.health] = e.health;
      f[o + EF.blowoffs] = e.blowoffs;
    }
    f[o + EF.load] = p.load;
    f[o + EF.doppler] = p.doppler || 1;
    f[o + EF.interior] = this.slots[i].entity === this.focus ? this.interior : 0;
    this.dirty = true;
  }

  /** Post this frame's engine states and recycle slots whose fade ended. Once per frame. */
  flush(): void {
    const t = this.ctx.currentTime;
    for (let i = 0; i < ENGINE_SLOTS; i++) {
      const s = this.slots[i];
      if (s.releasing && s.freeAt <= t) {
        s.entity = 0;
        s.releasing = false;
        this.frame[i * EF_COUNT + EF.active] = 0;
        this.node.port.postMessage({ t: "off", slot: i });
        this.dirty = true;
      }
    }
    if (!this.dirty) return;
    this.dirty = false;
    this.node.port.postMessage({ t: "frame", data: this.frame.slice() });
  }

  /**
   * The worklet's per-slot counters (events, misfires, pops, cuts, blow-offs) and pop onsets
   * (`popAt`, samples since the slot's engine started); tests.
   */
  counters(): Promise<(EngineCounters | null)[]> {
    return new Promise((resolve) => {
      const port = this.node.port;
      const done = (e: MessageEvent) => {
        if ((e.data as { t?: string })?.t !== "stats") return;
        port.removeEventListener("message", done);
        resolve((e.data as { counters: (EngineCounters | null)[] }).counters);
      };
      port.addEventListener("message", done);
      port.start();
      port.postMessage({ t: "stats" });
      setTimeout(() => resolve([]), 2000);
    });
  }

  /** Stop and disconnect everything. */
  dispose(): void {
    for (let i = 0; i < ENGINE_SLOTS; i++) this.node.port.postMessage({ t: "off", slot: i });
    this.node.disconnect();
  }
}
