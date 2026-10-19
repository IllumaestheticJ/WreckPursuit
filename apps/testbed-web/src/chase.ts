// City-chase overlay: the UI kit's wanted-level / heat panel (apps/ui-kit HeatPanel) plus police
// and traffic counts. Fed from `police_hud_json()` (Sim::police()) when the wasm build has it;
// otherwise it shows a "clear" panel with the live counts.
import { h, render } from "preact";
import { HeatPanel } from "@wreckpursuit/ui-kit/hud";
import type { HeatHud } from "@wreckpursuit/ui-kit";
import "@wreckpursuit/ui-kit/style.css";

const CLEAR: HeatHud = { level: "None", heat: 0, state: "clear", cooldown: 0, unitsInPursuit: 0, bounty: 0 };

export class ChaseHud {
  private readonly el: HTMLDivElement;
  private last = "";

  constructor(parent: HTMLElement) {
    this.el = document.createElement("div");
    this.el.className = "wp-ui chase-hud";
    Object.assign(this.el.style, {
      position: "absolute",
      top: "8px",
      left: "50%",
      right: "auto",
      bottom: "auto",
      transform: "translateX(-50%)",
      width: "15em",
      pointerEvents: "none",
      background: "transparent",
    });
    parent.appendChild(this.el);
    this.el.style.display = "none";
  }

  /** Show/hide and redraw (only when something changed). */
  update(visible: boolean, heat: HeatHud | null, police: number, traffic: number, trafficPhysical: number): void {
    this.el.style.display = visible ? "" : "none";
    if (!visible) return;
    const hh = heat ?? CLEAR;
    const key = JSON.stringify([hh, police, traffic, trafficPhysical]);
    if (key === this.last) return;
    this.last = key;
    render(
      // Not `.wp-hud` (that is the kit's full-screen HUD layout); the panel styles itself.
      h("div", { style: { fontFamily: "var(--font-display)", textShadow: "0 1px 2px rgba(0,0,0,0.9)" } }, [
        h(HeatPanel, { heat: hh }),
        h(
          "div",
          { class: "wp-mono wp-dim wp-hud-small", style: { marginTop: "4px", textAlign: "center" } },
          `POLICE ${police} · TRAFFIC ${traffic} (${trafficPhysical} physical)` +
            (heat ? "" : " · heat feed pending"),
        ),
      ]),
      this.el,
    );
  }
}
