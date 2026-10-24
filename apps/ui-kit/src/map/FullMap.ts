/**
 * Full city map (canvas 2D), north up: districts tinted and named, roads by class with bridge
 * decks drawn over a casing, POIs with names, the player and police as on the minimap, sight
 * areas / search zone while wanted. Pan (drag, arrows / WASD) and zoom (wheel, + / -). The
 * static layers are redrawn into an offscreen canvas only when the view changes; each frame
 * blits that and draws the moving things on top.
 */
import type { MapFrame, MapStatic, MapZone } from "./data";
import { isWanted, racersToDraw } from "./data";
import { districtLabels, type DistrictLabel } from "./districts";
import { drawHeliTether, drawPoi, drawPlayer, drawRacer, drawRoads, drawSearch, drawSight, drawTool, drawUnit, heliFollows, makeCanvas, readPalette, ROAD_MIN_PX, type MapPalette } from "./draw";
import { drawFlag, drawMarker, drawRoute, GPS_COLOUR, type MapExtras } from "./extras";
import { clampCentre, fitBounds, fullMatrix, glyphRotation, pan, worldToFull, zoomAt, type FullView } from "./projection";

/** District tints by kind (low alpha fills). */
export const DISTRICT_TINT: Record<MapZone["kind"], string> = {
  Core: "rgba(61,255,143,0.10)",
  Commercial: "rgba(120,170,255,0.07)",
  Residential: "rgba(200,190,140,0.06)",
  Industrial: "rgba(255,160,60,0.07)",
  Port: "rgba(80,200,220,0.08)",
  Suburb: "rgba(150,160,150,0.035)",
};

/** What the full map drew last frame (e2e). */
export interface FullMapStats {
  units: number;
  /** Race opponents drawn. */
  racers: number;
  player: boolean;
  districts: number;
  pois: number;
  pxPerM: number;
  /** Host markers drawn (`MapHud.setExtras`). */
  markers: number;
  /** Host route drawn. */
  route: boolean;
}

/** The full map renderer. */
export class FullMapRenderer {
  private map: MapStatic | null = null;
  private labels: DistrictLabel[] = [];
  private pal: MapPalette;
  private base: HTMLCanvasElement | OffscreenCanvas | null = null;
  private baseKey = "";
  private ctx: CanvasRenderingContext2D | null;
  view: FullView = { cx: 0, cy: 0, pxPerM: 0.2, w: 1, h: 1 };
  stats: FullMapStats = { units: 0, racers: 0, player: false, districts: 0, pois: 0, pxPerM: 0, markers: 0, route: false };
  /** Host overlay (street racers, a race route); null = none. */
  extras: MapExtras | null = null;
  /** GPS route `[x, y]` world points (F40); null = none. */
  gps: [number, number][] | null = null;
  /** Crosshair at the screen centre (gamepad picking). */
  crosshair = false;

  constructor(readonly canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d");
    this.pal = readPalette(canvas);
  }

  /** New city. */
  setMap(map: MapStatic | null): void {
    this.map = map;
    this.labels = map ? districtLabels(map) : [];
    this.pal = readPalette(this.canvas);
    this.baseKey = "";
    this.fit();
  }

  private size(): { w: number; h: number; dpr: number } {
    const dpr = (typeof devicePixelRatio === "number" && devicePixelRatio) || 1;
    return { w: Math.max(1, Math.round((this.canvas.clientWidth || 800) * dpr)), h: Math.max(1, Math.round((this.canvas.clientHeight || 600) * dpr)), dpr };
  }

  /** Fit the whole city. */
  fit(): void {
    const { w, h, dpr } = this.size();
    if (!this.map) return;
    this.view = fitBounds(this.map.bounds, w, h, 28 * dpr);
  }

  /** Centre on a world point (keeps the zoom). */
  centreOn(x: number, y: number): void {
    this.view = { ...this.view, cx: x, cy: y };
  }

  /** Drag by screen CSS pixels. */
  panBy(dxCss: number, dyCss: number): void {
    const dpr = this.size().dpr;
    this.setView(pan(this.view, dxCss * dpr, dyCss * dpr));
  }

  /** Zoom by `factor` about a CSS point (default: centre). */
  zoomBy(factor: number, cssX?: number, cssY?: number): void {
    const dpr = this.size().dpr;
    const sx = cssX === undefined ? this.view.w / 2 : cssX * dpr;
    const sy = cssY === undefined ? this.view.h / 2 : cssY * dpr;
    this.setView(zoomAt(this.view, sx, sy, factor));
  }

  private setView(v: FullView): void {
    this.view = this.map ? clampCentre(v, this.map.bounds) : v;
  }

  private drawBase(w: number, h: number, dpr: number): void {
    const key = `${w}x${h}|${this.view.cx.toFixed(2)},${this.view.cy.toFixed(2)},${this.view.pxPerM.toFixed(5)}`;
    if (key === this.baseKey && this.base) return;
    this.baseKey = key;
    if (!this.base || this.base.width !== w || this.base.height !== h) this.base = makeCanvas(w, h);
    const ctx = this.base.getContext("2d") as CanvasRenderingContext2D | null;
    if (!ctx) return;
    const pal = this.pal;
    const v = this.view;
    const m = fullMatrix(v);
    const k = v.pxPerM;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#050807";
    ctx.fillRect(0, 0, w, h);
    const map = this.map;
    if (!map) return;
    // districts: fill from the last zone to the first, so the first containing zone is on top
    for (let i = map.zones.length - 1; i >= 0; i--) {
      const z = map.zones[i];
      ctx.beginPath();
      for (let j = 0; j + 1 < z.poly.length; j += 2) {
        const [sx, sy] = worldToFull(v, z.poly[j], z.poly[j + 1]);
        if (j === 0) ctx.moveTo(sx, sy);
        else ctx.lineTo(sx, sy);
      }
      ctx.closePath();
      ctx.fillStyle = "#050807";
      ctx.fill();
      ctx.fillStyle = DISTRICT_TINT[z.kind] ?? "transparent";
      ctx.fill();
      ctx.strokeStyle = pal.edge;
      ctx.lineWidth = 1 * dpr;
      ctx.setLineDash([4 * dpr, 4 * dpr]);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    drawRoads(ctx, map, m, k, ROAD_MIN_PX.map((p) => p * dpr));
    // signalised junctions when zoomed in
    if (k > 0.6) {
      ctx.fillStyle = pal.ink3;
      for (const [x, y] of map.signals) {
        const [sx, sy] = worldToFull(v, x, y);
        ctx.fillRect(sx - 1.5 * dpr, sy - 1.5 * dpr, 3 * dpr, 3 * dpr);
      }
    }
    // district names
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    const fs = Math.round(Math.min(18, Math.max(10, 70 * k)) * dpr);
    ctx.font = `bold ${fs}px ui-monospace, Menlo, Consolas, monospace`;
    for (const l of this.labels) {
      const [sx, sy] = worldToFull(v, l.x, l.y);
      if (sx < -200 || sy < -40 || sx > w + 200 || sy > h + 40) continue;
      const text = l.name.toUpperCase().split("").join(" ");
      ctx.lineWidth = 3 * dpr;
      ctx.strokeStyle = "rgba(0,0,0,0.85)";
      ctx.strokeText(text, sx, sy);
      ctx.fillStyle = l.kind === "Core" ? pal.scan : pal.ink2;
      ctx.globalAlpha = 0.85;
      ctx.fillText(text, sx, sy);
      ctx.globalAlpha = 1;
    }
  }

  /** Draw one frame. */
  draw(f: MapFrame, nowMs: number): void {
    const ctx = this.ctx;
    if (!ctx) return;
    const { w, h, dpr } = this.size();
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    if (this.view.w !== w || this.view.h !== h) {
      const first = this.view.w <= 1;
      this.view = { ...this.view, w, h };
      if (first) this.fit();
    }
    this.drawBase(w, h, dpr);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    if (this.base) ctx.drawImage(this.base as CanvasImageSource, 0, 0);
    const pal = this.pal;
    const v = this.view;
    const m = fullMatrix(v);
    const k = v.pxPerM;
    if (isWanted(f)) {
      drawSight(ctx, f, m, k, pal);
      drawSearch(ctx, f, m, k, nowMs, pal);
    }
    for (const t of f.tools) drawTool(ctx, t, m, k, pal);
    const ex = this.extras;
    let markers = 0;
    if (this.gps && this.gps.length > 1) drawRoute(ctx, this.gps.map(([x, y]) => worldToFull(v, x, y)), 3.5 * dpr, GPS_COLOUR);
    if (ex?.route && ex.route.length > 1) drawRoute(ctx, ex.route.map(([x, y]) => worldToFull(v, x, y)), 3.5 * dpr);
    let units = 0;
    const us = 7 * dpr;
    // Who the helicopter is following (PR #32): tether + pulsing ring, under the glyphs.
    for (const u of f.units) {
      const at = heliFollows(u, f.player);
      if (!at) continue;
      const [hx, hy] = worldToFull(v, u.x, u.y);
      const [tx, ty] = worldToFull(v, at[0], at[1]);
      drawHeliTether(ctx, hx, hy, tx, ty, 8 * dpr, nowMs, pal);
    }
    for (const u of f.units) {
      if (u.wrecked && u.state === "out") continue;
      const [sx, sy] = worldToFull(v, u.x, u.y);
      if (sx < -20 || sy < -20 || sx > w + 20 || sy > h + 20) continue;
      drawUnit(ctx, u, sx, sy, glyphRotation(u.yaw, 0), us, nowMs, pal);
      units++;
    }
    let racers = 0;
    for (const r of racersToDraw(f).reverse()) {
      const [sx, sy] = worldToFull(v, r.x, r.y);
      if (sx < -20 || sy < -20 || sx > w + 20 || sy > h + 20) continue;
      drawRacer(ctx, r, sx, sy, glyphRotation(r.yaw, 0), 7 * dpr, pal);
      racers++;
    }
    if (ex) {
      for (const mk of ex.markers) {
        const [sx, sy] = worldToFull(v, mk.x, mk.y);
        if (sx < -20 || sy < -20 || sx > w + 20 || sy > h + 20) continue;
        drawMarker(ctx, mk, sx, sy, 8 * dpr);
        markers++;
      }
      for (const fl of ex.flags) {
        const [sx, sy] = worldToFull(v, fl.x, fl.y);
        drawFlag(ctx, fl, sx, sy, 20 * dpr);
      }
    }
    // POIs above the host markers (street racers sat on top of shops, garage audit F7). Shops
    // and race meets keep a readable size and their names at every zoom.
    ctx.font = `${Math.round(10 * dpr)}px ui-monospace, Menlo, Consolas, monospace`;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    const map = this.map;
    for (const poi of map?.pois ?? []) {
      if (poi.kind === "RaceStartGrid" && k < 0.3) continue;
      const [sx, sy] = worldToFull(v, poi.pos[0], poi.pos[1]);
      if (sx < -40 || sy < -40 || sx > w + 200 || sy > h + 40) continue;
      const key = poi.kind === "Garage" || poi.kind === "RaceSpot";
      const ps = (key ? 9 : 7) * dpr;
      drawPoi(ctx, poi.kind, sx, sy, ps, pal, poi.shop);
      if (poi.kind !== "Helipad" && (k > 0.35 || (key && k > 0.12))) {
        ctx.lineWidth = 3 * dpr;
        ctx.strokeStyle = "rgba(0,0,0,0.85)";
        ctx.strokeText(poi.name, sx + ps * 1.1, sy);
        ctx.fillStyle = pal.ink;
        ctx.fillText(poi.name, sx + ps * 1.1, sy);
      }
    }
    if (this.crosshair) {
      const cx = w / 2;
      const cy = h / 2;
      ctx.strokeStyle = pal.ink;
      ctx.lineWidth = 1.5 * dpr;
      ctx.beginPath();
      ctx.arc(cx, cy, 10 * dpr, 0, Math.PI * 2);
      ctx.moveTo(cx - 18 * dpr, cy);
      ctx.lineTo(cx - 4 * dpr, cy);
      ctx.moveTo(cx + 4 * dpr, cy);
      ctx.lineTo(cx + 18 * dpr, cy);
      ctx.moveTo(cx, cy - 18 * dpr);
      ctx.lineTo(cx, cy - 4 * dpr);
      ctx.moveTo(cx, cy + 4 * dpr);
      ctx.lineTo(cx, cy + 18 * dpr);
      ctx.stroke();
    }
    if (f.player) {
      const [sx, sy] = worldToFull(v, f.player.x, f.player.y);
      // locator ring so the player is easy to find on the whole-city view
      ctx.beginPath();
      ctx.arc(sx, sy, (14 + 4 * Math.sin(nowMs / 220)) * dpr, 0, Math.PI * 2);
      ctx.strokeStyle = pal.scan;
      ctx.globalAlpha = 0.6;
      ctx.lineWidth = 1.5 * dpr;
      ctx.stroke();
      ctx.globalAlpha = 1;
      drawPlayer(ctx, sx, sy, glyphRotation(f.player.yaw, 0), 8 * dpr, pal);
    }
    this.stats = { units, racers, player: !!f.player, districts: this.labels.length, pois: this.map?.pois.length ?? 0, pxPerM: k, markers, route: !!(ex?.route && ex.route.length > 1) };
  }
}
