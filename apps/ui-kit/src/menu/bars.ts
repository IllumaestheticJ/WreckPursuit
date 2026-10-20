/**
 * Live input test bars (steering bipolar, pedals 0..1). Shared by the game menu's Controls
 * section and the testbed's controls panel: both render elements carrying `data-bar="steer" |
 * "throttle" | "brake" | "clutch"` (a track holding one `<i>` fill) and, optionally,
 * `data-bar-value="<same>"` text, then call `paintInputBars` every animation frame.
 */
import type { InputProbe } from "./controls";

/** The four channels, in display order. */
export const BAR_CHANNELS = ["steer", "throttle", "brake", "clutch"] as const;

/** One channel id. */
export type BarChannel = (typeof BAR_CHANNELS)[number];

/** Update every bar under `root` from `p` (null = no input: all bars empty). Each track also
 * gets `data-value` (the processed value, 3 decimals) for automation. */
export function paintInputBars(root: ParentNode, p: InputProbe | null): void {
  for (const ch of BAR_CHANNELS) {
    const raw = p ? p[ch] : 0;
    const v = Number.isFinite(raw) ? raw : 0;
    const track = root.querySelector<HTMLElement>(`[data-bar="${ch}"]`);
    const fill = track?.querySelector<HTMLElement>("i");
    if (track) track.dataset.value = v.toFixed(3);
    if (fill) {
      if (ch === "steer") {
        const c = Math.max(-1, Math.min(1, v));
        fill.style.left = `${50 + Math.min(0, c) * 50}%`;
        fill.style.width = `${Math.abs(c) * 50}%`;
      } else {
        fill.style.left = "0";
        fill.style.width = `${Math.max(0, Math.min(1, v)) * 100}%`;
      }
    }
    const text = root.querySelector<HTMLElement>(`[data-bar-value="${ch}"]`);
    if (text) text.textContent = `${Math.round(v * 100)}%`;
  }
}
