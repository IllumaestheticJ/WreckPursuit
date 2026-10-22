/**
 * Shared canvas drawing for the minimap and the full map: palette (read from the kit's CSS
 * variables, so colour-blind palettes apply), road styling by class and layer, the offscreen
 * road cache, police glyphs, tools, POI icons. Everything here draws in screen pixels unless a
 * function says it takes a world matrix.
 */
import type { MapFrame, MapPoi, MapRacer, MapStatic, MapTool, MapUnit } from "./data";
import { unitBlinks } from "./data";
import { apply, type Mat } from "./projection";

/** Colours used by the map layers. */
export interface MapPalette {
  bg: string;
  scan: string;
  scanDim: string;
  ink: string;
  ink2: string;
  ink3: string;
  warn: string;
  danger: string;
  policeA: string;
  policeB: string;
  /** Race opponents (not police red / blue, not the player's scanner green). */
  racer: string;
  edge: string;
  edgeHot: string;
}

const FALLBACK: MapPalette = {
  bg: "#090b0a",
  scan: "#3dff8f",
  scanDim: "#1b6b41",
  ink: "#e6efe9",
  ink2: "#a9b8af",
  ink3: "#6c7d73",
  warn: "#ffb21e",
  danger: "#ff3d3d",
  policeA: "#ff2e55",
  policeB: "#3b7dff",
  racer: "#c77dff",
  edge: "#233029",
  edgeHot: "#3a5246",
};

/** Read the palette from the kit's CSS variables on `el` (fallbacks outside a `.wp-ui`). */
export function readPalette(el: Element | null): MapPalette {
  if (!el || typeof getComputedStyle === "undefined") return FALLBACK;
  const cs = getComputedStyle(el);
  const v = (name: string, fb: string) => cs.getPropertyValue(name).trim() || fb;
  return {
    bg: v("--bg", FALLBACK.bg),
    scan: v("--scan", FALLBACK.scan),
    scanDim: v("--scan-dim", FALLBACK.scanDim),
    ink: v("--ink", FALLBACK.ink),
    ink2: v("--ink-2", FALLBACK.ink2),
    ink3: v("--ink-3", FALLBACK.ink3),
    warn: v("--warn", FALLBACK.warn),
    danger: v("--danger", FALLBACK.danger),
    policeA: v("--police-a", FALLBACK.policeA),
    policeB: v("--police-b", FALLBACK.policeB),
    racer: v("--racer", FALLBACK.racer),
    edge: v("--edge", FALLBACK.edge),
    edgeHot: v("--edge-hot", FALLBACK.edgeHot),
  };
}

/** Road widths by class (highway … alley), world m, as drawn. */
export const ROAD_WIDTH_M = [16, 11, 8, 6, 4] as const;
/** Minimum road widths on the full map by class, px. */
export const ROAD_MIN_PX = [2.4, 1.8, 1.2, 0.9, 0.6] as const;
/** Road colours by class: muted so police lights stay the brightest thing on screen. */
export const ROAD_COLOR = ["#9db3a6", "#748a7d", "#5b6d62", "#45544b", "#323d37"] as const;
/** Bridge deck casing (drawn under decks so an overpass reads as on top). */
export const DECK_CASING = "#020403";

/** A canvas usable off screen (OffscreenCanvas where available). */
export function makeCanvas(w: number, h: number): HTMLCanvasElement | OffscreenCanvas {
  if (typeof OffscreenCanvas !== "undefined") return new OffscreenCanvas(w, h);
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return c;
}

type Ctx = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

function tracePolyline(ctx: Ctx, m: Mat, pts: number[]): void {
  let [x, y] = apply(m, pts[0], pts[1]);
  ctx.moveTo(x, y);
  for (let i = 2; i + 1 < pts.length; i += 2) {
    [x, y] = apply(m, pts[i], pts[i + 1]);
    ctx.lineTo(x, y);
  }
}

/**
 * Stroke every road with the world→screen matrix `m` (uniform scale `k` px/m): grade roads by
 * class (minor first), then bridge decks over a dark casing. One path per class and layer.
 */
export function drawRoads(ctx: Ctx, map: MapStatic, m: Mat, k: number, minPx: readonly number[] = [0, 0, 0, 0, 0]): void {
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const layer of [0, 1]) {
    for (let c = 4; c >= 0; c--) {
      const roads = map.roads.filter((r) => r.layer === layer && r.class === c);
      if (!roads.length) continue;
      const w = Math.max(ROAD_WIDTH_M[c] * k, minPx[c] ?? 0);
      if (layer === 1) {
        ctx.beginPath();
        for (const r of roads) tracePolyline(ctx, m, r.pts);
        ctx.strokeStyle = DECK_CASING;
        ctx.lineWidth = w + Math.max(3, 6 * k);
        ctx.lineCap = "butt";
        ctx.stroke();
      }
      ctx.beginPath();
      for (const r of roads) tracePolyline(ctx, m, r.pts);
      ctx.strokeStyle = ROAD_COLOR[c];
      ctx.lineWidth = w;
      ctx.lineCap = layer === 1 ? "butt" : "round";
      ctx.stroke();
    }
  }
}

/** Rasterised roads of a city: `px = (x - x0)·q`, `py = (y1 - y)·q`. */
export interface RoadCache {
  canvas: HTMLCanvasElement | OffscreenCanvas;
  x0: number;
  y1: number;
  /** Cache pixels per metre. */
  q: number;
  /** Cache pixel → world matrix (compose with a world→screen matrix to draw it). */
  toWorld: Mat;
}

/** Largest road cache side, px (keeps memory near 25 MB for Port Vesper). */
export const ROAD_CACHE_MAX_PX = 2600;

/** Cache pixels per metre for a city of `bounds`. */
export function cacheScale(bounds: readonly [number, number, number, number], maxPx = ROAD_CACHE_MAX_PX): number {
  const span = Math.max(bounds[2] - bounds[0], bounds[3] - bounds[1], 1) + 80;
  return Math.min(1, maxPx / span);
}

/** Draw the city's roads once into an offscreen canvas (minimap). */
export function buildRoadCache(map: MapStatic): RoadCache {
  const pad = 40;
  const q = cacheScale(map.bounds);
  const x0 = map.bounds[0] - pad;
  const y1 = map.bounds[3] + pad;
  const w = Math.ceil((map.bounds[2] - map.bounds[0] + 2 * pad) * q);
  const h = Math.ceil((map.bounds[3] - map.bounds[1] + 2 * pad) * q);
  const canvas = makeCanvas(Math.max(1, w), Math.max(1, h));
  const ctx = canvas.getContext("2d") as Ctx | null;
  const worldToCache: Mat = [q, 0, 0, -q, -x0 * q, y1 * q];
  if (ctx) drawRoads(ctx, map, worldToCache, q, [1, 1, 0.8, 0.6, 0.5]);
  return { canvas, x0, y1, q, toWorld: [1 / q, 0, 0, -1 / q, x0, y1] };
}

/** Blink phase of a unit's light bar (alternating red / blue, offset per unit). */
export function lightColor(u: MapUnit, nowMs: number, pal: MapPalette): string {
  const phase = Math.floor(nowMs / 140 + u.id * 1.7) % 2;
  return phase ? pal.policeA : pal.policeB;
}

/** Draw a police unit glyph centred at `(x, y)` screen px, rotated by `rot` (canvas radians),
 * `s` = size px. Lights on: bright alternating red/blue with a halo; patrol: steady dim blue. */
export function drawUnit(ctx: Ctx, u: MapUnit, x: number, y: number, rot: number, s: number, nowMs: number, pal: MapPalette): void {
  const blink = unitBlinks(u);
  const col = u.wrecked ? pal.ink3 : blink ? lightColor(u, nowMs, pal) : pal.policeB;
  ctx.save();
  ctx.translate(x, y);
  if (blink) {
    ctx.globalAlpha = 0.35;
    ctx.fillStyle = col;
    ctx.beginPath();
    ctx.arc(0, 0, s * 1.25, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = 1;
  }
  ctx.rotate(rot);
  ctx.fillStyle = col;
  ctx.strokeStyle = "#000";
  ctx.lineWidth = Math.max(1, s * 0.15);
  ctx.globalAlpha = u.wrecked ? 0.5 : blink ? 1 : 0.8;
  if (u.heli) {
    ctx.beginPath();
    ctx.moveTo(0, -s);
    ctx.lineTo(s * 0.7, 0);
    ctx.lineTo(0, s);
    ctx.lineTo(-s * 0.7, 0);
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
    ctx.globalAlpha = 0.7;
    ctx.strokeStyle = col;
    ctx.lineWidth = Math.max(1, s * 0.12);
    ctx.beginPath();
    ctx.arc(0, 0, s * 1.6, 0, Math.PI * 2);
    ctx.stroke();
  } else {
    ctx.beginPath();
    ctx.arc(0, 0, s * 0.62, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
    // heading tick
    ctx.beginPath();
    ctx.moveTo(0, -s * 1.15);
    ctx.lineTo(s * 0.38, -s * 0.5);
    ctx.lineTo(-s * 0.38, -s * 0.5);
    ctx.closePath();
    ctx.fill();
  }
  ctx.restore();
}

/** The player arrow at `(x, y)`, rotated by `rot`, size `s` px. */
export function drawPlayer(ctx: Ctx, x: number, y: number, rot: number, s: number, pal: MapPalette): void {
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(rot);
  ctx.beginPath();
  ctx.moveTo(0, -s);
  ctx.lineTo(s * 0.72, s * 0.72);
  ctx.lineTo(0, s * 0.36);
  ctx.lineTo(-s * 0.72, s * 0.72);
  ctx.closePath();
  ctx.fillStyle = pal.scan;
  ctx.strokeStyle = "#000";
  ctx.lineWidth = Math.max(1.5, s * 0.16);
  ctx.stroke();
  ctx.fill();
  ctx.restore();
}

/**
 * A race opponent at `(x, y)` screen px, rotated by `rot`, size `s` px: a violet arrow (the
 * player's shape, smaller) with its race position in a tag beside it. The leader is drawn
 * larger with a light ring so the race front is easy to find.
 */
export function drawRacer(ctx: Ctx, r: MapRacer, x: number, y: number, rot: number, s: number, pal: MapPalette): void {
  const leader = r.position === 1;
  const size = leader ? s * 1.2 : s;
  ctx.save();
  ctx.translate(x, y);
  if (leader) {
    ctx.beginPath();
    ctx.arc(0, 0, size * 1.45, 0, Math.PI * 2);
    ctx.strokeStyle = pal.ink;
    ctx.globalAlpha = 0.85;
    ctx.lineWidth = Math.max(1.2, size * 0.16);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
  ctx.save();
  ctx.rotate(rot);
  ctx.beginPath();
  ctx.moveTo(0, -size);
  ctx.lineTo(size * 0.72, size * 0.72);
  ctx.lineTo(0, size * 0.36);
  ctx.lineTo(-size * 0.72, size * 0.72);
  ctx.closePath();
  ctx.fillStyle = pal.racer;
  ctx.strokeStyle = "#000";
  ctx.lineWidth = Math.max(1.2, size * 0.16);
  ctx.stroke();
  ctx.fill();
  ctx.restore();
  // Position tag up-right of the arrow (screen-aligned, never rotated).
  const fs = Math.max(9, Math.round(size * 1.45));
  ctx.font = `bold ${fs}px ui-monospace, Menlo, Consolas, monospace`;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const label = String(r.position);
  const tx = size * 0.95;
  const ty = -size * 0.95;
  const tw = ctx.measureText(label).width;
  ctx.fillStyle = "rgba(0,0,0,0.75)";
  ctx.fillRect(tx - 2, ty - fs * 0.6, tw + 4, fs * 1.2);
  ctx.fillStyle = leader ? pal.ink : pal.racer;
  ctx.fillText(label, tx, ty);
  ctx.restore();
}

/** Roadblock (thick red/white barrier line) or spike strip (yellow teeth), world matrix `m`. */
export function drawTool(ctx: Ctx, t: MapTool, m: Mat, k: number, pal: MapPalette): void {
  const a = apply(m, t.x - t.dir[0] * t.halfLenM, t.y - t.dir[1] * t.halfLenM);
  const b = apply(m, t.x + t.dir[0] * t.halfLenM, t.y + t.dir[1] * t.halfLenM);
  ctx.save();
  ctx.lineCap = "butt";
  if (t.kind === "roadblock") {
    const w = Math.max(3, 3 * k);
    ctx.strokeStyle = "#000";
    ctx.lineWidth = w + 2;
    line(ctx, a, b);
    ctx.strokeStyle = pal.danger;
    ctx.lineWidth = w;
    line(ctx, a, b);
    ctx.strokeStyle = "#fff";
    ctx.setLineDash([w, w]);
    ctx.lineWidth = w * 0.5;
    line(ctx, a, b);
  } else {
    const w = Math.max(2, 1.5 * k);
    ctx.strokeStyle = "#000";
    ctx.lineWidth = w + 2;
    line(ctx, a, b);
    ctx.strokeStyle = pal.warn;
    ctx.setLineDash([w * 0.8, w * 0.6]);
    ctx.lineWidth = w * 1.6;
    line(ctx, a, b);
  }
  ctx.restore();
}

function line(ctx: Ctx, a: [number, number], b: [number, number]): void {
  ctx.beginPath();
  ctx.moveTo(a[0], a[1]);
  ctx.lineTo(b[0], b[1]);
  ctx.stroke();
}

/** POI icon at `(x, y)` screen px, size `s`. */
export function drawPoi(ctx: Ctx, kind: MapPoi["kind"], x: number, y: number, s: number, pal: MapPalette): void {
  ctx.save();
  ctx.translate(x, y);
  ctx.lineWidth = Math.max(1, s * 0.14);
  ctx.strokeStyle = "#000";
  switch (kind) {
    case "Garage":
      ctx.fillStyle = pal.warn;
      ctx.fillRect(-s * 0.6, -s * 0.6, s * 1.2, s * 1.2);
      ctx.strokeRect(-s * 0.6, -s * 0.6, s * 1.2, s * 1.2);
      glyph(ctx, "G", s, "#000");
      break;
    case "Safehouse":
      ctx.fillStyle = pal.scan;
      ctx.beginPath();
      ctx.moveTo(0, -s * 0.75);
      ctx.lineTo(s * 0.7, -s * 0.1);
      ctx.lineTo(s * 0.7, s * 0.65);
      ctx.lineTo(-s * 0.7, s * 0.65);
      ctx.lineTo(-s * 0.7, -s * 0.1);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
      break;
    case "PoliceStation":
      ctx.fillStyle = pal.policeB;
      ctx.beginPath();
      ctx.moveTo(-s * 0.65, -s * 0.65);
      ctx.lineTo(s * 0.65, -s * 0.65);
      ctx.lineTo(s * 0.65, s * 0.1);
      ctx.lineTo(0, s * 0.75);
      ctx.lineTo(-s * 0.65, s * 0.1);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
      glyph(ctx, "P", s * 0.9, "#fff");
      break;
    case "Helipad":
      ctx.strokeStyle = pal.policeB;
      ctx.beginPath();
      ctx.arc(0, 0, s * 0.6, 0, Math.PI * 2);
      ctx.stroke();
      glyph(ctx, "H", s * 0.8, pal.policeB);
      break;
    case "RaceStartGrid":
      ctx.fillStyle = pal.ink;
      ctx.fillRect(-s * 0.5, -s * 0.5, s * 0.5, s * 0.5);
      ctx.fillRect(0, 0, s * 0.5, s * 0.5);
      ctx.strokeStyle = pal.ink;
      ctx.strokeRect(-s * 0.5, -s * 0.5, s, s);
      break;
  }
  ctx.restore();
}

function glyph(ctx: Ctx, text: string, s: number, color: string): void {
  ctx.fillStyle = color;
  ctx.font = `bold ${Math.round(s)}px ui-monospace, Menlo, Consolas, monospace`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(text, 0, s * 0.05);
}

/**
 * Police sight areas while the player is wanted, with world matrix `m` (scale `k` px/m): one
 * union path of every ground unit's line-of-sight circle (they see all round, buildings
 * permitting), the helicopter's spotlight on the ground and its range ring, and each
 * intercepting unit's target (a dashed line and a small search ring).
 */
export function drawSight(ctx: Ctx, f: MapFrame, m: Mat, k: number, pal: MapPalette): void {
  ctx.save();
  ctx.beginPath();
  for (const u of f.units) {
    if (u.heli || u.wrecked || u.state === "out") continue;
    const [x, y] = apply(m, u.x, u.y);
    ctx.moveTo(x + f.carSightM * k, y);
    ctx.arc(x, y, f.carSightM * k, 0, Math.PI * 2);
  }
  ctx.fillStyle = pal.policeB;
  ctx.globalAlpha = 0.09;
  ctx.fill("nonzero");
  ctx.globalAlpha = 1;
  for (const u of f.units) {
    if (u.wrecked) continue;
    const [x, y] = apply(m, u.x, u.y);
    if (u.heli) {
      ctx.setLineDash([4, 4]);
      ctx.strokeStyle = pal.policeB;
      ctx.globalAlpha = 0.5;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.arc(x, y, f.heliSightM * k, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
      if (u.target) {
        const [tx, ty] = apply(m, u.target[0], u.target[1]);
        ctx.globalAlpha = 0.28;
        ctx.fillStyle = "#fffbe0";
        const rs = Math.max(4, f.heliSpotM * k);
        ctx.beginPath();
        ctx.arc(tx, ty, rs, 0, Math.PI * 2);
        ctx.fill();
      }
    } else if (u.state === "intercept" && u.target) {
      const [tx, ty] = apply(m, u.target[0], u.target[1]);
      ctx.globalAlpha = 0.6;
      ctx.strokeStyle = pal.policeB;
      ctx.setLineDash([3, 4]);
      ctx.lineWidth = 1;
      line(ctx, [x, y], [tx, ty]);
      ctx.setLineDash([]);
      ctx.beginPath();
      ctx.arc(tx, ty, Math.max(3, 25 * k), 0, Math.PI * 2);
      ctx.stroke();
    } else if (u.state === "pursue" && u.target) {
      // A chasing unit routing round by the roads: where its route meets the racer's path
      // (row 129). Dotted line and a diamond, to tell it from an interceptor's dashed circle.
      const [tx, ty] = apply(m, u.target[0], u.target[1]);
      ctx.globalAlpha = 0.6;
      ctx.strokeStyle = pal.policeB;
      ctx.setLineDash([1, 3]);
      ctx.lineWidth = 1;
      line(ctx, [x, y], [tx, ty]);
      ctx.setLineDash([]);
      const r = Math.max(3, 18 * k);
      ctx.beginPath();
      ctx.moveTo(tx, ty - r);
      ctx.lineTo(tx + r, ty);
      ctx.lineTo(tx, ty + r);
      ctx.lineTo(tx - r, ty);
      ctx.closePath();
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }
  ctx.restore();
}

/** The search zone (amber, dashed) around the last known position, world matrix `m`. */
export function drawSearch(ctx: Ctx, f: MapFrame, m: Mat, k: number, nowMs: number, pal: MapPalette): void {
  if (!f.search) return;
  const [x, y] = apply(m, f.search.x, f.search.y);
  const r = f.search.r * k;
  ctx.save();
  ctx.fillStyle = pal.warn;
  ctx.globalAlpha = 0.1;
  ctx.beginPath();
  ctx.arc(x, y, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.globalAlpha = 0.85;
  ctx.strokeStyle = pal.warn;
  ctx.lineWidth = 1.5;
  ctx.setLineDash([6, 5]);
  ctx.lineDashOffset = -nowMs / 60;
  ctx.stroke();
  ctx.setLineDash([]);
  // last-known cross
  ctx.lineWidth = 2;
  line(ctx, [x - 5, y - 5], [x + 5, y + 5]);
  line(ctx, [x - 5, y + 5], [x + 5, y - 5]);
  ctx.restore();
}
