// First-run onboarding card (owner-approved feature F5): the first time a player drives (debug
// tools off), one card explains the controls (gears are manual by default, with a button that
// turns on automatic gears), heat and the police, and how to start a race. Shown once per
// browser (`wp.testbed.onboarded.v1`); never under automation unless `?onboard=1`.

import { store } from "../settings";
import { isAutomation } from "./dev_toggle";

const SEEN_KEY = "wp.testbed.onboarded.v1";

/** What the card needs from the host. */
export interface OnboardingHost {
  /** Key text of a bound action ("W / ↑ · pad RT"). */
  keys: (action: "throttle" | "brake" | "steerLeft" | "steerRight" | "handbrake" | "gearUp" | "gearDown" | "horn" | "map" | "reset") => string;
  /** Automatic gears are on now. */
  autoGears: () => boolean;
  /** Turn automatic gears on (Settings → Assists). */
  setAutoGears: () => void;
  /** Pause / resume the game while the card is up. */
  pause: (on: boolean) => void;
}

/** The onboarding card (`#wp-onboarding`). */
export class Onboarding {
  readonly el: HTMLElement;
  /** The card is up. */
  open = false;
  private readonly force: boolean;

  constructor(
    parent: HTMLElement,
    private readonly host: OnboardingHost,
    params: URLSearchParams,
  ) {
    this.force = params.get("onboard") === "1";
    this.el = document.createElement("div");
    this.el.id = "wp-onboarding";
    this.el.className = "wp-ui hidden";
    this.el.setAttribute("role", "dialog");
    this.el.setAttribute("aria-label", "How to play");
    parent.appendChild(this.el);
    window.addEventListener(
      "keydown",
      (e) => {
        if (!this.open || (e.code !== "Enter" && e.code !== "Escape" && e.code !== "NumpadEnter")) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        this.close();
      },
      { capture: true },
    );
  }

  /** Should the card show (not seen yet; not automation unless forced)? */
  get due(): boolean {
    return (this.force || !isAutomation()) && !store.get(SEEN_KEY);
  }

  /** Once per frame while driving in player mode: shows the card the first time. */
  frame(driving: boolean): void {
    if (driving && !this.open && this.due) this.show();
  }

  /** Show the card now. */
  show(): void {
    const k = this.host.keys;
    const auto = this.host.autoGears();
    this.el.innerHTML =
      `<div class="wp-onboarding-card">` +
      `<div class="wp-mono wp-onboarding-kicker">// FIRST NIGHT</div><h2>How to drive</h2>` +
      `<dl>` +
      `<dt>Drive</dt><dd>${esc(k("throttle"))} throttle · ${esc(k("brake"))} brake · ${esc(k("steerLeft"))} / ${esc(k("steerRight"))} steer · ${esc(k("handbrake"))} handbrake · ${esc(k("reset"))} reset the car</dd>` +
      `<dt>Gears</dt><dd>${auto ? "Automatic gears are on." : `Gears are <b>manual</b>: ${esc(k("gearUp"))} up, ${esc(k("gearDown"))} down. ` + `<button type="button" class="wp-btn wp-btn--ghost" id="onb-auto">Use automatic gears</button>`} <span class="wp-dim">(Settings → Assists)</span></dd>` +
      `<dt>Heat</dt><dd>Speeding, red lights and hitting cars raise your heat (the bar at the top). Enough heat and the police chase you; break line of sight to escape. Getting boxed in costs fines and the impound.</dd>` +
      `<dt>Races</dt><dd>Title → Street races, or pull up right behind a street racer in the city (a numbered dot on the map, ${esc(k("map"))}) and honk (${esc(k("horn"))}) to challenge them.</dd>` +
      `</dl>` +
      `<button type="button" class="wp-btn wp-btn--primary" id="onb-ok">Got it <span class="wp-kbd">Enter</span></button>` +
      `</div>`;
    (this.el.querySelector("#onb-ok") as HTMLButtonElement).onclick = () => this.close();
    const autoBtn = this.el.querySelector("#onb-auto") as HTMLButtonElement | null;
    if (autoBtn)
      autoBtn.onclick = () => {
        this.host.setAutoGears();
        autoBtn.replaceWith("Automatic gears are on now.");
      };
    this.el.classList.remove("hidden");
    this.open = true;
    this.host.pause(true);
    (this.el.querySelector("#onb-ok") as HTMLButtonElement).focus();
  }

  /** Close the card; it is not shown again in this browser. */
  close(): void {
    if (!this.open) return;
    this.open = false;
    store.set(SEEN_KEY, "1");
    this.el.classList.add("hidden");
    this.host.pause(false);
  }
}

const esc = (t: string) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;");
