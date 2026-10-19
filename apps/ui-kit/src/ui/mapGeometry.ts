/**
 * SVG/canvas geometry for the city map. World coordinates are metres, +Y north; SVG is +Y down,
 * so every helper here flips Y (`svgY = -worldY`).
 */
import type { CityMap, Vec2 } from "../model/types";

const pathCache = new WeakMap<CityMap, string>();

/** One SVG path `d` with every road polyline (cached per map object). */
export function roadsPath(map: CityMap): string {
  const hit = pathCache.get(map);
  if (hit) return hit;
  const d = map.roads
    .map((r) => r.points.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(0)} ${(-y).toFixed(0)}`).join(""))
    .join("");
  pathCache.set(map, d);
  return d;
}

/** SVG path `d` for a polygon. */
export function polygonPath(poly: Vec2[]): string {
  return poly.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)} ${(-y).toFixed(1)}`).join("") + "Z";
}

/** Centroid of a polygon (vertex average; fine for convex blocks). */
export function centroid(poly: Vec2[]): Vec2 {
  const n = poly.length || 1;
  return [poly.reduce((s, p) => s + p[0], 0) / n, poly.reduce((s, p) => s + p[1], 0) / n];
}

/** Heading (0 = north, clockwise) → SVG rotation in degrees. */
export function headingDeg(h: number): number {
  return (h * 180) / Math.PI;
}

/** Point-in-polygon (ray casting). */
export function pointInPolygon(p: Vec2, poly: Vec2[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i];
    const [xj, yj] = poly[j];
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** Polygon area, m² (shoelace). */
export function polygonArea(poly: Vec2[]): number {
  let a = 0;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) a += (poly[j][0] + poly[i][0]) * (poly[j][1] - poly[i][1]);
  return Math.abs(a / 2);
}
