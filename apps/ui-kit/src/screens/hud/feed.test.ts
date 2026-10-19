import { describe, expect, it } from "vitest";
import type { ScannerEntry } from "../../model/types";
import { FEED_MAX_LINES, FEED_SHOW_S, feedOpacity, visibleFeed } from "./feed";

const entry = (id: number, tS: number, channel: ScannerEntry["channel"] = "dispatch"): ScannerEntry => ({
  id,
  tick: Math.round(tS * 240),
  tS,
  channel,
  severity: "info",
  text: `line ${id}`,
});

describe("HUD radio feed", () => {
  it("shows at most three lines, newest last", () => {
    const entries = [0, 1, 2, 3, 4].map((k) => entry(k, k * 0.5));
    const v = visibleFeed(entries, 2.5);
    expect(v.length).toBe(FEED_MAX_LINES);
    expect(v.map((l) => l.entry.id)).toEqual([2, 3, 4]);
  });

  it("fades lines out and drops them after the show time", () => {
    expect(feedOpacity(0)).toBe(1);
    expect(feedOpacity(FEED_SHOW_S - 1)).toBeGreaterThan(0);
    expect(feedOpacity(FEED_SHOW_S - 1)).toBeLessThan(1);
    expect(feedOpacity(FEED_SHOW_S)).toBe(0);
    expect(visibleFeed([entry(1, 0)], FEED_SHOW_S + 0.1)).toEqual([]);
  });

  it("filters by channel and ignores lines from the future (replay scrubbing)", () => {
    const entries = [entry(1, 0, "civil"), entry(2, 1, "unit"), entry(3, 9, "dispatch")];
    const v = visibleFeed(entries, 2, (e) => e.channel !== "civil");
    expect(v.map((l) => l.entry.id)).toEqual([2]);
  });
});
