// On-screen touch controls for phones and tablets (owner request, row 71; docs/integration/touch.md).
//
// Shown only on touch devices: a coarse primary pointer, or the first touch on the page
// (`?touch=1` forces them on, `?touch=0` off). Layout (landscape): steering bottom-left (two
// buttons; the finger's position across them is the analogue value, so sliding inwards steers
// less) or tilt-to-steer; brake and throttle pedals bottom-right (analogue by the finger's
// height on the pedal: low = light, high = full; slide to modulate); handbrake next to the
// pedals; map / pause top-right; camera / reset / horn / steering mode above the steering
// (the horn also challenges a street racer next to you, src/ui/street.ts). Portrait shows a
// "rotate your phone" hint.
//
// Values reach the sim through `input.ts` exactly like the gamepad's (registered with
// `setTouchReader`; no keyboard ramps, so the sim's analogue assist filters apply); main applies
// the touch assist defaults (auto gears, TCS, steer help) when the source becomes "Touch".
// Everything held is released on blur / tab hidden / pointercancel, like the keyboard.
import type { ActionId } from "./bindings";
import { setTouchReader, type AnalogRead } from "./input";
import type { DriverInput } from "./wasm/sim";

/** How the touch player steers. */
export type TouchSteerMode = "buttons" | "tilt";

const STORE_KEY = "wp.touch.v1";
/** Tilt angle (rad) for full lock, and the dead band around level. */
const TILT_FULL = (32 * Math.PI) / 180;
const TILT_DEAD = (3 * Math.PI) / 180;
/** Steering slew (full lock per second) so a tap on a button is not a 0 → 1 jump. */
const STEER_RATE = 8;
/** Lowest pedal value of a touch at the very bottom of a pedal. */
const PEDAL_MIN = 0.3;
/** No orientation events this long after enabling tilt → back to buttons (blocked in embeds). */
const TILT_TIMEOUT_MS = 1500;

const params = new URLSearchParams(typeof location !== "undefined" ? location.search : "");

/** Does this device want touch controls from the start? `?touch=1|0` forces it; otherwise a
 * coarse primary pointer (phones, tablets; not touch laptops, whose primary pointer is fine). */
export function prefersTouch(): boolean {
  const p = params.get("touch");
  if (p === "1") return true;
  if (p === "0") return false;
  try {
    return matchMedia("(pointer: coarse)").matches;
  } catch {
    return false;
  }
}

const touchDisabled = (): boolean => params.get("touch") === "0";

function loadMode(): TouchSteerMode {
  try {
    const s = JSON.parse(localStorage.getItem(STORE_KEY) ?? "null") as { steer?: string } | null;
    return s?.steer === "tilt" ? "tilt" : "buttons";
  } catch {
    return "buttons";
  }
}

function saveMode(m: TouchSteerMode): void {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify({ steer: m }));
  } catch {
    // ignore (private window): the choice lasts for the session
  }
}

/** Steering wheel angle (rad, + = clockwise = right) of a device from its orientation, for the
 * current screen rotation: the angle of gravity in the screen plane. Null when the phone is
 * nearly flat (gravity almost along the screen normal, so the angle is noise). */
export function tiltSteerAngle(betaDeg: number, gammaDeg: number, screenAngleDeg: number): number | null {
  const b = (betaDeg * Math.PI) / 180;
  const g = (gammaDeg * Math.PI) / 180;
  // Gravity ("down") in device coordinates for intrinsic Z-X'-Y'' angles (alpha, beta, gamma).
  const dx = Math.sin(g) * Math.cos(b);
  const dy = -Math.sin(b);
  if (Math.hypot(dx, dy) < 0.25) return null;
  const t = (screenAngleDeg * Math.PI) / 180;
  // Screen right / down in device coordinates for a screen rotated by t.
  const right = dx * Math.cos(t) - dy * Math.sin(t);
  const down = -dx * Math.sin(t) - dy * Math.cos(t);
  return Math.atan2(right, down);
}

/** Map a tilt angle to a steering value (dead band, full lock at `TILT_FULL`). */
export function tiltToSteer(angle: number): number {
  const a = Math.abs(angle);
  if (a < TILT_DEAD) return 0;
  return Math.sign(angle) * Math.min(1, (a - TILT_DEAD) / (TILT_FULL - TILT_DEAD));
}

/** Steering value of a finger at `u` (-1 = outer edge of the left button, +1 = outer edge of
 * the right button): small near the middle, full on the outer 40 % of each button. */
export function buttonSteer(u: number): number {
  const a = Math.abs(u);
  return Math.sign(u) * Math.min(1, Math.max(0, (a - 0.06) / 0.54));
}

/** Pedal value of a finger `f` of the way up the pedal (0 bottom .. 1 top). */
export const pedalValue = (f: number): number => PEDAL_MIN + (1 - PEDAL_MIN) * Math.min(1, Math.max(0, f));

const CSS = `
html.wp-touch, html.wp-touch body { touch-action: none; overscroll-behavior: none; -webkit-user-select: none; user-select: none; -webkit-touch-callout: none; }
#wp-touch { position: fixed; inset: 0; z-index: 20; pointer-events: none; font: 600 13px/1 system-ui, sans-serif; color: #fff;
  --sl: env(safe-area-inset-left, 0px); --sr: env(safe-area-inset-right, 0px); --st: env(safe-area-inset-top, 0px); --sb: env(safe-area-inset-bottom, 0px);
  --pad: 12px; --btn: clamp(64px, 24vh, 104px); --small: 44px; }
#wp-touch[hidden] { display: none; }
#wp-touch .tc { position: absolute; pointer-events: auto; touch-action: none; box-sizing: border-box; display: flex; align-items: center; justify-content: center;
  background: rgba(12, 14, 20, 0.42); border: 2px solid rgba(255, 255, 255, 0.28); border-radius: 14px; text-shadow: 0 1px 2px #000; }
#wp-touch .tc.on { background: rgba(255, 255, 255, 0.28); border-color: rgba(255, 255, 255, 0.7); }
#wp-touch .steer { left: calc(var(--sl) + var(--pad)); bottom: calc(var(--sb) + var(--pad)); width: calc(var(--btn) * 2 + 10px); height: var(--btn);
  background: none; border: none; justify-content: space-between; }
#wp-touch .steer i { width: var(--btn); height: 100%; box-sizing: border-box; display: flex; align-items: center; justify-content: center; font-style: normal; font-size: 30px;
  background: rgba(12, 14, 20, 0.42); border: 2px solid rgba(255, 255, 255, 0.28); border-radius: 14px; }
#wp-touch .steer i.on { background: rgba(255, 255, 255, 0.28); border-color: rgba(255, 255, 255, 0.7); }
#wp-touch .steer .fill { position: absolute; left: 50%; bottom: -8px; height: 4px; background: #7fdbff; border-radius: 2px; }
#wp-touch .pedal { bottom: calc(var(--sb) + var(--pad)); width: calc(var(--btn) * 0.8); align-items: flex-end; padding-bottom: 8px; overflow: hidden; }
#wp-touch .pedal .lvl { position: absolute; left: 0; right: 0; bottom: 0; height: 0; opacity: 0.45; }
#wp-touch .pedal span { position: relative; }
#wp-touch .throttle { right: calc(var(--sr) + var(--pad)); height: min(calc(var(--btn) * 1.9), calc(100vh - var(--st) - var(--sb) - var(--small) - var(--pad) * 3)); }
#wp-touch .throttle .lvl { background: #2ecc40; }
#wp-touch .brake { right: calc(var(--sr) + var(--pad) * 2 + var(--btn) * 0.8); height: calc(var(--btn) * 1.35); }
#wp-touch .brake .lvl { background: #ff4136; }
#wp-touch .hbrake { right: calc(var(--sr) + var(--pad) * 3 + var(--btn) * 1.6); bottom: calc(var(--sb) + var(--pad)); width: calc(var(--btn) * 0.72); height: calc(var(--btn) * 0.72); border-radius: 50%; font-size: 12px; }
#wp-touch .row { position: absolute; display: flex; gap: 6px; pointer-events: none; }
#wp-touch .row.top { top: calc(var(--st) + 8px); right: calc(var(--sr) + var(--pad)); }
#wp-touch .row.side { left: calc(var(--sl) + var(--pad)); bottom: calc(var(--sb) + var(--pad) + var(--btn) + 10px); }
#wp-touch .row .tc { position: relative; width: var(--small); height: var(--small); border-radius: 10px; font-size: 18px; }
#wp-touch .row .tc.mode { width: auto; padding: 0 8px; font-size: 12px; }
#wp-touch.no-drive .steer, #wp-touch.no-drive .pedal, #wp-touch.no-drive .hbrake, #wp-touch.no-drive .row.side { display: none; }
#wp-touch.tilt .steer { display: none; }
#wp-touch.covered { display: none; }
#wp-touch .tiltmark { position: absolute; left: calc(var(--sl) + var(--pad)); bottom: calc(var(--sb) + var(--pad)); padding: 6px 10px; border-radius: 10px;
  background: rgba(12, 14, 20, 0.42); pointer-events: none; font-size: 12px; }
#wp-touch:not(.tilt) .tiltmark, #wp-touch.no-drive .tiltmark { display: none; }
#wp-rotate { position: fixed; inset: 0; z-index: 60; display: none; align-items: center; justify-content: center; flex-direction: column; gap: 14px;
  background: rgba(6, 8, 12, 0.93); color: #e6e8ee; font: 600 18px/1.4 system-ui, sans-serif; text-align: center; padding: 24px; pointer-events: auto; }
#wp-rotate b { font-size: 54px; line-height: 1; }
#wp-rotate small { font-weight: 400; font-size: 13px; color: #9aa3b5; }
@media (orientation: portrait) { html.wp-touch:not(.wp-rotate-ok) #wp-rotate { display: flex; } }

/* Small screens (phones, landscape): no dev panels; compact HUD that fits 640x360 .. 932x430. */
@media (max-width: 960px), (max-height: 500px) {
  #controls, .telemetry, .comps, .regions, .feed, .inputs, .silhouette, #bench-out, #audio-panel, #weather-hud { display: none !important; }
  .hud-main { min-width: 0 !important; }
}
html.wp-touch .left-col { top: calc(env(safe-area-inset-top, 0px) + 6px); left: calc(env(safe-area-inset-left, 0px) + 8px); }
html.wp-touch .hud-main { min-width: 0 !important; padding: 4px 8px; }
html.wp-touch .hud-main > div:not(.speed) { display: none; }
html.wp-touch .hud-main .speed { font-size: 24px; line-height: 1.1; }
html.wp-touch .hud-main .speed .gear { font-size: 24px; }
html.wp-touch .right-col, html.wp-touch .help:not(.hidden) ~ .right-col { display: none; }
html.wp-touch .wp-minimap { left: calc(env(safe-area-inset-left, 0px) + 8px) !important; top: calc(env(safe-area-inset-top, 0px) + 50px) !important;
  right: auto !important; bottom: auto !important; width: clamp(84px, 24vh, 128px) !important; font-size: 11px; }
html.wp-touch #pursuit-banner { top: 30%; }
html.wp-touch #pursuit-banner b { font-size: clamp(24px, 8vh, 44px); }
html.wp-touch #pursuit-banner span { font-size: 12px; }
html.wp-touch #audio-panel, html.wp-touch #weather-hud { display: none !important; }
html.wp-touch .wp-chase-col { font-size: 12px; width: 180px; top: calc(env(safe-area-inset-top, 0px) + 6px); }
html.wp-touch .wp-chase-col .wp-hud-alerts { width: 200px; }
html.wp-touch .wp-race-hud, html.wp-touch #street-hud { left: auto; right: calc(env(safe-area-inset-right, 0px) + 12px); top: calc(env(safe-area-inset-top, 0px) + 60px); width: min(240px, 36vw); font-size: 11px; }
`;

interface Finger {
  id: number;
  kind: "steer" | "throttle" | "brake" | "hbrake";
}

/** The touch controls (one instance; created by `installTouch`). */
export class TouchControls {
  readonly root: HTMLElement;
  private readonly steerEl: HTMLElement;
  private readonly steerL: HTMLElement;
  private readonly steerR: HTMLElement;
  private readonly steerFill: HTMLElement;
  private readonly throttleEl: HTMLElement;
  private readonly brakeEl: HTMLElement;
  private readonly hbrakeEl: HTMLElement;
  private readonly modeEl: HTMLElement;
  private readonly fingers = new Map<number, Finger>();
  /** Raw targets from the fingers / tilt. */
  private steerTarget = 0;
  private throttle = 0;
  private brake = 0;
  private hbrake = false;
  /** Horn held (the 📯 button). */
  private horn = false;
  private readonly hornEl: HTMLElement;
  /** Slewed steering sent to the sim. */
  private steer = 0;
  private tiltSteer = 0;
  private tiltSeen = false;
  private readonly edges: ActionId[] = [];
  private touchedThisFrame = false;
  private mode: TouchSteerMode = "buttons";
  private driving = true;
  private shown = false;
  private readonly modeListeners: ((m: TouchSteerMode) => void)[] = [];
  private readonly onOrientation = (e: DeviceOrientationEvent) => {
    if (e.beta === null || e.gamma === null) return;
    this.tiltSeen = true;
    const angle = tiltSteerAngle(e.beta, e.gamma, screenAngle());
    if (angle !== null) this.tiltSteer = tiltToSteer(angle);
  };

  constructor() {
    const style = document.createElement("style");
    style.id = "wp-touch-css";
    style.textContent = CSS;
    document.head.appendChild(style);

    const root = (this.root = document.createElement("div"));
    root.id = "wp-touch";
    root.hidden = true;
    const el = (cls: string, html = "", parent: HTMLElement = root) => {
      const e = document.createElement("div");
      e.className = cls;
      e.innerHTML = html;
      parent.appendChild(e);
      return e;
    };
    this.steerEl = el("tc steer");
    this.steerEl.dataset.touch = "steer";
    this.steerL = document.createElement("i");
    this.steerL.textContent = "◀";
    this.steerR = document.createElement("i");
    this.steerR.textContent = "▶";
    this.steerFill = document.createElement("b");
    this.steerFill.className = "fill";
    this.steerEl.append(this.steerL, this.steerR, this.steerFill);
    el("tiltmark", "tilt to steer");
    this.brakeEl = el("tc pedal brake", `<div class="lvl"></div><span>BRAKE</span>`);
    this.brakeEl.dataset.touch = "brake";
    this.throttleEl = el("tc pedal throttle", `<div class="lvl"></div><span>GAS</span>`);
    this.throttleEl.dataset.touch = "throttle";
    this.hbrakeEl = el("tc hbrake", "HAND<br>BRAKE");
    this.hbrakeEl.style.textAlign = "center";
    this.hbrakeEl.style.lineHeight = "1.15";
    this.hbrakeEl.dataset.touch = "hbrake";
    const top = el("row top");
    const side = el("row side");
    const edgeBtn = (action: ActionId, label: string, title: string, row: HTMLElement) => {
      const b = el("tc", label, row);
      b.dataset.touch = action;
      b.title = title;
      b.addEventListener("pointerdown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.edges.push(action);
        this.touchedThisFrame = true;
        b.classList.add("on");
        setTimeout(() => b.classList.remove("on"), 120);
      });
      return b;
    };
    edgeBtn("cameraCycle", "🎥", "camera", side);
    edgeBtn("reset", "↺", "reset the car", side);
    // Horn (held): also challenges a street racer you are next to.
    this.hornEl = el("tc", "📯", side);
    this.hornEl.dataset.touch = "horn";
    this.hornEl.title = "horn (challenge a street racer)";
    const hornUp = (e: PointerEvent) => {
      if (!this.horn) return;
      e.preventDefault();
      this.setHorn(false);
    };
    this.hornEl.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      try {
        this.hornEl.setPointerCapture(e.pointerId);
      } catch {
        // synthetic events without an active pointer
      }
      this.touchedThisFrame = true;
      this.setHorn(true);
    });
    this.hornEl.addEventListener("pointerup", hornUp);
    this.hornEl.addEventListener("pointercancel", hornUp);
    this.hornEl.addEventListener("lostpointercapture", hornUp);
    this.hornEl.addEventListener("contextmenu", (ev) => ev.preventDefault());
    this.modeEl = el("tc mode", "", side);
    this.modeEl.dataset.touch = "steerMode";
    this.modeEl.title = "steering: buttons or tilt";
    this.modeEl.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      void setTouchSteerMode(this.mode === "tilt" ? "buttons" : "tilt");
    });
    edgeBtn("map", "🗺", "city map", top);
    edgeBtn("pause", "⏸", "pause", top);
    document.body.appendChild(root);

    const rotate = document.createElement("div");
    rotate.id = "wp-rotate";
    rotate.innerHTML = `<b>⟲</b>Rotate your phone<small>WreckPursuit plays in landscape. Tap to play anyway.</small>`;
    rotate.addEventListener("pointerdown", () => document.documentElement.classList.add("wp-rotate-ok"));
    document.body.appendChild(rotate);

    for (const e of [this.steerEl, this.throttleEl, this.brakeEl, this.hbrakeEl]) {
      e.addEventListener("pointerdown", (ev) => this.down(ev, e.dataset.touch as Finger["kind"], e));
      e.addEventListener("pointermove", (ev) => this.move(ev));
      e.addEventListener("pointerup", (ev) => this.up(ev));
      e.addEventListener("pointercancel", (ev) => this.up(ev));
      e.addEventListener("lostpointercapture", (ev) => this.up(ev));
      e.addEventListener("contextmenu", (ev) => ev.preventDefault());
    }
    const releaseAll = () => this.releaseAll();
    window.addEventListener("blur", releaseAll);
    document.addEventListener("visibilitychange", releaseAll);

    this.mode = loadMode();
    this.renderMode();
    if (this.mode === "tilt") void this.enableTilt(false);
  }

  /** Show the controls (idempotent). */
  show(): void {
    if (this.shown) return;
    this.shown = true;
    this.root.hidden = false;
    document.documentElement.classList.add("wp-touch");
  }

  /** Are the controls showing? */
  get visible(): boolean {
    return this.shown;
  }

  /** Current steering mode. */
  get steerMode(): TouchSteerMode {
    return this.mode;
  }

  /** Driving controls on (steer, pedals, handbrake) or only the button row (paused, map open,
   * menus). Turning them off releases everything held. */
  setDriving(on: boolean): void {
    if (on === this.driving) return;
    this.driving = on;
    this.root.classList.toggle("no-drive", !on);
    if (!on) this.releaseAll();
  }

  /** Hide every control (a menu or full-screen panel owns the screen; the touch layer sits
   * above the testbed overlay). Releases everything held. */
  setCovered(on: boolean): void {
    if (on === this.root.classList.contains("covered")) return;
    this.root.classList.toggle("covered", on);
    if (on) this.releaseAll();
  }

  /** Listen for steering-mode changes (settings UI). */
  onModeChange(cb: (m: TouchSteerMode) => void): void {
    this.modeListeners.push(cb);
  }

  /** Switch steering mode. Tilt asks for motion permission where the browser requires it (iOS:
   * call from a tap) and falls back to buttons when denied, unsupported or silent. Returns the
   * mode in effect. */
  async setMode(m: TouchSteerMode): Promise<TouchSteerMode> {
    if (m === "tilt") {
      const ok = await this.enableTilt(true);
      if (!ok) m = "buttons";
    } else {
      window.removeEventListener("deviceorientation", this.onOrientation);
    }
    this.mode = m;
    saveMode(m);
    this.renderMode();
    return m;
  }

  private async enableTilt(ask: boolean): Promise<boolean> {
    const DOE = (window as unknown as { DeviceOrientationEvent?: { requestPermission?: () => Promise<string> } }).DeviceOrientationEvent;
    if (!DOE) return this.fallback();
    if (typeof DOE.requestPermission === "function") {
      if (!ask) return this.fallback();
      try {
        if ((await DOE.requestPermission()) !== "granted") return this.fallback();
      } catch {
        return this.fallback();
      }
    }
    this.tiltSeen = false;
    this.tiltSteer = 0;
    window.addEventListener("deviceorientation", this.onOrientation);
    // Some embeds (cross-origin iframes without the permission policy) never deliver events.
    setTimeout(() => {
      if (this.mode === "tilt" && !this.tiltSeen) void this.setMode("buttons");
    }, TILT_TIMEOUT_MS);
    return true;
  }

  private fallback(): boolean {
    window.removeEventListener("deviceorientation", this.onOrientation);
    if (this.mode === "tilt") {
      this.mode = "buttons";
      saveMode("buttons");
      this.renderMode();
    }
    return false;
  }

  private renderMode(): void {
    this.root.classList.toggle("tilt", this.mode === "tilt");
    this.modeEl.textContent = this.mode === "tilt" ? "STEER TILT" : "STEER ◀▶";
    for (const cb of this.modeListeners) cb(this.mode);
  }

  private down(e: PointerEvent, kind: Finger["kind"], target: HTMLElement): void {
    e.preventDefault();
    e.stopPropagation();
    try {
      target.setPointerCapture(e.pointerId);
    } catch {
      // synthetic events without an active pointer
    }
    this.fingers.set(e.pointerId, { id: e.pointerId, kind });
    this.touchedThisFrame = true;
    this.apply(e, kind);
  }

  private move(e: PointerEvent): void {
    const f = this.fingers.get(e.pointerId);
    if (!f) return;
    e.preventDefault();
    this.apply(e, f.kind);
  }

  private up(e: PointerEvent): void {
    const f = this.fingers.get(e.pointerId);
    if (!f) return;
    this.fingers.delete(e.pointerId);
    if (![...this.fingers.values()].some((o) => o.kind === f.kind)) this.set(f.kind, 0);
  }

  private apply(e: PointerEvent, kind: Finger["kind"]): void {
    if (kind === "steer") {
      const r = this.steerEl.getBoundingClientRect();
      this.set("steer", buttonSteer(((e.clientX - (r.left + r.width / 2)) / (r.width / 2)) || 0));
    } else if (kind === "hbrake") {
      this.set("hbrake", 1);
    } else {
      const r = (kind === "throttle" ? this.throttleEl : this.brakeEl).getBoundingClientRect();
      this.set(kind, pedalValue((r.bottom - e.clientY) / Math.max(1, r.height)));
    }
  }

  private set(kind: Finger["kind"], v: number): void {
    if (kind === "steer") {
      this.steerTarget = v;
      this.steerL.classList.toggle("on", v < 0);
      this.steerR.classList.toggle("on", v > 0);
    } else if (kind === "hbrake") {
      this.hbrake = v > 0;
      this.hbrakeEl.classList.toggle("on", this.hbrake);
    } else {
      if (kind === "throttle") this.throttle = v;
      else this.brake = v;
      const el = kind === "throttle" ? this.throttleEl : this.brakeEl;
      el.classList.toggle("on", v > 0);
      (el.firstElementChild as HTMLElement).style.height = `${(v * 100).toFixed(0)}%`;
    }
  }

  private setHorn(on: boolean): void {
    this.horn = on;
    this.hornEl.classList.toggle("on", on);
  }

  /** Let go of everything (blur, tab hidden, controls hidden). */
  releaseAll(): void {
    this.setHorn(false);
    for (const id of this.fingers.keys()) {
      for (const e of [this.steerEl, this.throttleEl, this.brakeEl, this.hbrakeEl]) {
        try {
          if (e.hasPointerCapture(id)) e.releasePointerCapture(id);
        } catch {
          // ignore
        }
      }
    }
    this.fingers.clear();
    for (const k of ["steer", "throttle", "brake", "hbrake"] as const) this.set(k, 0);
    this.steer = 0;
  }

  /** One frame's reading (registered with input.ts). Null while the controls are hidden. */
  read(dt: number): AnalogRead | null {
    if (!this.shown) return null;
    const target = this.mode === "tilt" ? (this.driving ? this.tiltSteer : 0) : this.steerTarget;
    const step = STEER_RATE * Math.max(0, dt);
    const d = target - this.steer;
    this.steer = Math.abs(d) <= step ? target : this.steer + Math.sign(d) * step;
    const r = this.steerFill.style;
    r.width = `${(Math.abs(this.steer) * 50).toFixed(0)}%`;
    r.left = this.steer < 0 ? `${(50 - Math.abs(this.steer) * 50).toFixed(0)}%` : "50%";
    const input: DriverInput = {
      steer: this.steer,
      throttle: this.throttle,
      brake: this.brake,
      handbrake: this.hbrake ? 1 : 0,
      clutch: 0,
      gearUp: false,
      gearDown: false,
      horn: this.horn,
      headlights: false,
    };
    const tilting = this.mode === "tilt" && this.driving && this.tiltSteer !== 0;
    const active = this.touchedThisFrame || this.fingers.size > 0 || tilting || this.horn;
    this.touchedThisFrame = false;
    const edges = this.edges.splice(0);
    return { input, active, edges };
  }
}

const screenAngle = (): number => {
  const o = (screen as Screen & { orientation?: { angle?: number } }).orientation?.angle;
  if (typeof o === "number") return o;
  const w = (window as unknown as { orientation?: number }).orientation;
  return typeof w === "number" ? (w + 360) % 360 : 0;
};

let instance: TouchControls | null = null;

/** Create the touch controls and hook them into `readInput`. They show at once on a touch
 * device (`prefersTouch`), else on the first touch anywhere on the page; `?touch=0` never.
 * Returns the instance (also `touchControls()`). */
export function installTouch(): TouchControls | null {
  if (instance || touchDisabled()) return instance;
  const t = (instance = new TouchControls());
  setTouchReader((dt) => t.read(dt));
  if (prefersTouch()) t.show();
  else {
    const first = (e: PointerEvent | TouchEvent) => {
      if ("pointerType" in e && e.pointerType !== "touch") return;
      t.show();
      window.removeEventListener("pointerdown", first, true);
      window.removeEventListener("touchstart", first, true);
    };
    window.addEventListener("pointerdown", first, true);
    window.addEventListener("touchstart", first, true);
  }
  return t;
}

/** The touch controls, if installed. */
export const touchControls = (): TouchControls | null => instance;

/** Steering mode for touch play (settings / pause menu, machine M): on-screen buttons or tilt.
 * Tilt needs a tap to call this on iOS (motion permission) and falls back to buttons when it is
 * denied, unsupported, or blocked (no orientation events, e.g. some iframes). Remembered in
 * localStorage `wp.touch.v1`. Resolves to the mode in effect. */
export function setTouchSteerMode(mode: TouchSteerMode): Promise<TouchSteerMode> {
  if (!instance) {
    saveMode(mode);
    return Promise.resolve(mode);
  }
  return instance.setMode(mode);
}

/** The current touch steering mode (stored choice before the controls exist). */
export const touchSteerMode = (): TouchSteerMode => instance?.steerMode ?? loadMode();
