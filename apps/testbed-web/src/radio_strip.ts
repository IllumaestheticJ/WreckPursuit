// Police radio strip (owner playtest row 79): the last few transmissions under the chase
// readout, each led by a coloured call-sign chip ("UNIT 12", "AIR 9", "DISPATCH").
//
// Lines arrive when they are *heard*: from the audio scanner's RadioSay (so text and voice line
// up, after the scanner tier's delay), or straight from the police events while the audio is
// not running (before the first click, scanner tier "None", audio failed). wp_police paces the
// radio (one transmission at a time), so the strip never needs to queue.
//
// Priority tiers (owner playtest row 144): tiers 1–2 are highlighted (bold, an icon: ⚠ for
// tier 1 warnings, ▲ for tier 2 escalation) and stay on screen longer; tier 3 (impacts,
// wrecks) gets an urgent red style; tiers 4–5 are dimmed. Tier 0 = unknown (plain).

/** Lines kept on the strip by default (debug tools on). */
const MAX_LINES = 3;
/** A line is fully visible this long, then fades, ms. */
const SHOW_MS = 7000;
/** Tiers 1–2 stay this long, ms. */
const SHOW_IMPORTANT_MS = 12000;

/** Strip icon for a tier ("" = none). */
export function tierIcon(tier: number): string {
  return tier === 1 ? "⚠" : tier === 2 ? "▲" : tier === 3 ? "✸" : "";
}

/** How long a line of `tier` stays fully visible, ms. */
export function showMs(tier: number): number {
  return tier === 1 || tier === 2 ? SHOW_IMPORTANT_MS : SHOW_MS;
}
/** Fade-out length, ms. */
const FADE_MS = 1500;

/** Leading call sign of a radio line and the rest of it. */
export function splitCallsign(text: string): { callsign: string; rest: string } {
  const m = /^(Units? \d+(?:(?:, | and )\d+)*|Air \d+|Dispatch|Captain)\b[,:]?\s*/.exec(text);
  if (!m) return { callsign: "", rest: text };
  return { callsign: m[1], rest: text.slice(m[0].length) };
}

/** Chip colour of a call sign: dispatch amber, captain orange, air cyan, units by number. */
export function callsignColour(callsign: string): string {
  if (callsign === "Dispatch") return "#ffcc00";
  if (callsign === "Captain") return "#ff8a50";
  if (callsign.startsWith("Air")) return "#4dd0e1";
  const n = Number(/\d+/.exec(callsign)?.[0] ?? 0);
  if (callsign.startsWith("Units")) return "#c5cae9";
  // Golden-angle hues: neighbouring unit numbers get clearly different colours.
  return `hsl(${Math.round((n * 137.508) % 360)}, 85%, 68%)`;
}

interface Shown {
  el: HTMLElement;
  born: number;
  show: number;
}

export class RadioStrip {
  readonly el: HTMLElement;
  private shown: Shown[] = [];
  /** Lines pushed since the page loaded (e2e). */
  count = 0;
  /** Latest lines' text, newest last (e2e, debugging). */
  readonly recent: string[] = [];
  /** Lines kept on the strip (3; 2 in player mode, set by the debug switch). */
  maxLines = MAX_LINES;

  constructor(parent: HTMLElement) {
    this.el = document.createElement("div");
    this.el.id = "radio-strip";
    this.el.setAttribute("role", "log");
    this.el.setAttribute("aria-label", "Police radio");
    this.el.setAttribute("aria-live", "polite");
    parent.appendChild(this.el);
  }

  /** Show a transmission. `unit` = police unit id (null / 0 = dispatch); `tier` = police
   * priority tier (1 actionable … 5 routine, 0 unknown). */
  push(unit: number | null, text: string, nowMs = performance.now(), tier = 0): void {
    const { callsign, rest } = splitCallsign(text);
    const line = document.createElement("div");
    line.className = `radio-line tier-${tier}`;
    line.dataset.unit = String(unit ?? 0);
    line.dataset.tier = String(tier);
    const icon = tierIcon(tier);
    if (icon) {
      const i = document.createElement("i");
      i.className = "radio-icon";
      i.textContent = icon;
      line.appendChild(i);
    }
    const chip = document.createElement("b");
    chip.className = "radio-cs";
    chip.textContent = (callsign || (unit ? `Unit ${unit}` : "Radio")).toUpperCase();
    const colour = callsignColour(callsign || (unit ? `Unit ${unit}` : "Dispatch"));
    chip.style.color = colour;
    line.style.borderLeftColor = colour;
    const body = document.createElement("span");
    body.textContent = rest;
    line.append(chip, body);
    this.el.appendChild(line);
    this.shown.push({ el: line, born: nowMs, show: showMs(tier) });
    while (this.shown.length > this.maxLines) this.shown.shift()!.el.remove();
    this.count++;
    this.recent.push(text);
    if (this.recent.length > 8) this.recent.shift();
  }

  /** Fade old lines (call once per frame). */
  frame(nowMs: number): void {
    for (const s of this.shown) {
      const age = nowMs - s.born;
      s.el.style.opacity = age <= s.show ? "" : String(Math.max(0, 1 - (age - s.show) / FADE_MS));
    }
    // Oldest first, but a highlighted line outlives plain ones pushed after it.
    for (let k = this.shown.length - 1; k >= 0; k--) {
      const s = this.shown[k];
      if (nowMs - s.born > s.show + FADE_MS) {
        s.el.remove();
        this.shown.splice(k, 1);
      }
    }
    this.el.classList.toggle("empty", this.shown.length === 0);
  }

  /** Lines on screen now. */
  visible(): number {
    return this.shown.length;
  }

  /** Clear (new session). */
  clear(): void {
    for (const s of this.shown) s.el.remove();
    this.shown = [];
    this.el.classList.add("empty");
  }
}
