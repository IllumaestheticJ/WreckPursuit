// Failure lines (owner-approved feature F1, "damage you can read"): one short HUD line per *new*
// mechanical failure on the player's car, saying what it means for driving, e.g.
// "Radiator holed — ~40 s to overheat". Each line stays LINE_MS of wall time; at most MAX_LINES
// show at once (newest on top). The crash toast (F2, ui/crash_toast.ts) lists what failed in a
// hit; these lines say what the player will feel. `failureLine` is exported so the toast can use
// the same words.
//
// UI only: reads drained SimEvents and damage_fx() (seconds to overheat), never touches the sim.

import type { SimEvent } from "../wasm/sim";

/** A line stays this long, ms. */
const LINE_MS = 7000;
/** Lines shown at once. */
const MAX_LINES = 3;

const CORNER: Record<string, string> = { FL: "front left", FR: "front right", RL: "rear left", RR: "rear right" };

/**
 * The HUD line for a failed component (`Component` name from the event), or null for parts with
 * no driving consequence worth a line. `overheatS` = seconds until a holed radiator has cooked
 * the engine (damage_fx(); < 0 unknown).
 */
export function failureLine(component: string, overheatS = -1): string | null {
  const corner = /^(Suspension|Wheel|Brake)(FL|FR|RL|RR)$/.exec(component);
  if (corner) {
    const where = CORNER[corner[2]];
    if (corner[1] === "Suspension") return `Suspension ${where} broken — the car pulls and leans`;
    if (corner[1] === "Wheel") return `Wheel ${where} wrecked — grip gone on that corner`;
    return `Brake ${where} gone — longer stops, pulls under braking`;
  }
  switch (component) {
    case "Engine":
      return "Engine dead";
    case "Radiator":
      return overheatS > 0 ? `Radiator holed — ~${Math.round(overheatS)} s to overheat` : "Radiator holed — engine will overheat";
    case "SteeringRack":
      return "Steering rack broken — less lock";
    case "Gearbox":
      return "Gearbox damaged — slow, missed shifts";
    case "Driveshaft":
      return "Driveshaft broken — no drive";
    case "FuelTank":
      return "Fuel tank holed — leaking";
    case "Battery":
      return "Battery smashed — electrics failing";
    default:
      return null;
  }
}

interface Line {
  text: string;
  untilMs: number;
}

/** The player's failure lines (bottom centre of the overlay). */
export class FailureLines {
  readonly el: HTMLDivElement;
  private lines: Line[] = [];
  /** Lines shown since the page loaded (automation). */
  count = 0;

  constructor(parent: HTMLElement) {
    this.el = document.createElement("div");
    this.el.className = "failure-lines";
    Object.assign(this.el.style, {
      position: "absolute",
      left: "50%",
      bottom: "22%",
      transform: "translateX(-50%)",
      pointerEvents: "none",
      textAlign: "center",
      font: "600 15px system-ui, sans-serif",
      color: "#ffd9a0",
      textShadow: "0 1px 3px #000, 0 0 8px #000",
      zIndex: "5",
    } satisfies Partial<CSSStyleDeclaration>);
    parent.append(this.el);
  }

  /**
   * One frame: add a line for each ComponentFailed / WheelDetached of `player` in `events`,
   * drop expired lines. `overheatS` from damage_fx() for the player (-1 when unknown).
   */
  frame(events: SimEvent[], player: number, nowMs: number, overheatS: number): void {
    let changed = false;
    for (const ev of events) {
      const [kind, body] = Object.entries(ev)[0] ?? [];
      const b = (body ?? {}) as Record<string, unknown>;
      if (b.vehicle !== player) continue;
      let text: string | null = null;
      if (kind === "ComponentFailed") text = failureLine(String(b.component), overheatS);
      else if (kind === "WheelDetached") text = "Wheel torn off";
      if (!text || this.lines.some((l) => l.text === text)) continue;
      this.lines.unshift({ text, untilMs: nowMs + LINE_MS });
      this.lines.length = Math.min(this.lines.length, MAX_LINES);
      this.count++;
      changed = true;
    }
    const before = this.lines.length;
    this.lines = this.lines.filter((l) => l.untilMs > nowMs);
    if (changed || before !== this.lines.length) this.render();
  }

  /** Drop every line (new session, reset). */
  clear(): void {
    this.lines = [];
    this.render();
  }

  /** Lines shown now (automation). */
  shown(): string[] {
    return this.lines.map((l) => l.text);
  }

  private render(): void {
    this.el.replaceChildren(
      ...this.lines.map((l) => {
        const div = document.createElement("div");
        div.textContent = l.text;
        return div;
      }),
    );
  }
}
