/**
 * Pure map math: the heading-up minimap projection, speed-scaled zoom, and the north-up full
 * map view with pan and zoom. World: metres, +X east, +Y north, yaw about +Z with 0 = +Y
 * (north), counter-clockwise. Screen: canvas pixels, +x right, +y down.
 */

/** A 2D affine matrix in canvas `setTransform(a, b, c, d, e, f)` order:
 * `sx = a·x + c·y + e`, `sy = b·x + d·y + f`. */
export type Mat = [number, number, number, number, number, number];

/** Heading-up minimap view. */
export interface MinimapView {
  /** World point at the centre (the player), m. */
  cx: number;
  cy: number;
  /** Player yaw, rad (0 = north, counter-clockwise). */
  yaw: number;
  /** Screen pixels per world metre. */
  pxPerM: number;
  /** Centre on screen, px. */
  sx: number;
  sy: number;
}

/** Forward unit vector of a yaw. */
export function forwardOf(yaw: number): [number, number] {
  return [-Math.sin(yaw), Math.cos(yaw)];
}

/** World → minimap screen. The player's forward points up the screen. */
export function worldToMinimap(v: MinimapView, x: number, y: number): [number, number] {
  const dx = x - v.cx;
  const dy = y - v.cy;
  const c = Math.cos(v.yaw);
  const s = Math.sin(v.yaw);
  // right = (cos, sin), forward = (-sin, cos)
  const r = dx * c + dy * s;
  const f = -dx * s + dy * c;
  return [v.sx + r * v.pxPerM, v.sy - f * v.pxPerM];
}

/** Minimap screen → world (inverse of `worldToMinimap`). */
export function minimapToWorld(v: MinimapView, sx: number, sy: number): [number, number] {
  const r = (sx - v.sx) / v.pxPerM;
  const f = -(sy - v.sy) / v.pxPerM;
  const c = Math.cos(v.yaw);
  const s = Math.sin(v.yaw);
  return [v.cx + r * c - f * s, v.cy + r * s + f * c];
}

/** The `worldToMinimap` transform as a canvas matrix. */
export function minimapMatrix(v: MinimapView): Mat {
  const k = v.pxPerM;
  const c = Math.cos(v.yaw);
  const s = Math.sin(v.yaw);
  const a = k * c;
  const b = k * s;
  const cc = k * s;
  const d = -k * c;
  return [a, b, cc, d, v.sx - (a * v.cx + cc * v.cy), v.sy - (b * v.cx + d * v.cy)];
}

/** Screen-space rotation (canvas `rotate`, clockwise-positive) that turns a north-up glyph
 * into a unit's heading on the heading-up minimap. */
export function glyphRotation(unitYaw: number, viewYaw: number): number {
  return viewYaw - unitYaw;
}

/** Apply a matrix to a point. */
export function apply(m: Mat, x: number, y: number): [number, number] {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}

/** `m1 ∘ m2` (apply `m2` first). */
export function compose(m1: Mat, m2: Mat): Mat {
  return [
    m1[0] * m2[0] + m1[2] * m2[1],
    m1[1] * m2[0] + m1[3] * m2[1],
    m1[0] * m2[2] + m1[2] * m2[3],
    m1[1] * m2[2] + m1[3] * m2[3],
    m1[0] * m2[4] + m1[2] * m2[5] + m1[4],
    m1[1] * m2[4] + m1[3] * m2[5] + m1[5],
  ];
}

/** Speed → world radius shown by the minimap, m: tight in town, wider at highway speed. */
export const MINIMAP_ZOOM = { minRadiusM: 160, maxRadiusM: 480, fullAtMps: 45 } as const;

/** World radius for a speed (m/s), linear between the limits and clamped. */
export function radiusForSpeed(speedMps: number): number {
  const t = Math.min(1, Math.max(0, Math.abs(speedMps) / MINIMAP_ZOOM.fullAtMps));
  return MINIMAP_ZOOM.minRadiusM + (MINIMAP_ZOOM.maxRadiusM - MINIMAP_ZOOM.minRadiusM) * t;
}

/** Exponential smoothing toward `target` over `dtS` with time constant `tauS`. */
export function smooth(current: number, target: number, dtS: number, tauS = 0.6): number {
  if (!Number.isFinite(current)) return target;
  const a = 1 - Math.exp(-Math.max(0, dtS) / tauS);
  return current + (target - current) * a;
}

/** Shortest signed angle `to - from` in (-π, π]. */
export function angleDelta(from: number, to: number): number {
  let d = (to - from) % (2 * Math.PI);
  if (d > Math.PI) d -= 2 * Math.PI;
  if (d <= -Math.PI) d += 2 * Math.PI;
  return d;
}

/** Clamp a screen point to a circle of radius `r` around `(cx, cy)`; returns the point and
 * whether it was outside. */
export function clampToCircle(cx: number, cy: number, r: number, x: number, y: number): { x: number; y: number; outside: boolean } {
  const dx = x - cx;
  const dy = y - cy;
  const d = Math.hypot(dx, dy);
  if (d <= r) return { x, y, outside: false };
  return { x: cx + (dx / d) * r, y: cy + (dy / d) * r, outside: true };
}

// ----- full map (north up) -----------------------------------------------------------------

/** North-up full map view. */
export interface FullView {
  /** World point at the screen centre, m. */
  cx: number;
  cy: number;
  /** Screen pixels per metre. */
  pxPerM: number;
  /** Viewport size, px. */
  w: number;
  h: number;
}

/** Zoom limits of the full map, px per metre. */
export const FULL_ZOOM = { min: 0.05, max: 4 } as const;

/** World → full map screen. */
export function worldToFull(v: FullView, x: number, y: number): [number, number] {
  return [v.w / 2 + (x - v.cx) * v.pxPerM, v.h / 2 - (y - v.cy) * v.pxPerM];
}

/** Full map screen → world. */
export function fullToWorld(v: FullView, sx: number, sy: number): [number, number] {
  return [v.cx + (sx - v.w / 2) / v.pxPerM, v.cy - (sy - v.h / 2) / v.pxPerM];
}

/** The `worldToFull` transform as a canvas matrix. */
export function fullMatrix(v: FullView): Mat {
  const k = v.pxPerM;
  return [k, 0, 0, -k, v.w / 2 - v.cx * k, v.h / 2 + v.cy * k];
}

/** A view that fits `bounds` (`[x0, y0, x1, y1]`) into `w × h` with `marginPx` on each side. */
export function fitBounds(bounds: readonly [number, number, number, number], w: number, h: number, marginPx = 24): FullView {
  const [x0, y0, x1, y1] = bounds;
  const bw = Math.max(1, x1 - x0);
  const bh = Math.max(1, y1 - y0);
  const k = Math.min((w - 2 * marginPx) / bw, (h - 2 * marginPx) / bh);
  return { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, pxPerM: clampZoom(k), w, h };
}

/** Clamp a zoom to `FULL_ZOOM`. */
export function clampZoom(k: number): number {
  return Math.min(FULL_ZOOM.max, Math.max(FULL_ZOOM.min, k));
}

/** Zoom by `factor` keeping the world point under screen `(sx, sy)` fixed. */
export function zoomAt(v: FullView, sx: number, sy: number, factor: number): FullView {
  const [wx, wy] = fullToWorld(v, sx, sy);
  const k = clampZoom(v.pxPerM * factor);
  // Solve worldToFull(new, wx, wy) == (sx, sy) for the new centre.
  return { ...v, pxPerM: k, cx: wx - (sx - v.w / 2) / k, cy: wy + (sy - v.h / 2) / k };
}

/** Pan by a screen drag of `(dx, dy)` px (content follows the pointer). */
export function pan(v: FullView, dxPx: number, dyPx: number): FullView {
  return { ...v, cx: v.cx - dxPx / v.pxPerM, cy: v.cy + dyPx / v.pxPerM };
}

/** Keep the centre inside `bounds` (so the city can't be panned off screen). */
export function clampCentre(v: FullView, bounds: readonly [number, number, number, number]): FullView {
  const [x0, y0, x1, y1] = bounds;
  return { ...v, cx: Math.min(x1, Math.max(x0, v.cx)), cy: Math.min(y1, Math.max(y0, v.cy)) };
}
