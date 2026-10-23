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
import { EngineSynthHost } from "./engine_host";
import { renderFlap } from "./flat_tyre";
import { CurvePlan, renderSiren, sirenStats } from "./siren";
import { SirenLab } from "./siren_lab";
import { SirenRack } from "./siren_rack";
import { AudioMixer, setListener, VoicePool } from "./voices";
import type { AudioCommand, AudioLibrary, Bus, DirectorStats, EngineVoicing, RadioPriority, SampleStatus, SoundSource } from "./types";

const TEST_LINES: Record<string, string[]> = {
  Routine: ["Unit fourteen, respond to a fender bender on Harbor and Fifth.", "Copy dispatch, fourteen en route."],
  Pursuit: ["Twelve in pursuit, grey coupe northbound on the expressway, speeds over one twenty.", "Suspect vehicle took the off ramp, heading downtown."],
  Urgent: ["All units, air one has visual on the suspect, deploy spikes at the bridge.", "Officer needs assistance, Fifth and Market!"],
};

/** City ambience loop (optional World-pack cue, played by the backend, not the director). */
const AMBIENCE_CUE = "ambient/city_night";

/**
 * How much of the focus car's engine is heard from inside, by camera mode (cameras.ts): the
 * cockpit is inside; hood and bumper cameras sit on the body, outside the cabin but close to
 * the engine bay; every other camera is outside.
 */
const INTERIOR_BY_CAMERA: Record<string, number> = { cockpit: 1, hood: 0.35, bumper: 0.15 };

export class AudioSystem {
  private ctx: AudioContext | null = null;
  private mixer: AudioMixer | null = null;
  private pool: VoicePool | null = null;
  private scanner: ScannerPlayer | null = null;
  /** Siren v2 sub-bus and acoustics (null before the AudioContext starts). */
  sirens: SirenRack | null = null;
  /** Siren lab in the audio panel. */
  readonly sirenLab: SirenLab;
  /** Siren synth counters (`curveErrors` must stay 0) and an offline renderer (e2e). */
  readonly sirenStats = sirenStats;
  readonly renderSiren = renderSiren;
  readonly SirenCurvePlan = CurvePlan;
  /** Offline render of the flat-tyre flap at a wheel rate (e2e). */
  readonly renderFlap = renderFlap;
  private lib: AudioLibrary | null = null;
  private panel: AudioPanel;
  private lastPos: THREE.Vector3 | null = null;
  private vel = new THREE.Vector3();
  private muted = false;
  private paused = false;
  /** Photo mode: the mix is faded out while the game is frozen (the mute toggle is kept). */
  private ducked = false;
  private failed = false;
  private focus = 0;
  private lineNo = 0;
  private lastStats: DirectorStats | null = null;
  /** Scanner tier the director was given last ("None" hears nothing). */
  private tier = "Pro";
  /** Shows every line the scanner says (the radio strip, main.ts); `tier` = police priority
   * tier (row 144), 0 unknown. */
  onRadioSay: ((unit: number, text: string, tier: number) => void) | null = null;
  /** "Radio: Important only" (row 144): only police tiers 1–3 are played. */
  private importantOnly = false;
  private stopAmbience: (() => void) | null = null;
  /** Engine synthesizer (AudioWorklet), once loaded; null if unavailable. */
  private engineHost: EngineSynthHost | null = null;
  private engineHostState: "loading" | "ready" | "failed" | "off" = "off";
  private cameraMode = "chase";
  private readonly listenerArr = new Float32Array(12);
  private readonly scratch = Array.from({ length: 7 }, () => new THREE.Vector3());
  private readonly scratchQ = new THREE.Quaternion();
  private readonly scratchM = new THREE.Matrix4();
  private readonly scratchP = new THREE.Vector3();
  private readonly scratchS = new THREE.Vector3();
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
      setMaster: (v) => this.mixer?.master.gain.setTargetAtTime(this.muted || this.ducked ? 0 : v, this.ctx!.currentTime, 0.02),
      setBus: (b, v) => this.mixer?.setUserGain(b, v),
      setTier: (t) => this.setTier(t),
      setTts: (on) => {
        if (this.scanner) this.scanner.tts = on;
      },
      setSiren: (m) => this.setSiren(m),
      testRadio: (p) => this.testRadio(p),
      setSource: (src) => this.setSource(src),
      setAmbience: () => this.restartAmbience(),
      setEngineVoicing: (v) => this.setEngineVoicing(v),
      setImpacts: (m) => this.setImpacts(m),
    });
    this.sirenLab = new SirenLab(() =>
      this.ctx && this.sirens && this.lib && this.ctx.state === "running" ? { ctx: this.ctx, rack: this.sirens, lib: this.lib } : null,
    );
    this.panel.addSection("siren lab", this.sirenLab.root);
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
      if (this.ctx.state === "suspended" && !this.paused) void this.ctx.resume();
      return;
    }
    try {
      this.lib = audioBridge.library();
    } catch (e) {
      this.failed = true;
      this.panel.setStatus(`audio unavailable: ${e}`);
      return;
    }
    // "balanced" rather than "interactive": a somewhat larger output buffer rides out a busy
    // frame's audio-thread spike instead of dropping out (playtest: the whole mix cut in and
    // out with many cars around). A few ms more latency; impacts still land within a frame.
    this.ctx = new AudioContext({ latencyHint: "balanced" });
    this.mixer = new AudioMixer(this.ctx, this.lib.config.master);
    this.pool = new VoicePool(this.ctx, this.mixer, this.lib);
    this.pool.source = this.panel.settings.source;
    this.sirens = new SirenRack(this.ctx, this.mixer, this.lib);
    this.pool.router = this.sirens;
    this.pool.impacts.enabled = this.panel.settings.impacts !== "recorded";
    this.scanner = new ScannerPlayer(this.pool);
    this.scanner.onSay = (unit, text, tier) => this.onRadioSay?.(unit, text, tier);
    const s = this.panel.settings;
    this.mixer.master.gain.value = this.ducked ? 0 : s.master;
    for (const [b, v] of Object.entries(s.buses)) this.mixer.setUserGain(b as Bus, v);
    this.scanner.tts = s.tts;
    this.updateVoiceVolume();
    this.setTier(s.tier);
    this.setRadioImportantOnly(this.importantOnly);
    // Forget anything the director thinks is playing: it re-plays every loop next frame.
    this.resetDirector();
    this.restartAmbience();
    this.startEngineSynth();
    this.panel.setStatus(`${this.ctx.sampleRate} Hz · max ${this.lib.config.max_voices} voices`);
  }

  /**
   * Load the engine synthesizer worklet; once it runs, the director voices the focus car and
   * the nearest cars with the modelled engine (if the panel says "modelled"). On failure every
   * car stays on layers.
   */
  private startEngineSynth(): void {
    const ctx = this.ctx;
    if (!ctx || this.engineHostState !== "off") return;
    this.engineHostState = "loading";
    EngineSynthHost.create(ctx)
      .then((host) => {
        this.engineHost = host;
        this.engineHostState = "ready";
        if (this.pool) this.pool.engineHost = host;
        this.applyEngineVoicing();
      })
      .catch((e) => {
        console.warn("engine synthesizer unavailable, using layers:", e);
        this.engineHostState = "failed";
        this.applyEngineVoicing();
      });
  }

  /** Modelled engines (default) or layers for every car. */
  setEngineVoicing(v: EngineVoicing): void {
    this.panel.settings.engine = v;
    this.applyEngineVoicing();
  }

  /** Current engine voicing actually in use. */
  engineVoicing(): EngineVoicing {
    return this.engineHost && this.panel.settings.engine === "modelled" ? "modelled" : "layers";
  }

  private applyEngineVoicing(): void {
    this.safe(() => audioBridge.setEngineModel(this.engineVoicing() === "modelled" ? "Physical" : "Layers"));
  }

  /** Engine synthesizer state (e2e / panel). */
  engineSynth(): { state: string; playing: number; acquired: number; refused: number } {
    const h = this.engineHost;
    return { state: this.engineHostState, playing: h?.playing ?? 0, acquired: h?.stats.acquired ?? 0, refused: h?.stats.refused ?? 0 };
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

  /** Impacts & horns: modelled synthesis on the recordings ("synth", default) or recordings only. */
  setImpacts(mode: "synth" | "recorded"): void {
    this.panel.settings.impacts = mode;
    if (!this.pool) return;
    this.pool.impacts.enabled = mode !== "recorded";
    // Held loops (horn, scrapes) restart on their new path.
    this.resetDirector();
  }

  /** Impact voices built recently (`wall` = performance.now()), for latency checks. */
  impactLog(): unknown[] {
    return this.pool?.impacts.log ?? [];
  }

  /** `performance.now()` of recent frames whose events held a `Contact` (latency checks). */
  readonly contactFrames: number[] = [];

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
      if (!this.muted && !this.ducked) this.mixer.master.gain.setTargetAtTime(v.master, this.ctx.currentTime, 0.02);
      this.mixer.setUserGain("Engine", v.engine);
      this.mixer.setUserGain("World", v.effects);
      this.mixer.setUserGain("Police", v.police);
    }
    this.updateVoiceVolume();
    if (ambienceChanged) this.restartAmbience();
  }

  /**
   * Spoken radio lines follow master × Police bus × the Police bus's headroom trim
   * (speechSynthesis bypasses the mixer; the trim keeps the voice where it sat against the mix).
   */
  private updateVoiceVolume(): void {
    if (!this.scanner) return;
    const s = this.panel.settings;
    const trim = 10 ** ((this.mixer?.settings.bus_gain_db.Police ?? 0) / 20);
    this.scanner.volume = this.muted ? 0 : s.master * s.buses.Police * trim;
  }

  /** "Police radio voice" setting: speak radio lines (on) or squelch + text only (off). */
  setRadioVoice(on: boolean): void {
    this.panel.setTts(on);
    if (this.scanner) this.scanner.tts = on;
  }

  /** Whether radio lines are spoken. */
  radioVoice(): boolean {
    return this.panel.settings.tts;
  }

  /** The scanner hears the police radio now (audio running, a scanner tier): lines reach the
   * radio strip through RadioSay, in sync with the voice. Otherwise the strip shows police
   * events directly. */
  hearsRadio(): boolean {
    return !this.failed && this.ctx?.state === "running" && !!this.scanner && this.tier !== "None";
  }

  /** speechSynthesis.speak calls so far (e2e) and whether speech exists in this browser. */
  speech(): { supported: boolean; attempts: number; voice: boolean } {
    return { supported: ScannerPlayer.speechSupported(), attempts: this.scanner?.speechAttempts ?? 0, voice: this.radioVoice() };
  }

  /** Current player volumes (audio panel settings). */
  mix(): { master: number; engine: number; effects: number; police: number; ambience: number } {
    const s = this.panel.settings;
    return { master: s.master, engine: s.buses.Engine, effects: s.buses.World, police: s.buses.Police, ambience: s.ambience };
  }

  toggleMute(): void {
    this.muted = !this.muted;
    this.updateVoiceVolume();
    this.applyMaster();
    this.panel.setStatus(this.muted ? "muted" : "unmuted");
  }

  /**
   * Silence everything while the game is paused: suspends the AudioContext (looped engine /
   * siren voices freeze instead of droning on) and pauses scanner TTS. Idempotent, so main.ts
   * calls it every frame with `app.paused`.
   */
  setPaused(p: boolean): void {
    if (p === this.paused) return;
    this.paused = p;
    if (typeof speechSynthesis !== "undefined") {
      if (p) speechSynthesis.pause();
      else speechSynthesis.resume();
    }
    if (!this.ctx) return;
    if (p) void this.ctx.suspend();
    else void this.ctx.resume();
  }

  /** Fade the whole mix out (`true`) or back in, without touching the mute toggle. */
  setDucked(on: boolean): void {
    this.ducked = on;
    this.applyMaster();
  }

  private applyMaster(): void {
    if (this.mixer && this.ctx) {
      const v = this.muted || this.ducked ? 0 : this.panel.settings.master;
      this.mixer.master.gain.setTargetAtTime(v, this.ctx.currentTime, 0.02);
    }
  }

  /** Siren mode for the focused car: "Default" | "Off" | "Auto" | "Wail" | "Yelp" | "Phaser" | "HiLo". */
  setSiren(mode: string): void {
    this.safe(() => audioBridge.setSiren(this.focus, mode === "Default" ? "" : mode));
  }

  /** Siren mode for any entity (siren lab / e2e): same values as `setSiren`. */
  setSirenFor(entity: number, mode: string): void {
    this.safe(() => audioBridge.setSiren(entity, mode === "Default" ? "" : mode));
  }

  /** "Radio: Important only" (true: police tiers 1–3) or "All" (false). Game menu, row 144. */
  setRadioImportantOnly(on: boolean): void {
    this.importantOnly = on;
    this.safe(() => audioBridge.setRadioImportantOnly(on));
  }

  /** Whether only important radio (tiers 1–3) is played. */
  radioImportantOnly(): boolean {
    return this.importantOnly;
  }

  /** Scanner tier: "None" | "Basic" | "Pro" | "Elite" (wp_police tiers 0/1/2). */
  setTier(tier: string): void {
    this.tier = tier;
    if (this.panel.settings.tier !== tier) {
      this.panel.settings.tier = tier;
      this.panel.save();
    }
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
  update(camera: THREE.Camera, events: unknown[] | string, dt: number, focus: number, cameraMode?: string): void {
    this.focus = focus;
    if (cameraMode !== undefined) this.cameraMode = cameraMode;
    const ctx = this.ctx;
    if (!ctx || ctx.state !== "running" || !this.pool || !this.mixer || !this.scanner) return;
    // Camera → sim world space (the scene's world group is rotated Z-up → Y-up).
    // Scratch vectors (no allocation per frame).
    const [camPos, pos, fwdW, upW, fwd, up, v] = this.scratch;
    // getWorldPosition / getWorldDirection / getWorldQuaternion and three worldToLocal, with
    // the world matrices (and the camera's inverse) updated once: the same arithmetic.
    camera.updateWorldMatrix(true, false);
    const cw = camera.matrixWorld;
    camPos.setFromMatrixPosition(cw);
    this.worldFrame.updateWorldMatrix(true, false);
    const toLocal = this.scratchM.copy(this.worldFrame.matrixWorld).invert();
    pos.copy(camPos).applyMatrix4(toLocal);
    // Camera.getWorldDirection: -(z column), normalised.
    const e = cw.elements;
    fwdW.set(e[8], e[9], e[10]).normalize().negate();
    cw.decompose(this.scratchP, this.scratchQ, this.scratchS);
    upW.set(0, 1, 0).applyQuaternion(this.scratchQ);
    fwd.copy(camPos).add(fwdW).applyMatrix4(toLocal).sub(pos).normalize();
    up.copy(camPos).add(upW).applyMatrix4(toLocal).sub(pos).normalize();
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
      if (json.includes('"Contact"')) {
        this.contactFrames.push(t0);
        if (this.contactFrames.length > 64) this.contactFrames.shift();
      }
      out = audioBridge.frame(a, json, focus);
    } catch (e) {
      this.failed = true;
      this.panel.setStatus(`audio stopped: ${e}`);
      this.pool.stopAll();
      return;
    }
    const t1 = performance.now();
    this.directorMs += (t1 - t0 - this.directorMs) * 0.1;
    this.sirens?.frame(out.siren);
    this.apply(out.commands);
    const t2 = performance.now();
    this.applyMs += (t2 - t1 - this.applyMs) * 0.1;
    this.engineHost?.setInterior(focus, INTERIOR_BY_CAMERA[this.cameraMode] ?? 0);
    this.pool.collect();
    this.frameMs += (performance.now() - t0 - this.frameMs) * 0.1;
    this.frames++;
    this.lastStats = out.stats;
    // The panel shows these only while open (it returns early otherwise): skip building the
    // sample status (a [url, state] pair per sample) and the engine line every frame.
    if (this.panel.settings.open) {
      const eng = this.engineHost ? `engines modelled ${this.engineHost.playing}` : `engine synth ${this.engineHostState}`;
      this.panel.update(out.stats, this.pool.size, this.scanner.log, this.pool.sampleStatus(), eng);
    }
  }

  private apply(cmds: AudioCommand[]): void {
    const pool = this.pool!;
    for (const c of cmds) {
      if ("Play" in c) {
        const p = c.Play;
        this.cueCounts[p.cue] = (this.cueCounts[p.cue] ?? 0) + 1;
        const sr = this.sirens;
        sr?.noteVoice(p.voice, p.pack);
        pool.play(p.voice, p.bus, p.pack, p.cue, p.looped, sr ? sr.adjust(p.voice, p.params) : p.params, p.synth);
      } else if ("Set" in c) {
        pool.set(c.Set.voice, this.sirens ? this.sirens.adjust(c.Set.voice, c.Set.params) : c.Set.params);
      } else if ("Stop" in c) {
        this.sirens?.forget(c.Stop.voice);
        pool.stop(c.Stop.voice, c.Stop.fade_s);
      } else if ("Duck" in c) {
        this.mixer!.duck(c.Duck.bus, c.Duck.gain, c.Duck.fade_s);
      } else if ("RadioSay" in c) {
        const r = c.RadioSay;
        this.scanner!.say(r.line, r.unit, r.text, r.priority, r.voice_pitch, r.tier ?? 0);
        this.sirens?.setRadio(true);
      } else if ("RadioEnd" in c) {
        this.scanner!.end(c.RadioEnd.line, c.RadioEnd.interrupted);
        this.sirens?.setRadio(false);
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
