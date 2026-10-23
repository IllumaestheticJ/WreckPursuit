// Instant crash replay (lead, owner request row 70). See docs/integration/crash-replay.md.
//
// wp_wasm keeps a rolling snapshot buffer of the live session and detects big crashes of the
// player's car (`crash_take`). This director turns a crash into:
//   pending (0.5 s of live play) → offer "Replay · Skip" (3 s; or straight to the replay when
//   "auto crash replay" is on) → replay → live play resumes exactly where it was.
// The replay is a separate wasm session (`crash_replay_start`): the live session is parked,
// not stepped, so its tick and state hash are the same afterwards (checked on every exit).
//
// Replay presentation: impact − 3 s … impact + 2 s, the impact moment at 0.25× with eased
// ramps, a subtle letterbox, and three cameras picked by the hit side (chase from behind, a
// low trackside spot, an orbit around the wreck). Skippable any time.
import * as THREE from "three";
import type { CrashMark, CrashReplayInfo } from "./wasm/sim";
import { store } from "./settings";

const AUTO_KEY = "wp.crashReplay.auto.v1";
const TICK_HZ = 240;
/** Live time between the crash and the offer, s. */
export const OFFER_DELAY_S = 0.5;
/** How long the offer stays up, s (live time; it waits while the game is paused). */
export const OFFER_S = 3.0;
/** Slowest playback (the impact moment). */
export const SLOW_SPEED = 0.25;

let autoReplay = store.get(AUTO_KEY) === "1";

/** Settings hook (pause menu, machine M): play crash replays without asking. Default off
 * (the game offers the replay). Persisted per viewer. */
export function setAutoCrashReplay(on: boolean): void {
  autoReplay = on;
  store.set(AUTO_KEY, on ? "1" : "0");
  for (const cb of listeners) cb(on);
}

/** Current "auto crash replay" setting. */
export const autoCrashReplay = (): boolean => autoReplay;

const listeners: ((on: boolean) => void)[] = [];
/** Called whenever the setting changes (keeps a checkbox in sync). */
export function onAutoCrashReplayChange(cb: (on: boolean) => void): void {
  listeners.push(cb);
}

/** Which crashes offer a replay (feature F43, Settings → Crash replays): none, only big ones
 * (default; see [`isBigCrash`]), or every crash the sim detects. Persisted per viewer. */
export type CrashReplayOffer = "off" | "big" | "all";
const OFFER_KEY = "wp.crashReplay.offer.v1";
const OFFERS: readonly CrashReplayOffer[] = ["off", "big", "all"];
const readOffer = (): CrashReplayOffer => {
  const v = store.get(OFFER_KEY) as CrashReplayOffer | null;
  return v && OFFERS.includes(v) ? v : "big";
};
let offerMode: CrashReplayOffer = readOffer();
const offerListeners: ((m: CrashReplayOffer) => void)[] = [];

/** Settings hook: which crashes offer a replay (persisted). */
export function setCrashReplayOffer(mode: CrashReplayOffer): void {
  if (!OFFERS.includes(mode)) return;
  offerMode = mode;
  store.set(OFFER_KEY, mode);
  for (const cb of offerListeners) cb(mode);
}

/** Current "crash replays" setting. */
export const crashReplayOffer = (): CrashReplayOffer => offerMode;

/** Called whenever the "crash replays" setting changes (keeps a selector in sync). */
export function onCrashReplayOfferChange(cb: (m: CrashReplayOffer) => void): void {
  offerListeners.push(cb);
}

/** "Big crashes only": a crash at least this fast (closing speed), m/s (~50 km/h; the sim's
 * own detector starts at 7 m/s, so routine chase bumps no longer pop the offer). */
export const BIG_CLOSING_MPS = 14;
/** …or with at least this much speed change, m/s. */
export const BIG_DELTA_V_MPS = 8;
/** …or that cost at least this much health. */
export const BIG_HEALTH_DROP = 0.15;

/** Is this detected crash "big" (worth offering under the default setting)? */
export function isBigCrash(m: Pick<CrashMark, "closing_mps" | "delta_v_mps" | "health_drop">): boolean {
  return m.closing_mps >= BIG_CLOSING_MPS || m.delta_v_mps >= BIG_DELTA_V_MPS || m.health_drop >= BIG_HEALTH_DROP;
}

/** Does the current setting offer a replay of `m`? */
export function offersReplay(m: Pick<CrashMark, "closing_mps" | "delta_v_mps" | "health_drop">, mode: CrashReplayOffer = offerMode): boolean {
  return mode === "all" || (mode === "big" && isBigCrash(m));
}

const smooth = (a: number, b: number, x: number) => {
  const t = Math.min(Math.max((x - a) / (b - a), 0), 1);
  return t * t * (3 - 2 * t);
};

/** Playback speed at `d` seconds from the impact: 1× → 0.25× (eased) around the impact. */
export function replaySpeedAt(d: number): number {
  const slowIn = smooth(-1.1, -0.3, d); // 0 → 1 approaching the impact
  const slowOut = 1 - smooth(0.5, 1.3, d); // 1 → 0 after it
  const slow = Math.min(slowIn, slowOut);
  return 1 + (SLOW_SPEED - 1) * slow;
}

type Shot = "chase" | "trackside" | "orbit";

/** Shot order by hit side: frontal / rear hits are seen side-on from the trackside spot; a
 * side hit is circled first, then seen from the far side. */
function shotsFor(side: CrashMark["side"]): [Shot, Shot, Shot] {
  return side === "left" || side === "right" ? ["chase", "orbit", "trackside"] : ["chase", "trackside", "orbit"];
}

/** What the director needs from the app. */
export interface CrashReplayHooks {
  /** May a crash be offered right now (live, not a bench, no garage / panel open)? */
  allowed(): boolean;
  /** The replay session was installed (views must be rebuilt) or removed (likewise). */
  sessionSwapped(entering: boolean): void;
  /** Player car root (Three object) in the current session. */
  playerRoot(): THREE.Object3D | null;
}

/** Result of the last replay (automation: the live session must be unchanged). */
export interface CrashReplayResult {
  seq: number;
  before: { tick: number; hash: string };
  after: { tick: number; hash: string } | null;
  same: boolean;
  /** Replay tick hashes compared with the live log / mismatches. */
  verified: number;
  drift: number;
  skipped: boolean;
}

type Phase = "idle" | "pending" | "offer" | "playing";

export class CrashReplayDirector {
  phase: Phase = "idle";
  /** Crash being offered / played. */
  mark: CrashMark | null = null;
  info: CrashReplayInfo | null = null;
  last: CrashReplayResult | null = null;
  /** Crashes seen this session (automation). */
  crashes = 0;
  /** Crashes the "crash replays" setting did not offer (e2e reads it). */
  skipped = 0;
  private readonly offerEl: HTMLDivElement;
  private readonly barEl: HTMLDivElement;
  private readonly boxEl: HTMLDivElement;
  private readonly tagEl: HTMLDivElement;
  private shot: Shot | null = null;
  private orbitAngle = 0;
  private readonly eye = new THREE.Vector3();
  private readonly look = new THREE.Vector3();
  private haveCam = false;
  private trackSpot = new THREE.Vector3();
  private impact = new THREE.Vector3();
  private hitDir = new THREE.Vector3();

  constructor(
    parent: HTMLElement,
    private readonly overlay: HTMLElement,
    private readonly sim: {
      crashReplayStart(seq: number): CrashReplayInfo;
      crashReplayStop(): { tick: number; hash: string } | null;
      crashReplayActive(): boolean;
      replayPlay(speed: number): void;
      replayStatus(): { tick: number; end_tick: number; verified_ticks: number; drift_count: number } | null;
    },
    private readonly hooks: CrashReplayHooks,
    private readonly keyLabel: () => string,
  ) {
    const css = document.createElement("style");
    css.textContent = `
      #crash-offer { position: absolute; left: 50%; bottom: 14vh; transform: translateX(-50%); z-index: 40;
        background: rgba(10,12,16,0.82); color: #f2f2f2; border: 1px solid rgba(255,255,255,0.18);
        border-radius: 8px; padding: 10px 14px 8px; font: 600 15px/1.3 system-ui, sans-serif; display: none;
        pointer-events: auto; text-align: center; min-width: 260px; flex-direction: column; }
      #crash-offer .t { font-size: 12px; font-weight: 500; opacity: 0.75; margin-bottom: 6px; letter-spacing: 0.08em; }
      #crash-offer button { font: inherit; margin: 0 4px; padding: 4px 12px; border-radius: 5px; cursor: pointer;
        border: 1px solid rgba(255,255,255,0.3); background: rgba(255,255,255,0.08); color: inherit; }
      #crash-offer button.go { background: #d04a2a; border-color: #d04a2a; }
      #crash-offer .row { display: flex; justify-content: center; }
      #crash-offer .bar { display: block; width: 100%; height: 3px; margin-top: 8px; background: rgba(255,255,255,0.15); border-radius: 2px; overflow: hidden; }
      #crash-offer .bar > div { height: 100%; background: #f2f2f2; width: 100%; }
      #crash-letterbox { position: fixed; inset: 0; pointer-events: none; z-index: 30; display: none; }
      #crash-letterbox::before, #crash-letterbox::after { content: ""; position: absolute; left: 0; right: 0;
        height: 0; background: #000; transition: height 0.35s ease; }
      #crash-letterbox::before { top: 0; }
      #crash-letterbox::after { bottom: 0; }
      #crash-letterbox.on::before, #crash-letterbox.on::after { height: 8.5vh; }
      #crash-letterbox .tag { position: absolute; z-index: 1; color: #eee; font: 600 13px/1 system-ui, sans-serif;
        letter-spacing: 0.18em; opacity: 0; transition: opacity 0.35s ease; }
      #crash-letterbox.on .tag { opacity: 0.85; }
      #crash-letterbox .tag.l { top: 3.4vh; left: 4vw; }
      #crash-letterbox .tag.r { bottom: 3.4vh; right: 4vw; letter-spacing: 0.05em; font-weight: 500; }
    `;
    document.head.appendChild(css);
    this.offerEl = document.createElement("div");
    this.offerEl.id = "crash-offer";
    this.offerEl.innerHTML = `<div class="t">BIG CRASH</div><div class="row"><button class="go"></button><button class="skip">Skip</button></div><div class="bar"><div></div></div>`;
    this.barEl = this.offerEl.querySelector(".bar > div") as HTMLDivElement;
    (this.offerEl.querySelector(".go") as HTMLButtonElement).onclick = () => this.accept();
    (this.offerEl.querySelector(".skip") as HTMLButtonElement).onclick = () => this.dismiss();
    parent.appendChild(this.offerEl);
    this.boxEl = document.createElement("div");
    this.boxEl.id = "crash-letterbox";
    this.boxEl.innerHTML = `<div class="tag l">REPLAY</div><div class="tag r"></div>`;
    this.tagEl = this.boxEl.querySelector(".tag.r") as HTMLDivElement;
    document.body.appendChild(this.boxEl);
  }

  /** A new session started (or the live one was replaced): forget everything. */
  reset(): void {
    if (this.phase === "playing") this.leave(true, null);
    this.phase = "idle";
    this.mark = null;
    this.hideOffer();
  }

  /** A crash reported by `crash_take` (live session). */
  onCrash(mark: CrashMark): void {
    this.crashes++;
    if (this.phase === "playing") return;
    // Settings → Crash replays (F43): only the crashes the player wants offered.
    if (!offersReplay(mark)) {
      this.skipped++;
      return;
    }
    this.mark = mark;
    this.phase = "pending";
    this.hideOffer();
  }

  /** Per frame while live: `tick` = live tick. Shows / expires the offer, starts auto replays. */
  liveFrame(tick: number): void {
    if (!this.mark || (this.phase !== "pending" && this.phase !== "offer")) return;
    if (!this.hooks.allowed()) {
      if (this.phase === "offer") this.dismiss();
      return;
    }
    const since = (tick - this.mark.tick) / TICK_HZ;
    if (this.phase === "pending" && since >= OFFER_DELAY_S) {
      if (autoReplay) return void this.accept();
      this.phase = "offer";
      (this.offerEl.querySelector(".go") as HTMLButtonElement).textContent = `Replay (${this.keyLabel()})`;
      this.offerEl.style.display = "flex";
    }
    if (this.phase === "offer") {
      const left = 1 - (since - OFFER_DELAY_S) / OFFER_S;
      if (left <= 0) return this.dismiss();
      this.barEl.style.width = `${(left * 100).toFixed(1)}%`;
    }
  }

  /** The `replayCrash` action (key / pad): accept the offer, or skip a playing replay. */
  action(): boolean {
    if (this.phase === "offer") {
      this.accept();
      return true;
    }
    if (this.phase === "playing") {
      this.skip();
      return true;
    }
    return false;
  }

  /** Accept the offer: park the live session and play the crash. */
  accept(): void {
    if (!this.mark || this.phase === "playing") return;
    this.hideOffer();
    try {
      this.info = this.sim.crashReplayStart(this.mark.seq);
    } catch (e) {
      console.warn("crash replay unavailable", e);
      this.phase = "idle";
      return;
    }
    this.phase = "playing";
    this.shot = null;
    this.haveCam = false;
    this.overlay.style.visibility = "hidden";
    this.boxEl.style.display = "block";
    requestAnimationFrame(() => this.boxEl.classList.add("on"));
    this.tagEl.textContent = `${this.keyLabel()} · skip`;
    this.hooks.sessionSwapped(true);
    this.sim.replayPlay(1);
  }

  /** Close the offer without playing. */
  dismiss(): void {
    this.hideOffer();
    if (this.phase !== "playing") this.phase = "idle";
  }

  /** Skip the rest of a playing replay. */
  skip(): void {
    if (this.phase === "playing") this.leave(true, this.sim.replayStatus());
  }

  /** Before the replay session steps: set the speed for this frame (`paused` = game paused). */
  beforeStep(paused: boolean): void {
    if (this.phase !== "playing" || !this.info) return;
    const st = this.sim.replayStatus();
    if (!st) return;
    const d = (st.tick - this.info.crash.tick) / TICK_HZ;
    this.sim.replayPlay(paused ? 0 : replaySpeedAt(d));
  }

  /** After the step: leave at the end of the window. */
  afterStep(): void {
    if (this.phase !== "playing" || !this.info) return;
    const st = this.sim.replayStatus();
    if (!st || st.tick >= this.info.end_tick) this.leave(false, st);
  }

  /** Is the director driving the camera this frame? */
  get playing(): boolean {
    return this.phase === "playing";
  }

  /** Drive the camera during the replay (Three world space). `tick` = replay tick, `dt` wall s. */
  camera(cam: THREE.PerspectiveCamera, tick: number, dt: number): void {
    const info = this.info;
    const root = this.hooks.playerRoot();
    if (!info || !root) return;
    root.updateWorldMatrix(true, false);
    const car = root.getWorldPosition(new THREE.Vector3());
    const fwd = new THREE.Vector3(0, 1, 0).applyQuaternion(root.getWorldQuaternion(new THREE.Quaternion())).setY(0);
    if (fwd.lengthSq() < 1e-6) fwd.set(0, 0, 1);
    fwd.normalize();
    const d = (tick - info.crash.tick) / TICK_HZ;
    const [a, b, c] = shotsFor(info.crash.side);
    const shot: Shot = d < -0.8 ? a : d < 0.8 ? b : c;
    const p = info.crash.point;
    // Sim (x, y, z) → Three (x, z, -y).
    this.impact.set(p[0], p[2], -p[1]);
    if (shot !== this.shot) {
      // A cut: place the new shot's fixed parts.
      this.shot = shot;
      this.haveCam = false;
      const side = new THREE.Vector3().crossVectors(fwd, new THREE.Vector3(0, 1, 0)).normalize();
      // Hit direction in world (car centre → contact), horizontal.
      this.hitDir.copy(this.impact).sub(car).setY(0);
      if (this.hitDir.lengthSq() < 1e-4) this.hitDir.copy(fwd);
      this.hitDir.normalize();
      const ground = Math.min(car.y, this.impact.y) - 0.3;
      if (info.crash.side === "left" || info.crash.side === "right") {
        // Opposite the struck flank, a little ahead: the wreck slides toward the camera.
        this.trackSpot.copy(car).addScaledVector(this.hitDir, -11).addScaledVector(fwd, 4);
      } else {
        this.trackSpot.copy(this.impact).addScaledVector(side, 10).addScaledVector(fwd, info.crash.side === "rear" ? -3 : 3);
      }
      this.trackSpot.y = ground + 0.55;
      this.orbitAngle = Math.atan2(-fwd.x, -fwd.z) + Math.PI * 0.75;
    }
    const eye = new THREE.Vector3();
    const look = new THREE.Vector3();
    let fov = 55;
    switch (shot) {
      case "chase": {
        eye.copy(car).addScaledVector(fwd, -7.5);
        eye.y += 2.4;
        look.copy(car).addScaledVector(fwd, 4);
        look.y += 0.6;
        fov = 58;
        break;
      }
      case "trackside": {
        eye.copy(this.trackSpot);
        look.copy(car);
        look.y += 0.3;
        fov = 38;
        break;
      }
      case "orbit": {
        // Wall-clock rotation: smooth in slow motion too.
        this.orbitAngle += 0.35 * dt;
        const centre = car.clone().lerp(this.impact, 0.3);
        eye.set(centre.x + Math.sin(this.orbitAngle) * 8.5, centre.y + 2.6, centre.z + Math.cos(this.orbitAngle) * 8.5);
        look.copy(centre);
        fov = 52;
        break;
      }
    }
    if (!this.haveCam) {
      this.eye.copy(eye);
      this.look.copy(look);
      this.haveCam = true;
    } else {
      // Follow smoothly (chase / orbit); the trackside spot stays put and pans.
      const k = 1 - Math.exp(-(shot === "trackside" ? 30 : 6) * dt);
      this.eye.lerp(eye, shot === "trackside" ? 1 : k);
      this.look.lerp(look, Math.min(1, k * 1.5));
    }
    cam.position.copy(this.eye);
    cam.lookAt(this.look);
    if (Math.abs(cam.fov - fov) > 0.01) {
      cam.fov = fov;
      cam.updateProjectionMatrix();
    }
  }

  private hideOffer(): void {
    this.offerEl.style.display = "none";
  }

  /** Back to live play; records whether the live session is unchanged. */
  private leave(skipped: boolean, st: { verified_ticks: number; drift_count: number } | null): void {
    const info = this.info;
    const after = this.sim.crashReplayActive() ? this.sim.crashReplayStop() : null;
    if (info && this.mark) {
      const before = { tick: info.live_tick, hash: info.live_hash };
      this.last = {
        seq: this.mark.seq,
        before,
        after,
        same: !!after && after.tick === before.tick && after.hash === before.hash,
        verified: st?.verified_ticks ?? 0,
        drift: st?.drift_count ?? 0,
        skipped,
      };
      if (!this.last.same) console.error("crash replay changed the live session", this.last);
    }
    this.phase = "idle";
    this.mark = null;
    this.info = null;
    this.shot = null;
    this.overlay.style.visibility = "";
    this.boxEl.classList.remove("on");
    this.boxEl.style.display = "none";
    if (after) this.hooks.sessionSwapped(false);
  }
}
