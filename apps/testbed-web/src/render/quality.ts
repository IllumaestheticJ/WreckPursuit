// Graphics quality (browser perf pass, docs/integration/browser-perf.md): MSAA, render
// resolution and city draw distance. Nothing here touches materials, lights or post effects.
//
// Modes (`?gfx=auto|high|low`, else the stored choice, else auto):
//   high  MSAA, native resolution (device pixel ratio, capped at 2), full draw distance.
//   low   no MSAA, half resolution, 900 m draw distance, 30 % of the rain streaks, no backdrop
//         blur behind UI panels (re-blurring the 3D view every frame cost ~90 ms a frame under
//         SwiftShader; 5000 streaks ~60 ms).
//   auto  high on a hardware GPU; low on a software rasterizer (SwiftShader, llvmpipe: no
//         usable GPU, e.g. headless Chromium or a blocklisted driver). Then a governor scales
//         the resolution down while frames stay slow (and back up when they are fast again),
//         so the game keeps real time (no slow motion) on weak machines.
//         On a touch device (phone / tablet, src/touch.ts `prefersTouch`) auto = the touch
//         profile: low, pixel ratio capped at 1, 600 m city / 300 m traffic draw distance, full
//         car meshes only within 120 m (at most 24 parked cars), parked cars drawn within 350 m, 20 % rain, governor down to 0.6 px ratio (target 30 fps).
// MSAA is a WebGL context attribute: changing it needs a reload.
import type * as THREE from "three";
import { store } from "../settings";
import { prefersTouch } from "../touch";

export type GfxMode = "auto" | "high" | "low";
const STORE_KEY = "wp.gfx";
const MODES: GfxMode[] = ["auto", "high", "low"];

/** Software rasterizers (renderer string of WEBGL_debug_renderer_info). */
const SOFTWARE_GL = /swiftshader|llvmpipe|softpipe|software|basic render/i;

/** The selected mode: URL `?gfx=` first, then the viewer's stored choice, else auto. */
export function gfxMode(): GfxMode {
  const p = new URLSearchParams(location.search).get("gfx") as GfxMode | null;
  if (p && MODES.includes(p)) return p;
  const s = store.get(STORE_KEY) as GfxMode | null;
  return s && MODES.includes(s) ? s : "auto";
}

/** Remember a mode (takes effect on the next page load). */
export function setGfxMode(mode: GfxMode): void {
  store.set(STORE_KEY, mode);
}

/** WebGL renderer string, probed on a throwaway canvas ("" if unavailable). */
export function glRendererName(): string {
  try {
    const gl = document.createElement("canvas").getContext("webgl2") ?? document.createElement("canvas").getContext("webgl");
    if (!gl) return "";
    const ext = gl.getExtension("WEBGL_debug_renderer_info");
    const name = String(gl.getParameter(ext ? ext.UNMASKED_RENDERER_WEBGL : gl.RENDERER) ?? "");
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return name;
  } catch {
    return "";
  }
}

/** Static settings for a mode: MSAA, resolution scale range, draw distance (m). */
interface Profile {
  antialias: boolean;
  /** CSS backdrop-filter (blur behind UI kit panels) allowed. */
  backdropBlur: boolean;
  /** Fraction of the rain streak pool drawn. */
  rain: number;
  scale: number;
  minScale: number;
  drawM: number;
  adaptive: boolean;
  /** Highest effective pixel ratio (after the scale). */
  maxPixelRatio: number;
  /** Instanced traffic past this distance (m) is not drawn. */
  trafficM: number;
  /** Full car meshes within this distance (m), proxies beyond (render/fleet.ts, props.ts). */
  fullLodM: number;
  /** Full-mesh parked cars at most (render/props.ts). */
  propFullMax: number;
  /** Parked cars past this distance (m) are not drawn (render/props.ts). */
  propDrawM: number;
}

function profileFor(mode: GfxMode, software: boolean, touch: boolean): Profile {
  const common = { maxPixelRatio: 2, trafficM: Infinity, fullLodM: 250, propFullMax: 256, propDrawM: Infinity };
  const high = { ...common, antialias: true, backdropBlur: true, rain: 1, scale: 1, minScale: 1, drawM: Infinity, adaptive: false };
  const low = { ...common, antialias: false, backdropBlur: false, rain: 0.3, scale: 0.5, minScale: 0.5, drawM: 900, adaptive: false };
  if (mode === "high") return high;
  if (mode === "low") return low;
  if (touch) return { ...low, rain: 0.2, minScale: 0.3, drawM: 600, adaptive: true, maxPixelRatio: 1, trafficM: 300, fullLodM: 120, propFullMax: 24, propDrawM: 350 };
  return software ? { ...low, minScale: 0.35, adaptive: true } : { ...high, minScale: 0.6, adaptive: true };
}

/** Frame-time thresholds of the governor (ms, smoothed wall frame time). */
const SLOW_MS = 40;
const FAST_MS = 22;

/**
 * Applies the quality to a renderer and, in auto mode, adapts the resolution scale to the
 * frame time. Call `frame(wallMs)` once per frame.
 */
export class Quality {
  readonly mode: GfxMode;
  readonly software: boolean;
  /** Touch device (phone / tablet): the touch profile in auto mode. */
  readonly touch: boolean;
  /** Instanced traffic draw distance, m (LodDirector.trafficDrawM). */
  readonly trafficDrawM: number;
  /** Full car mesh distance, m (Fleet.fullLodM, PropView.fullLodM). */
  readonly fullLodM: number;
  /** Full-mesh parked cars at most (PropView.maxFull). */
  readonly propFullMax: number;
  /** Parked car draw distance, m (PropView.drawM). */
  readonly propDrawM: number;
  readonly rendererName: string;
  readonly antialias: boolean;
  /** City draw distance, m (SceneView.drawDistanceM). */
  readonly drawDistanceM: number;
  /** Fraction of the rain streak pool drawn (WeatherView.rainFraction). */
  readonly rainFraction: number;
  /** Resolution scale (1 = native, capped at device pixel ratio 2). */
  scale: number;
  private readonly profile: Profile;
  private renderer: THREE.WebGLRenderer | null = null;
  private emaMs = 16.7;
  private slowS = 0;
  private fastS = 0;
  /** Resolution changes made by the governor (automation, HUD). */
  changes = 0;

  constructor() {
    this.mode = gfxMode();
    this.rendererName = glRendererName();
    this.software = SOFTWARE_GL.test(this.rendererName);
    this.touch = prefersTouch();
    this.profile = profileFor(this.mode, this.software, this.touch);
    this.trafficDrawM = this.profile.trafficM;
    this.fullLodM = this.profile.fullLodM;
    this.propFullMax = this.profile.propFullMax;
    this.propDrawM = this.profile.propDrawM;
    this.antialias = this.profile.antialias;
    this.drawDistanceM = this.profile.drawM;
    this.rainFraction = this.profile.rain;
    this.scale = this.profile.scale;
  }

  /** Native pixel ratio cap. */
  private static native(): number {
    return Math.min(window.devicePixelRatio || 1, 2);
  }

  /** Effective pixel ratio for a scale (capped by the profile). */
  private ratio(scale: number): number {
    return Math.min(Quality.native() * scale, this.profile.maxPixelRatio);
  }

  attach(renderer: THREE.WebGLRenderer): void {
    this.renderer = renderer;
    renderer.setPixelRatio(this.ratio(this.scale));
    document.documentElement.dataset.gfx = this.mode;
    if (!this.profile.backdropBlur) {
      const style = document.createElement("style");
      style.id = "gfx-no-backdrop";
      style.textContent = "* { backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }";
      document.head.appendChild(style);
    }
  }

  /** One frame of `wallMs` (rAF interval). Ignores hitches over 1 s (loading, tab switches). */
  frame(wallMs: number): void {
    if (!this.profile.adaptive || !this.renderer || wallMs > 1000 || wallMs <= 0) return;
    this.emaMs += (wallMs - this.emaMs) * 0.1;
    const dt = wallMs / 1000;
    this.slowS = this.emaMs > SLOW_MS ? this.slowS + dt : 0;
    this.fastS = this.emaMs < FAST_MS ? this.fastS + dt : 0;
    let next = this.scale;
    if (this.slowS > 1.5) next = Math.max(this.profile.minScale, this.scale * 0.85);
    else if (this.fastS > 4) next = Math.min(this.profile.scale, this.scale / 0.85);
    if (Math.abs(next - this.scale) > 1e-3) {
      this.scale = next;
      this.renderer.setPixelRatio(this.ratio(next));
      this.changes++;
      this.slowS = 0;
      this.fastS = 0;
    }
  }

  /** Short label for the HUD: `auto (software GL) · 0.50× · no MSAA`. */
  label(): string {
    const kind = `${this.software ? " (software GL)" : ""}${this.touch && this.mode === "auto" ? " (touch)" : ""}`;
    return `${this.mode}${kind} · ${this.scale.toFixed(2)}× · ${this.antialias ? "MSAA" : "no MSAA"}`;
  }
}
