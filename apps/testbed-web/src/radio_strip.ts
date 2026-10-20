// Police radio strip (owner playtest row 79): the last few transmissions under the chase
// readout, each led by a coloured call-sign chip ("UNIT 12", "AIR 9", "DISPATCH").
//
// Lines arrive when they are *heard*: from the audio scanner's RadioSay (so text and voice line
// up, after the scanner tier's delay), or straight from the police events while the audio is
// not running (before the first click, scanner tier "None", audio failed). wp_police paces the
// radio (one transmission at a time), so the strip never needs to queue.

/** Lines kept on the strip. */
const MAX_LINES = 3;
/** A line is fully visible this long, then fades, ms. */
const SHOW_MS = 7000;
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
}

export class RadioStrip {
  readonly el: HTMLElement;
  private shown: Shown[] = [];
  /** Lines pushed since the page loaded (e2e). */
  count = 0;
  /** Latest lines' text, newest last (e2e, debugging). */
  readonly recent: string[] = [];

  constructor(parent: HTMLElement) {
    this.el = document.createElement("div");
    this.el.id = "radio-strip";
    this.el.setAttribute("role", "log");
    this.el.setAttribute("aria-label", "Police radio");
    this.el.setAttribute("aria-live", "polite");
    parent.appendChild(this.el);
  }

  /** Show a transmission. `unit` = police unit id (null / 0 = dispatch). */
  push(unit: number | null, text: string, nowMs = performance.now()): void {
    const { callsign, rest } = splitCallsign(text);
    const line = document.createElement("div");
    line.className = "radio-line";
    line.dataset.unit = String(unit ?? 0);
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
    this.shown.push({ el: line, born: nowMs });
    while (this.shown.length > MAX_LINES) this.shown.shift()!.el.remove();
    this.count++;
    this.recent.push(text);
    if (this.recent.length > 8) this.recent.shift();
  }

  /** Fade old lines (call once per frame). */
  frame(nowMs: number): void {
    for (const s of this.shown) {
      const age = nowMs - s.born;
      s.el.style.opacity = age <= SHOW_MS ? "1" : String(Math.max(0, 1 - (age - SHOW_MS) / FADE_MS));
    }
    while (this.shown.length && nowMs - this.shown[0].born > SHOW_MS + FADE_MS) this.shown.shift()!.el.remove();
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
