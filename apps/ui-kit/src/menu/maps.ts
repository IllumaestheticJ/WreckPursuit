/**
 * Map choice of the main menu (owner, 2026-10-23: "when do I get to see Ashpeak?"). The host
 * lists the maps (`GameMenuOptions.maps`); the menu shows a "Map" picker on the main screen and
 * hands the chosen map id to `GameMenuHost.play`. The choice is remembered per browser
 * (localStorage, never throws: private mode or blocked storage just forget it).
 */
import type { SettingsStorage } from "./settings";

/** One map the main menu offers. */
export interface MenuMap {
  /** City id the host starts sessions in (e.g. `vesper_v2`, `ashpeak`). */
  id: string;
  /** Name shown in the picker. */
  label: string;
  /** One line under the picker, e.g. a large-download note. */
  note?: string;
}

/** localStorage key of the remembered map id. */
export const MAP_KEY = "wp.menu.map.v1";

function browserStorage(): SettingsStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** The remembered map if it is one of `maps`, else the first map ("" without maps). Never throws. */
export function loadMenuMap(maps: MenuMap[], storage: SettingsStorage | null = browserStorage()): string {
  const first = maps[0]?.id ?? "";
  try {
    const id = storage?.getItem(MAP_KEY);
    return id && maps.some((m) => m.id === id) ? id : first;
  } catch {
    return first;
  }
}

/** Remember `id`. Returns false when storage is unavailable (the choice still applies this page). */
export function saveMenuMap(id: string, storage: SettingsStorage | null = browserStorage()): boolean {
  try {
    if (!storage) return false;
    storage.setItem(MAP_KEY, id);
    return true;
  } catch {
    return false;
  }
}
