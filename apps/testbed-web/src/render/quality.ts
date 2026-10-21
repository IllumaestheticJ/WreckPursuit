// Graphics quality (browser perf pass, docs/integration/browser-perf.md): MSAA, render
// resolution and city draw distance. Nothing here touches materials, lights or post effects.
//
// Modes (`?gfx=auto|high|low`, else the stored choice, else auto):
//   high  MSAA, native resolution (device pixel ratio, capped at 2), full draw distance.
//   low   no MSAA, half resolution, 900 m draw distance, 30 % of the rain streaks, no backdrop
//         blur behind UI panels (re-blurring the 3D view every frame cost ~90 ms a frame under
//         SwiftShader; 5000 streaks ~60 ms).
//   auto  high on a hardware GPU; low on a software rasterizer (SwiftShader, llvmpipe: no
//         usable GPU, e.g. headless Chromium or a blocklisted driver; less far geometry, see profileFor). Then a governor scales
//         the resolution down while frames stay slow (and back up when they are fast again),
//         so the game keeps real time (no slow motion) on weak machines.
//         On a touch device (phone / tablet, src/touch.ts `prefersTouch`) auto = the touch
//         profile: low, pixel ratio capped at 1, 600 m city / 300 m traffic draw distance,
//         mid-LOD car meshes within 120 m, parked-car proxies within 120 m, lamps within 300 m,
//         promoted traffic / police always drawn by the fleet (only the player and AI racers
//         keep their full per-panel view), 20 % rain, governor down to 0.3× (target 30 fps).
// Car LOD on every profile: see VIEW_DRAW_M below.
// MSAA is a WebGL context attribute: changing it needs a reload.
//
// Effects quality (`?fx=low|med|high`, else the stored choice `wp.fx`, else low on the low
// profile and high otherwise) is separate: the crash effects (render/fx) read it at start and
// `CrashFx.setQuality()` changes it live (settings screen).
import { CITY_TILE_M } from "./city_tiles";
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

/** Crash effects quality (render/fx): particle caps, emission rates, skid-mark length and
 * lifetime. Independent of the GPU profile, which only picks the default. */
export type FxLevel = "low" | "med" | "high";
const FX_KEY = "wp.fx";
const FX_LEVELS: FxLevel[] = ["low", "med", "high"];

/** The selected effects quality: URL `?fx=` first, then the stored choice, else `fallback`. */
export function fxLevel(fallback: FxLevel): FxLevel {
  const p = new URLSearchParams(location.search).get("fx") as FxLevel | null;
  if (p && FX_LEVELS.includes(p)) return p;
  const s = store.get(FX_KEY) as FxLevel | null;
  return s && FX_LEVELS.includes(s) ? s : fallback;
}

/** Remember an effects quality (the settings screen applies it live via `CrashFx.setQuality`). */
export function setFxLevel(level: FxLevel): void {
  store.set(FX_KEY, level);
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
  /** Parked-car chunks nearer than this (m) draw the low-poly proxy, farther ones a box. */
  propProxyM: number;
  /** Street lamps (art style) past this distance (m) are not drawn. */
  lampDrawM: number;
  /** Promoted traffic / police cars past this distance (m) are drawn by the fleet, not their
   * own per-panel view (render/lod.ts). */
  viewDrawM: number;
  /** Traffic signal heads within this distance (m) are drawn (signals_view.ts). */
  signalM: number;
  /** Small city detail (kerbs, lot skirts, corner fills) within this distance (m). */
  cityDetailM: number;
  /** City tile edge, m (CityTiles, MarkingsView): bigger tiles, fewer draw calls. */
  cityTileM: number;
}

/**
 * Car LOD (every profile): the player and AI racers always draw their full detailed per-panel
 * view; promoted traffic / police cars draw theirs within VIEW_DRAW_M of the camera (the cars
 * you see crumple; handed to the instanced fleet past VIEW_DRAW_M + render/lod.ts
 * VIEW_LOD_BAND_M, back within VIEW_DRAW_M), touch: never (the phone is draw-call bound).
 * Everything the fleet / parked props draw uses the mid-LOD mesh within `fullLodM` and the
 * low proxy beyond. */
const VIEW_DRAW_M = 30;

function profileFor(mode: GfxMode, software: boolean, touch: boolean): Profile {
  const common = { maxPixelRatio: 2, trafficM: Infinity, fullLodM: 250, propFullMax: 24, propDrawM: Infinity, propProxyM: 300, lampDrawM: Infinity, viewDrawM: VIEW_DRAW_M, signalM: 450, cityDetailM: Infinity, cityTileM: CITY_TILE_M };
  const high = { ...common, antialias: true, backdropBlur: true, rain: 1, scale: 1, minScale: 1, drawM: Infinity, adaptive: false };
  // Low: a weak or no GPU (software rasterizers are vertex / draw-call bound: fewer, cheaper
  // car meshes and lamps matter more than pixels). Mid-LOD car meshes within 120 m (proxies
  // beyond), parked-car proxies within 150 m (two-box cars beyond; every prop stays drawn: no
  // invisible collisions), street lamps within 450 m.
  const low = {
    ...common,
    antialias: false,
    backdropBlur: false,
    rain: 0.3,
    scale: 0.5,
    minScale: 0.5,
    drawM: 900,
    adaptive: false,
    trafficM: 600,
    fullLodM: 120,
    propProxyM: 150,
    lampDrawM: 450,
    // Signal heads within 250 m (a lamp is ~2 px there), kerbs / lot skirts / corner fills
    // within 250 m (city_tiles.ts CITY_DETAIL).
    signalM: 250,
    cityDetailM: 250,
  };
  if (mode === "high") return high;
  if (mode === "low") return low;
  // Touch: 640 m city tiles (render budget, lead/main-sync): with the 600 m draw distance a
  // 320 m tile grid put ~10 tiles of every city material in view (one draw call each); the
  // phone is draw-call bound (budget 300) and has triangle headroom, so fewer, bigger tiles.
  if (touch) return { ...low, rain: 0.2, minScale: 0.3, drawM: 600, adaptive: true, maxPixelRatio: 1, trafficM: 300, fullLodM: 120, propProxyM: 120, lampDrawM: 300, viewDrawM: 0, cityTileM: 2 * CITY_TILE_M };
  // Software GL starts at 0.35× (where the governor always ended up): every governor step
  // resizes the canvas and the post targets, a long hitch under a software rasterizer. Rain at
  // 20 % there (30 % cost ~25 ms a frame, browser-perf.md round 12b).
  return software ? { ...low, scale: 0.35, minScale: 0.25, rain: 0.2, adaptive: true } : { ...high, minScale: 0.6, adaptive: true };
}

/** Frame-time thresholds of the governor (ms, smoothed wall frame time). */
const SLOW_MS = 40;
const FAST_MS = 22;

/**
 * Applies the quality to a renderer and, in auto mode, adapts the resolution scale to the
 * frame time. Call `frame(wallMs)` once per frame.
 */
export class Quality {
  mode: GfxMode;
  readonly software: boolean;
  /** Touch device (phone / tablet): the touch profile in auto mode. */
  readonly touch: boolean;
  /** Instanced traffic draw distance, m (LodDirector.trafficDrawM). */
  trafficDrawM: number;
  /** Full car mesh distance, m (Fleet.fullLodM, PropView.fullLodM). */
  fullLodM: number;
  /** Full-mesh parked cars at most (PropView.maxFull). */
  propFullMax: number;
  /** Parked car draw distance, m (PropView.drawM). */
  propDrawM: number;
  /** Parked-car proxy distance, m (PropView.proxyM). */
  propProxyM: number;
  /** Street-lamp draw distance, m (render/style.ts). */
  lampDrawM: number;
  /** Promoted car view distance, m (LodDirector.viewDrawM). */
  viewDrawM: number;
  /** Signal head draw distance, m (SignalView.drawM). */
  signalDrawM: number;
  /** Small city detail distance, m (SceneView.detailDistanceM). */
  cityDetailM: number;
  /** City tile edge, m, for cities loaded from now on (a loaded city keeps its tiles). */
  cityTileM: number;
  /** Called after `setMode` changed the profile (main re-applies the LOD distances). */
  onChange: (() => void) | null = null;
  readonly rendererName: string;
  readonly antialias: boolean;
  /** The low profile (no MSAA, reduced resolution): optional art-style effects are skipped
   * (render/style.ts: street-lamp light pools). */
  readonly lite: boolean;
  /** City draw distance, m (SceneView.drawDistanceM). */
  drawDistanceM: number;
  /** Fraction of the rain streak pool drawn (WeatherView.rainFraction). */
  rainFraction: number;
  /** Crash effects quality at start (`?fx=`, stored, else low on the low profile, high otherwise). */
  readonly fx: FxLevel;
  /** Resolution scale (1 = native, capped at device pixel ratio 2). */
  scale: number;
  private profile: Profile;
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
    this.trafficDrawM = this.fullLodM = this.propFullMax = this.propDrawM = this.propProxyM = this.lampDrawM = this.viewDrawM = 0;
    this.signalDrawM = this.cityDetailM = 0;
    this.cityTileM = CITY_TILE_M;
    this.applyLod();
    this.antialias = this.profile.antialias;
    this.lite = !this.profile.backdropBlur;
    this.drawDistanceM = this.profile.drawM;
    this.rainFraction = this.profile.rain;
    this.scale = this.profile.scale;
    this.fx = fxLevel(this.profile.antialias ? "high" : "low");
  }

  /** Copy the profile's LOD distances to the public fields. */
  private applyLod(): void {
    this.trafficDrawM = this.profile.trafficM;
    this.fullLodM = this.profile.fullLodM;
    this.propFullMax = this.profile.propFullMax;
    this.propDrawM = this.profile.propDrawM;
    this.propProxyM = this.profile.propProxyM;
    this.lampDrawM = this.profile.lampDrawM;
    this.viewDrawM = this.profile.viewDrawM;
    this.signalDrawM = this.profile.signalM;
    this.cityDetailM = this.profile.cityDetailM;
    this.cityTileM = this.profile.cityTileM;
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
    this.applyBackdrop();
  }

  private applyBackdrop(): void {
    const old = document.getElementById("gfx-no-backdrop");
    if (this.profile.backdropBlur) return void old?.remove();
    if (old) return;
    const style = document.createElement("style");
    style.id = "gfx-no-backdrop";
    style.textContent = "* { backdrop-filter: none !important; -webkit-backdrop-filter: none !important; }";
    document.head.appendChild(style);
  }

  /**
   * Switch the level now (game menu, UI kit): resolution scale, governor, rain density, backdrop
   * blur and draw distance follow at once; MSAA (a context attribute) only after a reload. The
   * choice is stored. Callers copy `drawDistanceM` / `rainFraction` to the scene and weather.
   */
  setMode(mode: GfxMode): void {
    setGfxMode(mode);
    if (mode === this.mode) return;
    this.mode = mode;
    this.profile = profileFor(mode, this.software, this.touch);
    this.drawDistanceM = this.profile.drawM;
    this.rainFraction = this.profile.rain;
    this.scale = this.profile.scale;
    this.applyLod();
    this.slowS = this.fastS = 0;
    this.onChange?.();
    if (this.renderer) {
      this.renderer.setPixelRatio(this.ratio(this.scale));
      document.documentElement.dataset.gfx = mode;
      this.applyBackdrop();
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
