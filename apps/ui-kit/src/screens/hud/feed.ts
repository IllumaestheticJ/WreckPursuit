/**
 * HUD radio feed rule (playtest row 6, "the cops are too talkative"): at most
 * {@link FEED_MAX_LINES} lines on screen, each shown for {@link FEED_SHOW_S} s and fading out
 * over the last {@link FEED_FADE_S} s. `wp_police` already paces the radio (data/police/radio.toml);
 * this only keeps the HUD from stacking old lines. The full scanner screen keeps every line.
 */
import type { ScannerEntry } from "../../model/types";

/** Most lines visible at once. */
export const FEED_MAX_LINES = 3;
/** Seconds a line stays on screen. */
export const FEED_SHOW_S = 6;
/** Seconds of fade at the end of {@link FEED_SHOW_S}. */
export const FEED_FADE_S = 2;

/** A visible feed line with its opacity (1 = fresh, → 0 as it expires). */
export interface FeedLine {
  entry: ScannerEntry;
  opacity: number;
}

/** Opacity of a line `ageS` seconds old: 1 until the fade starts, then linear to 0. */
export function feedOpacity(ageS: number): number {
  if (ageS < 0 || ageS >= FEED_SHOW_S) return 0;
  const fadeFrom = FEED_SHOW_S - FEED_FADE_S;
  return ageS <= fadeFrom ? 1 : 1 - (ageS - fadeFrom) / FEED_FADE_S;
}

/** The newest `keep(e)` entries still on screen at `nowS`, oldest first, at most {@link FEED_MAX_LINES}. */
export function visibleFeed(entries: readonly ScannerEntry[], nowS: number, keep: (e: ScannerEntry) => boolean = () => true): FeedLine[] {
  const out: FeedLine[] = [];
  for (let i = entries.length - 1; i >= 0 && out.length < FEED_MAX_LINES; i--) {
    const e = entries[i];
    const age = nowS - e.tS;
    // Entries are in time order: once one is too old, every earlier one is too.
    if (age >= FEED_SHOW_S) break;
    if (age < 0 || !keep(e)) continue;
    out.push({ entry: e, opacity: feedOpacity(age) });
  }
  return out.reverse();
}
