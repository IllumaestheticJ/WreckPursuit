import { beforeEach, describe, expect, it } from "vitest";
import { clampStep, isStockTune, loadQuickTunes, normalizeTune, QUICK_TUNE_KEY, quickTune, saveQuickTune, STOCK_TUNE, TUNE_SLIDERS, tuneSummary } from "./tune";

/** Minimal in-memory localStorage for the node test environment. */
class MemStorage {
  private m = new Map<string, string>();
  getItem(k: string) {
    return this.m.has(k) ? this.m.get(k)! : null;
  }
  setItem(k: string, v: string) {
    this.m.set(k, String(v));
  }
  removeItem(k: string) {
    this.m.delete(k);
  }
  clear() {
    this.m.clear();
  }
}

describe("tune helpers", () => {
  it("has four 5-step sliders matching the Rust fields", () => {
    expect(TUNE_SLIDERS.map((s) => s.key)).toEqual(["suspension", "gearing", "diff", "brake_balance"]);
    for (const s of TUNE_SLIDERS) expect(s.steps[2]).toBe("Stock");
  });

  it("clamps and fills missing sliders with stock", () => {
    expect(clampStep(0)).toBe(1);
    expect(clampStep(9)).toBe(5);
    expect(clampStep(2.6)).toBe(3);
    expect(clampStep(Number.NaN)).toBe(3);
    expect(normalizeTune({ gearing: 5 })).toEqual({ ...STOCK_TUNE, gearing: 5 });
    expect(normalizeTune(null)).toEqual(STOCK_TUNE);
  });

  it("summarises only the moved sliders", () => {
    expect(isStockTune(STOCK_TUNE)).toBe(true);
    expect(tuneSummary(STOCK_TUNE)).toBe("Stock");
    expect(tuneSummary({ suspension: 5, gearing: 5, diff: 4, brake_balance: 2 })).toBe("Suspension stiff · Gear ratio long · Tight LSD · Brake balance rearward");
  });
});

describe("quick-run tunes (per viewer)", () => {
  beforeEach(() => {
    (globalThis as { localStorage?: unknown }).localStorage = new MemStorage();
  });

  it("stores per car and forgets a stock tune", () => {
    expect(quickTune("rampart_50")).toEqual(STOCK_TUNE);
    saveQuickTune("rampart_50", { ...STOCK_TUNE, diff: 5 });
    expect(quickTune("rampart_50").diff).toBe(5);
    expect(quickTune("vireo_xs")).toEqual(STOCK_TUNE);
    saveQuickTune("rampart_50", { ...STOCK_TUNE });
    expect(loadQuickTunes()).toEqual({});
  });

  it("survives a corrupt store", () => {
    localStorage.setItem(QUICK_TUNE_KEY, "{not json");
    expect(quickTune("rampart_50")).toEqual(STOCK_TUNE);
    localStorage.setItem(QUICK_TUNE_KEY, JSON.stringify({ rampart_50: { gearing: 42 } }));
    expect(quickTune("rampart_50")).toEqual({ ...STOCK_TUNE, gearing: 5 });
  });
});
