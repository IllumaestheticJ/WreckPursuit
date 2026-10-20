import { describe, expect, it } from "vitest";
import { defaultGameSettings, loadGameSettings, normalizeSettings, saveGameSettings, SETTINGS_KEY, speedIn, type SettingsStorage } from "./settings";
import { keyAction, PadNav, REPEAT_DELAY_MS, REPEAT_EVERY_MS, stepIndex } from "./nav";

const memory = (): SettingsStorage & { data: Map<string, string> } => {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
};

describe("game settings", () => {
  it("round-trips through storage", () => {
    const st = memory();
    const s = { ...defaultGameSettings(false), units: "mph" as const, gfx: "low" as const };
    s.volumes.engine = 0.25;
    expect(saveGameSettings(s, st)).toBe(true);
    expect(loadGameSettings(defaultGameSettings(true), st)).toEqual(s);
  });
  it("falls back field by field on bad data", () => {
    const base = defaultGameSettings(true);
    const n = normalizeSettings({ gfx: "ultra", units: "mph", drawDistanceM: 123, volumes: { master: 7, engine: "loud" }, assists: { abs: false, countersteer: -1 } }, base);
    expect(n.gfx).toBe("auto");
    expect(n.units).toBe("mph");
    expect(n.drawDistanceM).toBe(0);
    expect(n.volumes.master).toBe(1);
    expect(n.volumes.engine).toBe(base.volumes.engine);
    expect(n.assists.abs).toBe(false);
    expect(n.assists.countersteer).toBe(0);
    expect(n.developer).toBe(true);
  });
  it("never throws on corrupt or blocked storage", () => {
    const st = memory();
    st.data.set(SETTINGS_KEY, "{not json");
    expect(loadGameSettings(defaultGameSettings(), st)).toEqual(defaultGameSettings());
    const blocked: SettingsStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(loadGameSettings(defaultGameSettings(), blocked)).toEqual(defaultGameSettings());
    expect(saveGameSettings(defaultGameSettings(), blocked)).toBe(false);
  });
  it("converts speed units", () => {
    expect(speedIn("kmh", 10)).toBeCloseTo(36);
    expect(speedIn("mph", 44.704)).toBeCloseTo(100, 2);
  });
});

describe("menu navigation", () => {
  it("maps keys", () => {
    expect(keyAction("ArrowDown")).toBe("down");
    expect(keyAction("Enter")).toBe("activate");
    expect(keyAction("Escape")).toBe("back");
    expect(keyAction("KeyQ")).toBeNull();
  });
  it("steps and wraps", () => {
    expect(stepIndex(-1, 3, 1)).toBe(0);
    expect(stepIndex(-1, 3, -1)).toBe(2);
    expect(stepIndex(2, 3, 1)).toBe(0);
    expect(stepIndex(0, 0, 1)).toBe(-1);
  });
  it("pad: edges, held-at-open ignored, direction repeat", () => {
    const b = (i: number) => Array.from({ length: 17 }, (_, k) => (k === i ? 1 : 0));
    const nav = new PadNav();
    nav.rest(b(9), [0, 0]); // Start held while the pause menu opens
    expect(nav.update(b(9), [0, 0], 0)).toEqual([]);
    expect(nav.update(b(-1), [0, 0], 10)).toEqual([]);
    expect(nav.update(b(9), [0, 0], 20)).toEqual(["start"]);
    expect(nav.update(b(-1), [0, 0.9], 30)).toEqual(["down"]);
    expect(nav.update(b(-1), [0, 0.9], 30 + REPEAT_DELAY_MS - 1)).toEqual([]);
    expect(nav.update(b(-1), [0, 0.9], 30 + REPEAT_DELAY_MS)).toEqual(["down"]);
    expect(nav.update(b(-1), [0, 0.9], 30 + REPEAT_DELAY_MS + REPEAT_EVERY_MS)).toEqual(["down"]);
    expect(nav.update(b(0), [0, 0], 1000)).toEqual(["activate"]);
    expect(nav.update(b(0), [0, 0], 2000)).toEqual([]); // buttons do not repeat
  });
});
