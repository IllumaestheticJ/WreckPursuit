// Player HUD preferences (owner-approved features F47 / F48):
//  - HUD presets (Settings → HUD, saved with the game settings): "full", "minimal" (the speed
//    cluster, race / street panels and prompts) and "off" (prompts only, for recording). They
//    only change the player HUD; with the debug tools on everything shows.
//  - Pinned debug readouts: single lines of the debug readout (fps / sim ms, real-time factor,
//    tick + hash, graphics, camera) that stay on screen with the debug tools off. Picked in the
//    developer drawer's "pin" row, saved per viewer (`wp.testbed.pins.v1`).
//
// Prompts never hide: pursuit outcome / countdown / crash-replay offer / race-spot prompt /
// street challenge picker / menus.

import { store } from "../settings";

/** HUD preset ids (ui-kit `GameSettings.hudPreset`). */
export type HudPreset = "full" | "minimal" | "off";

/** Apply a HUD preset (body classes `wp-hud-minimal` / `wp-hud-off`, CSS in index.html). */
export function applyHudPreset(preset: HudPreset): void {
  const body = document.body;
  body.classList.toggle("wp-hud-minimal", preset === "minimal");
  body.classList.toggle("wp-hud-off", preset === "off");
  body.dataset.hud = preset;
}

/** Debug readout lines that can be pinned. */
export const PINS = [
  { id: "perf", label: "fps · sim / render ms" },
  { id: "rtf", label: "real-time factor" },
  { id: "tick", label: "tick · clock · hash" },
  { id: "gfx", label: "graphics quality" },
  { id: "cam", label: "camera" },
] as const;
export type PinId = (typeof PINS)[number]["id"];

const PINS_KEY = "wp.testbed.pins.v1";

function loadPins(): Set<PinId> {
  try {
    const v = JSON.parse(store.get(PINS_KEY) ?? "[]") as unknown;
    return new Set(Array.isArray(v) ? (v.filter((x) => PINS.some((p) => p.id === x)) as PinId[]) : []);
  } catch {
    return new Set();
  }
}

const pinned = loadPins();

/** The pinned readouts (saved per viewer). */
export const pins = {
  has: (id: PinId): boolean => pinned.has(id),
  toggle(id: PinId, on = !pinned.has(id)): void {
    if (on) pinned.add(id);
    else pinned.delete(id);
    store.set(PINS_KEY, JSON.stringify([...pinned]));
  },
};

/** The developer drawer's "pin" row (`#pin-row`): one checkbox per readout. */
export function mountPinRow(drawer: HTMLElement, before: Element | null): HTMLElement {
  const row = document.createElement("div");
  row.className = "row";
  row.id = "pin-row";
  row.title = "keep these readout lines on screen with the debug tools off";
  row.append("pin");
  for (const p of PINS) {
    const label = document.createElement("label");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.dataset.pin = p.id;
    box.checked = pins.has(p.id);
    box.onchange = () => pins.toggle(p.id, box.checked);
    label.append(box, p.label);
    row.append(label);
  }
  if (before) drawer.insertBefore(row, before);
  else drawer.append(row);
  return row;
}
