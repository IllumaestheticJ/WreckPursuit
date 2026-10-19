import { describe, expect, it } from "vitest";
import { decodeMapDynamic, DYN, EMPTY_FRAME, isWanted, UNIT_FLAG, unitBlinks } from "./data";

/** Build a map_dynamic() array the way wp_wasm does. */
function dyn(header: Record<number, number>, units: number[][], tools: number[][]): Float32Array {
  const h = new Array(DYN.HEADER).fill(0);
  h[0] = DYN.VERSION;
  h[1] = units.length;
  h[2] = tools.length;
  for (const [k, v] of Object.entries(header)) h[Number(k)] = v;
  return new Float32Array([...h, ...units.flat(), ...tools.flat()]);
}

describe("decodeMapDynamic", () => {
  it("decodes header, units and tools", () => {
    const f = decodeMapDynamic(
      dyn(
        { 3: 3, 4: 2, 5: 10, 6: 20, 7: 140, 8: 0.25, 9: 260, 10: 550, 11: 42, 12: 10, 13: 20, 14: 1, 15: 1, 16: 2, 17: 0.5, 18: 30, 19: 1 },
        [
          [7, 100, 200, 0.1, 25, 3 | UNIT_FLAG.LIGHTS | UNIT_FLAG.ON_PLAYER, 0, 0],
          [8, -50, 60, 0, 0, 1, 0, 0],
          [9, 0, 0, 1.0, 40, 7 | UNIT_FLAG.HELI | UNIT_FLAG.HAS_TARGET, 11, 22],
        ],
        [
          [0, 5, 6, 0, 1, 8],
          [1, 7, 8, 1, 0, 4],
        ],
      ),
    );
    expect(f.status).toBe("searching");
    expect(f.level).toBe(2);
    expect(f.search).toEqual({ x: 10, y: 20, r: 140 });
    expect(f.escape).toBeCloseTo(0.25);
    expect(f.carSightM).toBe(260);
    expect(f.lastKnown).toEqual([10, 20]);
    expect(f.player).toEqual({ x: 1, y: 2, yaw: 0.5, speed: 30 });
    expect(f.units).toHaveLength(3);
    expect(f.units[0]).toMatchObject({ id: 7, state: "pursue", lights: true, onPlayer: true, heli: false, target: null });
    expect(f.units[1]).toMatchObject({ state: "patrol", lights: false });
    expect(f.units[2]).toMatchObject({ state: "track", heli: true, target: [11, 22] });
    expect(unitBlinks(f.units[0])).toBe(true);
    expect(unitBlinks(f.units[1])).toBe(false);
    expect(f.tools).toEqual([
      { kind: "roadblock", x: 5, y: 6, dir: [0, 1], halfLenM: 8 },
      { kind: "spikes", x: 7, y: 8, dir: [1, 0], halfLenM: 4 },
    ]);
    expect(isWanted(f)).toBe(true);
  });

  it("no search zone unless the radius is set; no player flag → null", () => {
    const f = decodeMapDynamic(dyn({ 3: 2 }, [], []));
    expect(f.search).toBeNull();
    expect(f.player).toBeNull();
    expect(f.status).toBe("pursuit");
  });

  it("short arrays are an empty frame; wrong version throws", () => {
    expect(decodeMapDynamic([])).toBe(EMPTY_FRAME);
    const bad = dyn({}, [], []);
    bad[0] = 99;
    expect(() => decodeMapDynamic(bad)).toThrow(/layout/);
  });

  it("a wrecked unit with lights does not blink", () => {
    const f = decodeMapDynamic(dyn({}, [[1, 0, 0, 0, 0, 3 | UNIT_FLAG.LIGHTS | UNIT_FLAG.WRECKED, 0, 0]], []));
    expect(unitBlinks(f.units[0])).toBe(false);
  });
});
