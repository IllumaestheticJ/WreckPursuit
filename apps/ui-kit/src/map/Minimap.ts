/**
 * Always-on corner minimap (canvas 2D): heading-up, zoom scales with speed, roads blitted from
 * one offscreen cache, police dots blinking red/blue when responding or pursuing (steady on
 * patrol), sight areas and the search zone while wanted, roadblocks / spikes, the helicopter,
 * garages and safehouses, optional traffic signals, and the escape ring while searching.
 * Units outside the view sit on the rim as edge markers.
 */
import type { MapFrame, MapStatic } from "./data";
import { isWanted, racersToDraw } from "./data";
import { buildRoadCache, drawPoi, drawPlayer, drawRacer, drawSearch, drawSight, drawTool, drawUnit, readPalette, type MapPalette, type RoadCache } from "./draw";
import { drawFlag, drawMarker, drawRoute, type MapExtras } from "./extras";
import { angleDelta, clampToCircle, compose, glyphRotation, minimapMatrix, radiusForSpeed, smooth, worldToMinimap, type MinimapView } from "./projection";

/** Minimap options. */
export interface MinimapOptions {
  /** Draw signalised junctions. Default false. */
  signals?: boolean;
}

/** What the minimap drew last frame (e2e and debugging). */
export interface MinimapStats {
  /** Player arrow drawn. */
  player: boolean;
  /** Police units drawn (inside the disc or on the rim). */
  units: number;
  /** Units drawn with lights (blinking). */
  blinking: number;
  /** Race opponents drawn (inside the disc or on the rim). */
  racers: number;
  /** Roadblocks / spike strips drawn. */
  tools: number;
  /** Search zone drawn. */
  search: boolean;
  /** Escape ring progress drawn (0 = none). */
  escape: number;
  /** World radius shown, m. */
  radiusM: number;
  /** Host markers drawn (`MapHud.setExtras`; inside the disc or on the rim). */
  markers: number;
  /** Host route drawn. */
  route: boolean;
}

/** The minimap renderer. Call `setMap` once per city and `draw` every frame. */
export class MinimapRenderer {
  private map: MapStatic | null = null;
  private cache: RoadCache | null = null;
  private pal: MapPalette;
  private radiusM = NaN;
  private yaw = NaN;
  private lastMs = NaN;
  private ctx: CanvasRenderingContext2D | null;
  /** Last frame's stats. */
  stats: MinimapStats = { player: false, units: 0, blinking: 0, racers: 0, tools: 0, search: false, escape: 0, radiusM: 0, markers: 0, route: false };
  options: MinimapOptions;
  /** Host overlay (street racers, a race route); null = none. */
  extras: MapExtras | null = null;

  constructor(
    readonly canvas: HTMLCanvasElement,
    options: MinimapOptions = {},
  ) {
    this.ctx = canvas.getContext("2d");
    this.pal = readPalette(canvas);
    this.options = { ...options };
  }

  /** New city (or `null` for none). Builds the road cache. */
  setMap(map: MapStatic | null): void {
    this.map = map;
    this.cache = map ? buildRoadCache(map) : null;
    this.pal = readPalette(this.canvas);
    this.radiusM = NaN;
    this.yaw = NaN;
  }

  /** Re-read colours (palette setting changed). */
  refreshPalette(): void {
    this.pal = readPalette(this.canvas);
  }

  /** Draw one frame at `nowMs` (any monotonic clock, ms). */
  draw(f: MapFrame, nowMs: number): void {
    const ctx = this.ctx;
    if (!ctx) return;
    const dpr = (typeof devicePixelRatio === "number" && devicePixelRatio) || 1;
    const css = this.canvas.clientWidth || 200;
    const size = Math.round(css * dpr);
    if (this.canvas.width !== size || this.canvas.height !== size) {
      this.canvas.width = size;
      this.canvas.height = size;
    }
    const pal = this.pal;
    const R = size / 2;
    const rim = Math.max(2, size * 0.025);
    const inner = R - rim;
    const dt = Number.isFinite(this.lastMs) ? (nowMs - this.lastMs) / 1000 : 0;
    this.lastMs = nowMs;
    const p = f.player;
    this.radiusM = smooth(this.radiusM, radiusForSpeed(p?.speed ?? 0), dt);
    // Smooth the heading a little (a rebound after a crash flicks the yaw).
    const yawTarget = p?.yaw ?? 0;
    this.yaw = Number.isFinite(this.yaw) ? this.yaw + angleDelta(this.yaw, yawTarget) * Math.min(1, dt / 0.12) : yawTarget;
    const view: MinimapView = { cx: p?.x ?? 0, cy: p?.y ?? 0, yaw: this.yaw, pxPerM: inner / this.radiusM, sx: R, sy: R };
    const m = minimapMatrix(view);
    const k = view.pxPerM;
    const stats: MinimapStats = { player: false, units: 0, blinking: 0, racers: 0, tools: 0, search: false, escape: 0, radiusM: this.radiusM, markers: 0, route: false };
    const ex = this.extras;

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, size, size);
    // disc
    ctx.beginPath();
    ctx.arc(R, R, inner, 0, Math.PI * 2);
    ctx.fillStyle = "rgba(4,7,6,0.86)";
    ctx.fill();
    ctx.save();
    ctx.clip();
    if (this.cache && p) {
      const cm = compose(m, this.cache.toWorld);
      ctx.setTransform(cm[0], cm[1], cm[2], cm[3], cm[4], cm[5]);
      ctx.imageSmoothingEnabled = true;
      ctx.drawImage(this.cache.canvas as CanvasImageSource, 0, 0);
      ctx.setTransform(1, 0, 0, 1, 0, 0);
    }
    if (this.map && p && this.options.signals) {
      ctx.fillStyle = pal.ink3;
      for (const [x, y] of this.map.signals) {
        const [sx, sy] = worldToMinimap(view, x, y);
        if ((sx - R) ** 2 + (sy - R) ** 2 > inner * inner) continue;
        ctx.fillRect(sx - 1.5 * dpr, sy - 1.5 * dpr, 3 * dpr, 3 * dpr);
      }
    }
    if (p && isWanted(f)) {
      drawSight(ctx, f, m, k, pal);
      if (f.search) {
        drawSearch(ctx, f, m, k, nowMs, pal);
        stats.search = true;
      }
    }
    for (const t of f.tools) {
      drawTool(ctx, t, m, k, pal);
      stats.tools++;
    }
    if (this.map && p) {
      const s = 5.5 * dpr;
      for (const poi of this.map.pois) {
        if (poi.kind !== "Garage" && poi.kind !== "Safehouse") continue;
        const [sx, sy] = worldToMinimap(view, poi.pos[0], poi.pos[1]);
        if ((sx - R) ** 2 + (sy - R) ** 2 > (inner - s) ** 2) continue;
        drawPoi(ctx, poi.kind, sx, sy, s, pal);
      }
    }
    if (p && ex?.route && ex.route.length > 1) {
      drawRoute(ctx, ex.route.map(([x, y]) => worldToMinimap(view, x, y)), 3 * dpr);
      stats.route = true;
    }
    ctx.restore();

    // units (rim markers for the ones outside)
    if (p) {
      const us = 5.2 * dpr;
      for (const u of f.units) {
        if (u.wrecked && u.state === "out") continue;
        const [sx, sy] = worldToMinimap(view, u.x, u.y);
        const c = clampToCircle(R, R, inner - us, sx, sy);
        if (c.outside && !(u.lights || u.onPlayer || u.heli)) continue;
        drawUnit(ctx, u, c.x, c.y, glyphRotation(u.yaw, view.yaw), c.outside ? us * 0.75 : us, nowMs, pal);
        stats.units++;
        if (u.lights && !u.wrecked) stats.blinking++;
      }
      // race opponents (rim markers for the ones outside), back of the field first so the
      // leader is drawn on top
      const rs = 5.5 * dpr;
      for (const r of racersToDraw(f).reverse()) {
        const [sx, sy] = worldToMinimap(view, r.x, r.y);
        const c = clampToCircle(R, R, inner - rs * 1.4, sx, sy);
        drawRacer(ctx, r, c.x, c.y, glyphRotation(r.yaw, view.yaw), c.outside ? rs * 0.75 : rs, pal);
        stats.racers++;
      }
      // host markers (rim markers for the ones outside) and flags (kept on the rim)
      if (ex) {
        const ms = 6 * dpr;
        for (const mk of ex.markers) {
          const [sx, sy] = worldToMinimap(view, mk.x, mk.y);
          const c = clampToCircle(R, R, inner - ms * 1.3, sx, sy);
          drawMarker(ctx, mk, c.x, c.y, c.outside ? ms * 0.75 : ms);
          stats.markers++;
        }
        for (const fl of ex.flags) {
          const [sx, sy] = worldToMinimap(view, fl.x, fl.y);
          const c = clampToCircle(R, R, inner - 12 * dpr, sx, sy);
          drawFlag(ctx, fl, c.x, c.y + 6 * dpr, 14 * dpr);
        }
      }
      drawPlayer(ctx, R, R, 0, 7 * dpr, pal);
      stats.player = true;
    }

    // rim: police colours while pursued, scanner green otherwise
    ctx.beginPath();
    ctx.arc(R, R, inner + rim / 2, 0, Math.PI * 2);
    ctx.lineWidth = rim;
    const chased = f.status === "pursuit";
    ctx.strokeStyle = chased ? (Math.floor(nowMs / 250) % 2 ? pal.policeA : pal.policeB) : f.status === "searching" ? pal.warn : pal.edgeHot;
    ctx.stroke();
    // escape ring
    if (f.status === "searching" && f.escape > 0) {
      ctx.beginPath();
      ctx.arc(R, R, inner + rim / 2, -Math.PI / 2, -Math.PI / 2 + f.escape * Math.PI * 2);
      ctx.strokeStyle = pal.scan;
      ctx.lineWidth = rim * 1.6;
      ctx.stroke();
      ctx.fillStyle = pal.scan;
      ctx.font = `bold ${Math.round(10 * dpr)}px ui-monospace, Menlo, Consolas, monospace`;
      ctx.textAlign = "center";
      ctx.textBaseline = "alphabetic";
      ctx.fillText(`ESCAPE ${Math.round(f.escape * 100)}%`, R, size - rim * 3 - 4 * dpr);
      stats.escape = f.escape;
    }
    // north marker on the rim
    if (p) {
      const [nx, ny] = worldToMinimap(view, view.cx, view.cy + this.radiusM);
      const c = clampToCircle(R, R, inner - 7 * dpr, nx, ny);
      ctx.fillStyle = pal.ink2;
      ctx.font = `bold ${Math.round(10 * dpr)}px ui-monospace, Menlo, Consolas, monospace`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText("N", c.x, c.y);
    }
    this.stats = stats;
  }
}
