import { describe, expect, it } from "vitest";
import {
  angleDelta,
  apply,
  clampToCircle,
  compose,
  fitBounds,
  fullMatrix,
  fullToWorld,
  glyphRotation,
  MINIMAP_ZOOM,
  minimapMatrix,
  minimapToWorld,
  pan,
  radiusForSpeed,
  smooth,
  worldToFull,
  worldToMinimap,
  zoomAt,
  type MinimapView,
} from "./projection";

const close = (a: number[], b: number[], eps = 1e-6) => a.forEach((v, i) => expect(v).toBeCloseTo(b[i], 6 - Math.log10(eps / 1e-6)));

describe("minimap projection (heading up)", () => {
  const base: MinimapView = { cx: 100, cy: 50, yaw: 0, pxPerM: 2, sx: 120, sy: 120 };

  it("puts the player at the centre", () => {
    close(worldToMinimap(base, 100, 50), [120, 120]);
    close(worldToMinimap({ ...base, yaw: 1.3 }, 100, 50), [120, 120]);
  });

  it("north-facing: north is up, east is right", () => {
    close(worldToMinimap(base, 100, 60), [120, 100]); // 10 m north → 20 px up
    close(worldToMinimap(base, 110, 50), [140, 120]); // 10 m east → 20 px right
  });

  it("whatever the yaw, the player's forward is screen up and right is screen right", () => {
    for (const yaw of [0.4, Math.PI / 2, -2.1, Math.PI]) {
      const v = { ...base, yaw };
      const f = [-Math.sin(yaw), Math.cos(yaw)];
      const r = [Math.cos(yaw), Math.sin(yaw)];
      close(worldToMinimap(v, 100 + f[0] * 10, 50 + f[1] * 10), [120, 100]);
      close(worldToMinimap(v, 100 + r[0] * 10, 50 + r[1] * 10), [140, 120]);
    }
  });

  it("facing west (yaw +π/2): north is on the right", () => {
    const v = { ...base, yaw: Math.PI / 2 };
    close(worldToMinimap(v, 100, 60), [140, 120]);
    close(worldToMinimap(v, 90, 50), [120, 100]); // west = forward = up
  });

  it("inverse round-trips", () => {
    const v = { ...base, yaw: -0.77, pxPerM: 0.37 };
    for (const [x, y] of [
      [0, 0],
      [-340, 812],
      [100, 50],
    ]) {
      const [sx, sy] = worldToMinimap(v, x, y);
      close(minimapToWorld(v, sx, sy), [x, y]);
    }
  });

  it("canvas matrix matches the function", () => {
    const v = { ...base, yaw: 2.2, pxPerM: 0.8 };
    const m = minimapMatrix(v);
    for (const [x, y] of [
      [0, 0],
      [250, -90],
      [-31, 777],
    ]) {
      close(apply(m, x, y), worldToMinimap(v, x, y));
    }
  });

  it("composes with a cache→world matrix", () => {
    const v = { ...base, yaw: 0.5 };
    const q = 0.6;
    const toWorld: [number, number, number, number, number, number] = [1 / q, 0, 0, -1 / q, -500, 900];
    const cm = compose(minimapMatrix(v), toWorld);
    const [wx, wy] = apply(toWorld, 30, 40);
    close(apply(cm, 30, 40), worldToMinimap(v, wx, wy));
  });

  it("glyph rotation: a unit heading the same way as the player points up", () => {
    expect(glyphRotation(0.7, 0.7)).toBe(0);
    // A unit facing west on a north-up map turns counter-clockwise (negative canvas angle).
    expect(glyphRotation(Math.PI / 2, 0)).toBeCloseTo(-Math.PI / 2);
  });
});

describe("minimap zoom", () => {
  it("scales with speed and clamps", () => {
    expect(radiusForSpeed(0)).toBe(MINIMAP_ZOOM.minRadiusM);
    expect(radiusForSpeed(-5)).toBe(radiusForSpeed(5));
    expect(radiusForSpeed(20)).toBeGreaterThan(radiusForSpeed(10));
    expect(radiusForSpeed(1000)).toBe(MINIMAP_ZOOM.maxRadiusM);
  });

  it("smooths toward the target", () => {
    expect(smooth(NaN, 300, 0.016)).toBe(300);
    const s = smooth(160, 480, 0.1);
    expect(s).toBeGreaterThan(160);
    expect(s).toBeLessThan(480);
    expect(smooth(160, 480, 100)).toBeCloseTo(480);
  });

  it("angle delta takes the short way round", () => {
    expect(angleDelta(3, -3)).toBeCloseTo(2 * Math.PI - 6);
    expect(angleDelta(-3, 3)).toBeCloseTo(6 - 2 * Math.PI);
    expect(angleDelta(0.2, 0.5)).toBeCloseTo(0.3);
  });

  it("clamps rim markers to the circle", () => {
    expect(clampToCircle(0, 0, 10, 3, 4)).toEqual({ x: 3, y: 4, outside: false });
    const c = clampToCircle(0, 0, 10, 30, 40);
    expect(c.outside).toBe(true);
    close([c.x, c.y], [6, 8]);
  });
});

describe("full map view (north up)", () => {
  const b: [number, number, number, number] = [-1700, -1300, 1700, 1750];

  it("fits the city with a margin", () => {
    const v = fitBounds(b, 1000, 800, 20);
    const [x0, y0] = worldToFull(v, b[0], b[1]);
    const [x1, y1] = worldToFull(v, b[2], b[3]);
    expect(Math.min(x0, x1)).toBeGreaterThanOrEqual(19.9);
    expect(Math.max(x0, x1)).toBeLessThanOrEqual(980.1);
    expect(Math.min(y0, y1)).toBeGreaterThanOrEqual(19.9);
    expect(Math.max(y0, y1)).toBeLessThanOrEqual(780.1);
    expect(y1).toBeLessThan(y0); // north is up
  });

  it("matrix matches, inverse round-trips", () => {
    const v = fitBounds(b, 1000, 800);
    close(apply(fullMatrix(v), 123, -456), worldToFull(v, 123, -456));
    const [sx, sy] = worldToFull(v, 123, -456);
    close(fullToWorld(v, sx, sy), [123, -456]);
  });

  it("zoom keeps the point under the cursor fixed", () => {
    const v = fitBounds(b, 1000, 800);
    const before = fullToWorld(v, 300, 200);
    const z = zoomAt(v, 300, 200, 2.5);
    expect(z.pxPerM).toBeCloseTo(v.pxPerM * 2.5);
    close(fullToWorld(z, 300, 200), before);
  });

  it("pan moves the content with the pointer", () => {
    const v = fitBounds(b, 1000, 800);
    const p = worldToFull(v, 0, 0);
    const moved = pan(v, 40, -25);
    close(worldToFull(moved, 0, 0), [p[0] + 40, p[1] - 25]);
  });
});
