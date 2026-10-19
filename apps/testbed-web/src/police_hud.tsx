// Wanted level / heat readout using M's UI kit (apps/ui-kit `HeatPanel`, Preact). The testbed
// owns the data: `police_status()` from wp_wasm is mapped onto the kit's `HeatHud` shape.
import { render } from "preact";
import { HeatPanel } from "../../ui-kit/src/screens/hud/Hud";
import type { HeatHud } from "../../ui-kit/src/model/types";
import "../../ui-kit/src/ui/theme.css";

/** `police_status(entity)` JSON from wp_wasm (mirrors `wp_police` for one racer). */
export interface PoliceStatus {
  level: HeatHud["level"];
  /** Heat 0..=1 within the current level. */
  heat_in_level: number;
  /** `Clear` | `Wanted` | `Pursuit` | `Searching` (`wp_police::PursuitStatus`). */
  status: "Clear" | "Wanted" | "Pursuit" | "Searching";
  /** Escape progress 0..=1 while `Searching`. */
  cooldown: number;
  units_in_pursuit: number;
  bounty: number;
}

const STATE: Record<PoliceStatus["status"], HeatHud["state"]> = {
  Clear: "clear",
  Wanted: "noticed",
  Pursuit: "pursuit",
  Searching: "cooldown",
};

/** Map wp_wasm's status onto the UI kit's `HeatHud`. */
export function toHeatHud(p: PoliceStatus): HeatHud {
  return {
    level: p.level,
    heat: p.heat_in_level,
    state: STATE[p.status],
    cooldown: p.cooldown,
    unitsInPursuit: p.units_in_pursuit,
    bounty: p.bounty,
  };
}

/** The kit's heat panel in a `.wp-ui` scope (top-centre of the overlay). */
export class PoliceHud {
  private readonly root: HTMLElement;
  private last = "";

  constructor(parent: HTMLElement) {
    this.root = document.createElement("div");
    this.root.className = "wp-ui police-hud";
    parent.appendChild(this.root);
  }

  /** Show the panel for `status`, or hide it (`null` = no police in this session). */
  update(status: PoliceStatus | null): void {
    const key = JSON.stringify(status);
    if (key === this.last) return;
    this.last = key;
    this.root.style.display = status ? "" : "none";
    render(status ? <HeatPanel heat={toHeatHud(status)} /> : null, this.root);
  }
}
