import { describe, expect, it } from "vitest";
import { chaseRadar, decodeMapDynamic, DYN, EMPTY_FRAME, isWanted, RACER_FLAG, racersToDraw, UNIT_FLAG, unitBlinks } from "./data";

/** Build a map_dynamic() array the way wp_wasm does. */
function dyn(header: Record<number, number>, units: number[][], tools: number[][], racers: number[][] = []): Float32Array {
  const h = new Array(DYN.HEADER).fill(0);
  h[0] = DYN.VERSION;
  h[1] = units.length;
  h[2] = tools.length;
  h[20] = racers.length;
  for (const [k, v] of Object.entries(header)) h[Number(k)] = v;
  return new Float32Array([...h, ...units.flat(), ...tools.flat(), ...racers.flat()]);
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
          [3, 9, 9, 1, 0, 2],
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
      { kind: "roadblock", x: 5, y: 6, dir: [0, 1], halfLenM: 8, intel: false },
      { kind: "spikes", x: 7, y: 8, dir: [1, 0], halfLenM: 4, intel: false },
      { kind: "spikes", x: 9, y: 9, dir: [1, 0], halfLenM: 2, intel: true },
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

  it("decodes racers after the units and tools, in position order", () => {
    const f = decodeMapDynamic(
      dyn(
        {},
        [[7, 100, 200, 0.1, 25, 3, 0, 0]],
        [[0, 5, 6, 0, 1, 8]],
        [
          [3, 30, -3, 0.5, 1, 0],
          [1, 10, -1, 0.25, 2, RACER_FLAG.PLAYER],
          [2, 20, -2, 0, 3, RACER_FLAG.FINISHED],
          [4, 40, -4, 0, 4, RACER_FLAG.DNF | RACER_FLAG.CAPTURED],
          [5, 50, -5, 0, 5, RACER_FLAG.DQ],
          [6, 60, -6, 1.5, 6, 0],
        ],
      ),
    );
    expect(f.units).toHaveLength(1);
    expect(f.tools).toHaveLength(1);
    expect(f.racers.map((r) => r.position)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(f.racers[0]).toEqual({ id: 3, x: 30, y: -3, yaw: 0.5, position: 1, finished: false, dnf: false, captured: false, dq: false, player: false });
    expect(f.racers[1]).toMatchObject({ id: 1, player: true });
    expect(f.racers[2]).toMatchObject({ id: 2, finished: true });
    expect(f.racers[3]).toMatchObject({ id: 4, dnf: true, captured: true });
    expect(f.racers[4]).toMatchObject({ id: 5, dq: true });
    // Drawn: still racing and not the player (finished, DNF and DQ cars are hidden).
    expect(racersToDraw(f).map((r) => r.id)).toEqual([3, 6]);
  });

  it("no race → no racers; a truncated racer block stops at the end of the array", () => {
    expect(decodeMapDynamic(dyn({}, [], [])).racers).toEqual([]);
    const a = dyn({}, [], [], [[3, 30, -3, 0.5, 1, 0]]);
    a[20] = 2; // claims two racers, carries one
    expect(decodeMapDynamic(a).racers).toHaveLength(1);
    expect(EMPTY_FRAME.racers).toEqual([]);
  });

  it("a v1 array (no racer block) is rejected", () => {
    const a = dyn({}, [], []);
    a[0] = 1;
    expect(() => decodeMapDynamic(a)).toThrow(/v1, expected v2/);
  });

  it("a wrecked unit with lights does not blink", () => {
    const f = decodeMapDynamic(dyn({}, [[1, 0, 0, 0, 0, 3 | UNIT_FLAG.LIGHTS | UNIT_FLAG.WRECKED, 0, 0]], []));
    expect(unitBlinks(f.units[0])).toBe(false);
  });
});

describe("chaseRadar (PR #32)", () => {
  it("names the helicopter by call sign and tethers it to the player it is on", () => {
    const f = decodeMapDynamic(
      dyn(
        { 3: 2, 15: 100, 16: 200, 17: 0.5, 19: 1 },
        [
          [9, 60, 170, 1.0, 40, 7 | UNIT_FLAG.HELI | UNIT_FLAG.ON_PLAYER | UNIT_FLAG.HAS_TARGET, 95, 205],
          [12, 0, 0, 0, 0, 7 | UNIT_FLAG.HELI | UNIT_FLAG.HAS_TARGET, 11, 22],
          [4, 120, 230, 0.2, 25, 3 | UNIT_FLAG.LIGHTS | UNIT_FLAG.ON_PLAYER, 0, 0],
          [5, 0, 0, 0, 0, 8 | UNIT_FLAG.WRECKED, 0, 0],
        ],
        [],
      ),
    );
    const r = chaseRadar(f)!;
    expect(r.pos).toEqual([100, 200]);
    expect(r.headingRad).toBeCloseTo(-0.5);
    const air = r.blips.find((b) => b.id === 9)!;
    expect(air).toMatchObject({ kind: "helicopter", label: "AIR 9", tracking: [100, 200] });
    // Tracking someone else: tethered to its tracked spot.
    expect(r.blips.find((b) => b.id === 12)).toMatchObject({ kind: "helicopter", tracking: [11, 22] });
    expect(r.blips.find((b) => b.id === 4)?.kind).toBe("police");
    expect(r.blips.find((b) => b.id === 4)?.tracking).toBeUndefined();
    // Out of service / wrecked units are not on the radar.
    expect(r.blips.some((b) => b.id === 5)).toBe(false);
  });

  it("is null without a player or with nothing around", () => {
    expect(chaseRadar(decodeMapDynamic(dyn({ 3: 2 }, [[9, 0, 0, 0, 0, 7 | UNIT_FLAG.HELI, 0, 0]], [])))).toBeNull();
    expect(chaseRadar(decodeMapDynamic(dyn({ 19: 1 }, [], [])))).toBeNull();
  });
});
