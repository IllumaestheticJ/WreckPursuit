// Web Audio backend for wp_audio (workstream 13). The Rust AudioDirector (via wp_wasm's
// audio_frame) decides everything that plays; this module turns its commands into Web Audio
// nodes. Usage from main.ts:
//
//   const audio = new AudioSystem(scene.world, overlay);
//   ...each frame: audio.update(rig.camera, events, dt, app.focus);
//
// The AudioContext starts on the first click / key press (browser autoplay rules). On start
// the director is reset so it re-issues every looped voice.

import * as THREE from "three";
import { audioBridge } from "./bridge";
import { AudioPanel } from "./panel";
import { ScannerPlayer } from "./scanner";
import { AudioMixer, setListener, VoicePool } from "./voices";
import type { AudioCommand, AudioLibrary, Bus, DirectorStats, RadioPriority, SampleStatus, SoundSource } from "./types";

const TEST_LINES: Record<string, string[]> = {
  Routine: ["Unit fourteen, respond to a fender bender on Harbor and Fifth.", "Copy dispatch, fourteen en route."],
  Pursuit: ["Twelve in pursuit, grey coupe northbound on the expressway, speeds over one twenty.", "Suspect vehicle took the off ramp, heading downtown."],
  Urgent: ["All units, air one has visual on the suspect, deploy spikes at the bridge.", "Officer needs assistance, Fifth and Market!"],
};

/** City ambience loop (optional World-pack cue, played by the backend, not the director). */
const AMBIENCE_CUE = "ambient/city_night";

export class AudioSystem {
  private ctx: AudioContext | null = null;
  private mixer: AudioMixer | null = null;
  private pool: VoicePool | null = null;
  private scanner: ScannerPlayer | null = null;
  private lib: AudioLibrary | null = null;
  private panel: AudioPanel;
  private lastPos: THREE.Vector3 | null = null;
  private vel = new THREE.Vector3();
  private muted = false;
  private failed = false;
  private focus = 0;
  private lineNo = 0;
  private lastStats: DirectorStats | null = null;
  private stopAmbience: (() => void) | null = null;
  private readonly listenerArr = new Float32Array(12);
  private readonly scratch = Array.from({ length: 7 }, () => new THREE.Vector3());
  private readonly scratchQ = new THREE.Quaternion();
  /** Smoothed cost of one audio frame (director in wasm + JSON + Web Audio calls), ms. */
  frameMs = 0;
  /** Smoothed cost of the wasm director call alone (incl. JSON), ms. */
  directorMs = 0;
  /** Smoothed cost of applying the commands to Web Audio, ms. */
  applyMs = 0;
  /** Audio frames run (one per rendered frame while running). */
  frames = 0;
  /** Plays per cue since start (e2e checks and debugging). */
  readonly cueCounts: Record<string, number> = {};

  constructor(private worldFrame: THREE.Object3D, parent: HTMLElement) {
    this.panel = new AudioPanel(parent, {
      start: () => this.start(),
      setMaster: (v) => this.mixer?.master.gain.setTargetAtTime(this.muted ? 0 : v, this.ctx!.currentTime, 0.02),
      setBus: (b, v) => this.mixer?.setUserGain(b, v),
      setTier: (t) => this.safe(() => audioBridge.setScannerTier(t)),
      setTts: (on) => {
        if (this.scanner) this.scanner.tts = on;
      },
      setSiren: (m) => this.setSiren(m),
      testRadio: (p) => this.testRadio(p),
      setSource: (src) => this.setSource(src),
      setAmbience: () => this.restartAmbience(),
    });
    (window as unknown as { __wpAudio?: AudioSystem }).__wpAudio = this;
    const kick = () => this.start();
    window.addEventListener("pointerdown", kick);
    // Mute is the rebindable "mute" action (main.ts dispatches it to toggleMute).
    window.addEventListener("keydown", kick);
  }

  private safe(f: () => void): void {
    try {
      f();
    } catch (e) {
      console.warn("audio:", e);
    }
  }

  /** Create / resume the AudioContext (must run inside a user gesture). */
  start(): void {
    if (this.failed) return;
    if (this.ctx) {
      if (this.ctx.state === "suspended") void this.ctx.resume();
      return;
    }
    try {
      this.lib = audioBridge.library();
    } catch (e) {
      this.failed = true;
      this.panel.setStatus(`audio unavailable: ${e}`);
      return;
    }
    this.ctx = new AudioContext({ latencyHint: "interactive" });
    this.mixer = new AudioMixer(this.ctx);
    this.pool = new VoicePool(this.ctx, this.mixer, this.lib);
    this.pool.source = this.panel.settings.source;
    this.scanner = new ScannerPlayer(this.pool);
    const s = this.panel.settings;
    this.mixer.master.gain.value = s.master;
    for (const [b, v] of Object.entries(s.buses)) this.mixer.setUserGain(b as Bus, v);
    this.scanner.tts = s.tts;
    this.safe(() => audioBridge.setScannerTier(s.tier));
    // Forget anything the director thinks is playing: it re-plays every loop next frame.
    this.resetDirector();
    this.restartAmbience();
    this.panel.setStatus(`${this.ctx.sampleRate} Hz · max ${this.lib.config.max_voices} voices`);
  }

  /** Recorded samples (default) or the procedural fallback packs; loops restart with the new source. */
  setSource(source: SoundSource): void {
    this.panel.settings.source = source;
    if (!this.pool) return;
    this.pool.source = source;
    this.resetDirector();
    this.restartAmbience();
  }

  /**
   * Reset the director and apply the Stop commands it returns (it re-plays every loop on the
   * next frame). Dropping them left the old loops playing next to the new ones.
   */
  private resetDirector(): void {
    this.safe(() => this.apply(audioBridge.reset()));
  }

  /** Current sound source. */
  soundSource(): SoundSource {
    return this.pool?.source ?? this.panel.settings.source;
  }

  /** Recorded sample loading progress (null before the AudioContext starts). */
  sampleStatus(): SampleStatus | null {
    return this.pool?.sampleStatus() ?? null;
  }

  /** (Re)start the city ambience bed: recorded only, non-spatial, on the World bus. */
  private restartAmbience(): void {
    this.stopAmbience?.();
    this.stopAmbience = null;
    const pool = this.pool;
    const gain = this.panel.settings.ambience;
    if (!pool || gain <= 0 || pool.source !== "recorded") return;
    const pack = Object.keys(this.lib?.packs ?? {}).find((p) => pool.has(p, AMBIENCE_CUE));
    if (pack) this.stopAmbience = pool.playLocal(pack, AMBIENCE_CUE, gain, "World");
  }

  /**
   * Player volumes from the game menu (0..=1 each): master, the Engine / World (effects) /
   * Police (radio, sirens) buses and the ambience bed. Saved with the audio panel's settings.
   */
  setMix(v: { master: number; engine: number; effects: number; police: number; ambience: number }): void {
    const s = this.panel.settings;
    const ambienceChanged = s.ambience !== v.ambience;
    s.master = v.master;
    s.buses.Engine = v.engine;
    s.buses.World = v.effects;
    s.buses.Police = v.police;
    s.ambience = v.ambience;
    this.panel.save();
    if (this.mixer && this.ctx) {
      if (!this.muted) this.mixer.master.gain.setTargetAtTime(v.master, this.ctx.currentTime, 0.02);
      this.mixer.setUserGain("Engine", v.engine);
      this.mixer.setUserGain("World", v.effects);
      this.mixer.setUserGain("Police", v.police);
    }
    if (ambienceChanged) this.restartAmbience();
  }

  /** Current player volumes (audio panel settings). */
  mix(): { master: number; engine: number; effects: number; police: number; ambience: number } {
    const s = this.panel.settings;
    return { master: s.master, engine: s.buses.Engine, effects: s.buses.World, police: s.buses.Police, ambience: s.ambience };
  }

  toggleMute(): void {
    this.muted = !this.muted;
    if (this.mixer && this.ctx) {
      this.mixer.master.gain.setTargetAtTime(this.muted ? 0 : this.panel.settings.master, this.ctx.currentTime, 0.02);
    }
    this.panel.setStatus(this.muted ? "muted" : "unmuted");
  }

  /** Siren mode for the focused car: "Default" | "Off" | "Auto" | "Wail" | "Yelp" | "Phaser" | "HiLo". */
  setSiren(mode: string): void {
    this.safe(() => audioBridge.setSiren(this.focus, mode === "Default" ? "" : mode));
  }

  /** Scanner tier: "None" | "Basic" | "Pro" | "Elite" (wp_police tiers 0/1/2). */
  setTier(tier: string): void {
    this.safe(() => audioBridge.setScannerTier(tier));
  }

  /** Whether the AudioContext is running. */
  running(): boolean {
    return this.ctx?.state === "running";
  }

  /** Recent scanner lines, newest first. */
  radioLog(): unknown[] {
    return this.scanner?.log ?? [];
  }

  /** Queue a canned test line of priority `p` as if police said it now. */
  testRadio(p: RadioPriority): void {
    const lines = TEST_LINES[p] ?? TEST_LINES.Routine;
    const text = lines[this.lineNo++ % lines.length];
    this.safe(() => audioBridge.radio(12 + (this.lineNo % 3), p, text));
  }

  /** Per-frame update. `events` = this frame's drained SimEvents: decoded, or the JSON text the
   * sim produced (`sim.lastEventsJson()`, passed through without a re-stringify). */
  update(camera: THREE.Camera, events: unknown[] | string, dt: number, focus: number): void {
    this.focus = focus;
    const ctx = this.ctx;
    if (!ctx || ctx.state !== "running" || !this.pool || !this.mixer || !this.scanner) return;
    // Camera → sim world space (the scene's world group is rotated Z-up → Y-up).
    // Scratch vectors (no allocation per frame).
    const [camPos, pos, fwdW, upW, fwd, up, v] = this.scratch;
    camera.getWorldPosition(camPos);
    this.worldFrame.worldToLocal(pos.copy(camPos));
    camera.getWorldDirection(fwdW);
    upW.set(0, 1, 0).applyQuaternion(camera.getWorldQuaternion(this.scratchQ));
    this.worldFrame.worldToLocal(fwd.copy(camPos).add(fwdW)).sub(pos).normalize();
    this.worldFrame.worldToLocal(up.copy(camPos).add(upW)).sub(pos).normalize();
    if (this.lastPos && dt > 0) {
      v.copy(pos).sub(this.lastPos).divideScalar(dt);
      // Camera cuts teleport: ignore absurd speeds instead of a Doppler squawk.
      if (v.length() < 120) this.vel.lerp(v, 0.3);
      else this.vel.set(0, 0, 0);
    }
    this.lastPos = (this.lastPos ?? new THREE.Vector3()).copy(pos);
    const a = this.listenerArr;
    a.set([pos.x, pos.y, pos.z, this.vel.x, this.vel.y, this.vel.z, fwd.x, fwd.y, fwd.z, up.x, up.y, up.z]);
    setListener(ctx, [pos.x, pos.y, pos.z], [fwd.x, fwd.y, fwd.z], [up.x, up.y, up.z]);

    const t0 = performance.now();
    let out;
    try {
      const json = typeof events === "string" ? events : events.length ? JSON.stringify(events) : "";
      out = audioBridge.frame(a, json, focus);
    } catch (e) {
      this.failed = true;
      this.panel.setStatus(`audio stopped: ${e}`);
      this.pool.stopAll();
      return;
    }
    const t1 = performance.now();
    this.directorMs += (t1 - t0 - this.directorMs) * 0.1;
    this.apply(out.commands);
    const t2 = performance.now();
    this.applyMs += (t2 - t1 - this.applyMs) * 0.1;
    this.pool.collect();
    this.frameMs += (performance.now() - t0 - this.frameMs) * 0.1;
    this.frames++;
    this.lastStats = out.stats;
    this.panel.update(out.stats, this.pool.size, this.scanner.log, this.pool.sampleStatus());
  }

  private apply(cmds: AudioCommand[]): void {
    const pool = this.pool!;
    for (const c of cmds) {
      if ("Play" in c) {
        const p = c.Play;
        this.cueCounts[p.cue] = (this.cueCounts[p.cue] ?? 0) + 1;
        pool.play(p.voice, p.bus, p.pack, p.cue, p.looped, p.params, p.synth);
      } else if ("Set" in c) {
        pool.set(c.Set.voice, c.Set.params);
      } else if ("Stop" in c) {
        pool.stop(c.Stop.voice, c.Stop.fade_s);
      } else if ("Duck" in c) {
        this.mixer!.duck(c.Duck.bus, c.Duck.gain, c.Duck.fade_s);
      } else if ("RadioSay" in c) {
        const r = c.RadioSay;
        this.scanner!.say(r.line, r.unit, r.text, r.priority, r.voice_pitch);
      } else if ("RadioEnd" in c) {
        this.scanner!.end(c.RadioEnd.line, c.RadioEnd.interrupted);
      }
    }
  }

  /** Looped voices the backend plays vs the director's count (e2e: no stale voices). */
  loopCounts(): { backend: number; director: number } | null {
    if (!this.pool || !this.lastStats) return null;
    return { backend: this.pool.looping, director: this.lastStats.looped };
  }

  /** Last director counters (e2e / HUD). */
  stats(): DirectorStats | null {
    return this.lastStats;
  }
}
