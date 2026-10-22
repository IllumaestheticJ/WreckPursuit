import { describe, expect, it } from "vitest";
import { assistLevel, defaultGameSettings, loadGameSettings, normalizeSettings, saveGameSettings, SETTINGS_KEY, speedIn, type SettingsStorage } from "./settings";
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
  it("reads TCS / ABS levels, legacy bools and junk", () => {
    const base = defaultGameSettings(true);
    expect(base.assists.abs).toBe("Full");
    expect(base.assists.tcs).toBe("Off");
    const legacy = normalizeSettings({ assists: { abs: true, tcs: true } }, base);
    expect([legacy.assists.abs, legacy.assists.tcs]).toEqual(["Full", "Full"]);
    const half = normalizeSettings({ assists: { abs: "half", tcs: "Half" } }, base);
    expect([half.assists.abs, half.assists.tcs]).toEqual(["Half", "Half"]);
    const junk = normalizeSettings({ assists: { abs: "max", tcs: 3 } }, base);
    expect([junk.assists.abs, junk.assists.tcs]).toEqual(["Full", "Off"]);
    expect(assistLevel(false, "Full")).toBe("Off");
  });
  it("countersteer 30 % by default; migrates the old untouched 50 % and 0 (rows 137 / 142)", () => {
    const base = defaultGameSettings(true);
    expect(base.assists.countersteer).toBe(0.3);
    // Saved before row 137 (no mark): the old 50 % reads as 30 %, a changed value stays.
    expect(normalizeSettings({ assists: { countersteer: 0.5 } }, base).assists.countersteer).toBe(0.3);
    expect(normalizeSettings({ assists: { countersteer: 0.8 } }, base).assists.countersteer).toBe(0.8);
    // Saved by 0.12.44 (mark 137): its untouched 0 reads as 30 %, a changed value stays.
    expect(normalizeSettings({ countersteerRev: 137, assists: { countersteer: 0 } }, base).assists.countersteer).toBe(0.3);
    expect(normalizeSettings({ countersteerRev: 137, assists: { countersteer: 0.5 } }, base).assists.countersteer).toBe(0.5);
    // Saved now: 0 is the player's choice and survives a save / load.
    const st = memory();
    saveGameSettings({ ...base, assists: { ...base.assists, countersteer: 0 } }, st);
    expect(loadGameSettings(base, st).assists.countersteer).toBe(0);
  });
  it("falls back field by field on bad data", () => {
    const base = defaultGameSettings(true);
    const n = normalizeSettings({ gfx: "ultra", units: "mph", drawDistanceM: 123, volumes: { master: 7, engine: "loud" }, assists: { abs: false, countersteer: -1 } }, base);
    expect(n.gfx).toBe("auto");
    expect(n.units).toBe("mph");
    expect(n.drawDistanceM).toBe(0);
    expect(n.volumes.master).toBe(1);
    expect(n.volumes.engine).toBe(base.volumes.engine);
    expect(n.assists.abs).toBe("Off");
    expect(n.assists.countersteer).toBe(0);
    expect(n.developer).toBe(true);
    expect(n.radioVoice).toBe(true);
  });
  it("keeps the police radio voice switch (row 79)", () => {
    const base = defaultGameSettings(true);
    expect(base.radioVoice).toBe(true);
    expect(normalizeSettings({ radioVoice: false }, base).radioVoice).toBe(false);
    expect(normalizeSettings({ radioVoice: "no" }, base).radioVoice).toBe(true);
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
