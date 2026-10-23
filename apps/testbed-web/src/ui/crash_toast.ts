// Crash report toast (owner-approved feature F2): after a real hit on the player's car, a short
// card at the right edge says what happened: closing speed, regions hit (and how badly), new
// component failures / lost parts, and how much the repair quote went up.
//
// It folds the drained SimEvents of one impact window (1 s of sim time from the first hard
// contact or region damage on the player) and shows the toast for 6 s of wall time. Small bumps
// (no region past cosmetic, nothing failed) show nothing. UI only: reads events and the wasm
// repair quote, never steps or changes the sim.

import { billRequestJson, DEFAULT_BILL_REQUEST } from "../../../ui-kit/src/live";
import type { SimEvent } from "../wasm/sim";

/** Contact closing speed that opens an impact window, m/s (≈ 22 km/h). */
const MIN_CLOSING_MPS = 6;
/** Impact window length, sim ticks (1 s at 240 Hz). */
const WINDOW_TICKS = 240;
/** Region severity below which a hit counts as cosmetic. */
const MIN_SEVERITY = 0.08;
/** The toast stays this long, ms. */
const SHOW_MS = 6000;
/** Re-read the "before" repair quote at most this often while nothing happens, ms. */
const QUOTE_EVERY_MS = 2000;

/** What one impact window collected. */
export interface CrashReport {
  /** Highest closing speed of the player's contacts, m/s. */
  closingMps: number;
  /** Regions hit, worst severity each (0..1). */
  regions: Map<string, number>;
  /** Components that failed in the window. */
  failed: string[];
  /** Parts / wheels that came off. */
  lost: number;
  totalLoss: boolean;
  /** Repair quote before and after (player pays), $; null = no quote. */
  quoteBefore: number | null;
  quoteAfter: number | null;
}

/** Readable region name: "FrontLeft" → "front-left". */
export function regionName(r: string): string {
  return r.replace(/([a-z])([A-Z])/g, "$1-$2").toLowerCase();
}

/** Severity word for a region hit. */
export function severityWord(s: number): string {
  return s >= 0.66 ? "wrecked" : s >= 0.33 ? "heavy" : "light";
}

/** Does this window deserve a toast? (Not for scrapes.) */
export function worthShowing(r: CrashReport): boolean {
  return r.totalLoss || r.failed.length > 0 || r.lost > 0 || [...r.regions.values()].some((s) => s >= MIN_SEVERITY);
}

/** The toast's lines (title first), plain text. */
export function reportLines(r: CrashReport, units: "kmh" | "mph" = "kmh"): string[] {
  const speed = units === "mph" ? `${Math.round(r.closingMps * 2.236936)} mph` : `${Math.round(r.closingMps * 3.6)} km/h`;
  const lines = [r.totalLoss ? "TOTAL LOSS" : "CRASH", r.closingMps > 0 ? `closing speed ${speed}` : "hit"];
  const regions = [...r.regions.entries()].filter(([, s]) => s >= MIN_SEVERITY).sort((a, b) => b[1] - a[1]);
  if (regions.length) lines.push(`hit: ${regions.slice(0, 4).map(([n, s]) => `${regionName(n)} (${severityWord(s)})`).join(", ")}`);
  if (r.failed.length) lines.push(`failed: ${r.failed.join(", ")}`);
  if (r.lost) lines.push(`${r.lost} part${r.lost === 1 ? "" : "s"} came off`);
  if (r.quoteAfter !== null) {
    const delta = r.quoteBefore !== null ? r.quoteAfter - r.quoteBefore : null;
    lines.push(`repair ${delta !== null && delta > 0 ? `+$${delta.toLocaleString("en-US")} → ` : ""}$${r.quoteAfter.toLocaleString("en-US")}`);
  }
  return lines;
}

/** The crash report toast (`#crash-toast`). */
export class CrashToast {
  readonly el: HTMLElement;
  /** Toasts shown this page (e2e). */
  count = 0;
  /** The latest report shown (e2e). */
  last: CrashReport | null = null;
  private window: (CrashReport & { endTick: number }) | null = null;
  private hideAt = 0;
  private quote: number | null = null;
  private quoteAt = -Infinity;
  private quoteFor = -1;
  private lastTick = -1;

  constructor(
    parent: HTMLElement,
    /** Player-pays repair quote of `entity` now, $ (null: none). */
    private readonly quoteOf: (entity: number) => number | null,
  ) {
    this.el = document.createElement("div");
    this.el.id = "crash-toast";
    this.el.className = "hidden";
    this.el.setAttribute("role", "status");
    parent.appendChild(this.el);
  }

  /** Forget everything (new session). */
  reset(): void {
    this.window = null;
    this.quote = null;
    this.quoteAt = -Infinity;
    this.hideAt = 0;
    this.el.classList.add("hidden");
  }

  /** One frame: the events drained this frame, the player, sim tick and wall time. */
  frame(events: SimEvent[], player: number, tick: number, nowMs: number, units: "kmh" | "mph" = "kmh"): void {
    // A new session (the tick went back): forget the old window and quote.
    if (tick < this.lastTick) this.reset();
    this.lastTick = tick;
    if (player !== this.quoteFor) {
      this.quoteFor = player;
      this.quote = null;
      this.quoteAt = -Infinity;
    }
    for (const ev of events) this.fold(ev as Record<string, Record<string, unknown>>, player, tick);
    const w = this.window;
    if (w && tick >= w.endTick) {
      this.window = null;
      w.quoteAfter = this.quoteOf(player);
      this.quote = w.quoteAfter;
      this.quoteAt = nowMs;
      if (worthShowing(w)) this.show(w, nowMs, units);
    } else if (!w && nowMs - this.quoteAt > QUOTE_EVERY_MS) {
      // The "before" quote, refreshed now and then while nothing is happening.
      this.quote = this.quoteOf(player);
      this.quoteAt = nowMs;
    }
    if (this.hideAt && nowMs > this.hideAt) {
      this.hideAt = 0;
      this.el.classList.add("hidden");
    }
  }

  private open(tick: number): CrashReport & { endTick: number } {
    this.window ??= { closingMps: 0, regions: new Map(), failed: [], lost: 0, totalLoss: false, quoteBefore: this.quote, quoteAfter: null, endTick: tick + WINDOW_TICKS };
    return this.window;
  }

  private fold(ev: Record<string, Record<string, unknown>>, player: number, tick: number): void {
    const [kind, b] = Object.entries(ev)[0] ?? [];
    if (!kind || !b) return;
    if (kind === "Contact") {
      if ((b.a !== player && b.b !== player) || (b.rel_speed as number) < MIN_CLOSING_MPS) return;
      const w = this.open(tick);
      w.closingMps = Math.max(w.closingMps, b.rel_speed as number);
      return;
    }
    if (b.vehicle !== player) return;
    if (kind === "RegionDamaged") {
      const w = this.open(tick);
      const r = String(b.region);
      w.regions.set(r, Math.max(w.regions.get(r) ?? 0, b.severity as number));
    } else if (kind === "ComponentFailed") {
      const w = this.open(tick);
      const c = String(b.component);
      if (!w.failed.includes(c)) w.failed.push(c);
    } else if (kind === "PartDetached" || kind === "WheelDetached") {
      this.open(tick).lost++;
    } else if (kind === "TotalLoss") {
      this.open(tick).totalLoss = true;
    }
  }

  private show(r: CrashReport, nowMs: number, units: "kmh" | "mph"): void {
    this.last = r;
    this.count++;
    const [title, ...rest] = reportLines(r, units);
    this.el.innerHTML = "";
    const head = document.createElement("b");
    head.textContent = title;
    this.el.append(head);
    for (const line of rest) {
      const div = document.createElement("div");
      div.textContent = line;
      this.el.append(div);
    }
    this.el.dataset.kind = r.totalLoss ? "total" : "crash";
    this.el.classList.remove("hidden");
    this.hideAt = nowMs + SHOW_MS;
  }
}

/** Player-pays repair quote from the wasm bill (the default legit-shop request). */
export function quoteFromSim(repairQuote: (entity: number, requestJson: string) => unknown, entity: number): number | null {
  try {
    const bill = repairQuote(entity, billRequestJson(DEFAULT_BILL_REQUEST)) as { quote?: { player_pays: number } | null } | null;
    // An undamaged car has a bill without a quote: nothing to pay.
    return bill ? (bill.quote ? Math.round(bill.quote.player_pays) : 0) : null;
  } catch {
    return null;
  }
}
