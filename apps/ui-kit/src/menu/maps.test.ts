import { describe, expect, it } from "vitest";
import { loadMenuMap, MAP_KEY, saveMenuMap, type MenuMap } from "./maps";
import type { SettingsStorage } from "./settings";

const MAPS: MenuMap[] = [
  { id: "vesper_v2", label: "Port Vesper" },
  { id: "ashpeak", label: "Port Vesper + Ashpeak County", note: "Large download" },
];

const memory = (): SettingsStorage & { data: Map<string, string> } => {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
};

describe("main menu map choice", () => {
  it("defaults to the first map and remembers the choice", () => {
    const st = memory();
    expect(loadMenuMap(MAPS, st)).toBe("vesper_v2");
    expect(saveMenuMap("ashpeak", st)).toBe(true);
    expect(st.data.get(MAP_KEY)).toBe("ashpeak");
    expect(loadMenuMap(MAPS, st)).toBe("ashpeak");
  });
  it("ignores an unknown stored map and never throws", () => {
    const st = memory();
    st.data.set(MAP_KEY, "atlantis");
    expect(loadMenuMap(MAPS, st)).toBe("vesper_v2");
    expect(loadMenuMap([], st)).toBe("");
    const blocked: SettingsStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(loadMenuMap(MAPS, blocked)).toBe("vesper_v2");
    expect(saveMenuMap("ashpeak", blocked)).toBe(false);
    expect(saveMenuMap("ashpeak", null)).toBe(false);
  });
});
