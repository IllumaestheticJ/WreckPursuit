// Live UI-kit overlays (workstream 16, apps/ui-kit) on top of the testbed:
//  - chase readout (top centre): wanted level, heat, bounty, pursuit status, police radio — from
//    the drained SimEvents and, when the sim exposes it, the player's police record;
//  - repair bill (F): the running car's wp_damage estimate priced by wp_economy (wasm
//    repair_quote_json), with shop / district / option / insurance / wanted controls.
// The kit's live model folds events with the same reducer as its mock provider, so these widgets
// behave the same in the testbed and on the kit's demo page.
import { billRequestJson, createLiveModel, mountOverlay, type LiveBillJson, type LiveModel, type PoliceReadout } from "../../ui-kit/src/live";
import type { SimEvent as KitSimEvent } from "../../ui-kit/src/generated/contracts";
import { layers } from "./settings";
import { sim, type SimEvent } from "./wasm/sim";

export class UiOverlay {
  readonly model: LiveModel;
  private readonly chaseEl: HTMLElement;
  private readonly billEl: HTMLElement;
  private billOpen = false;

  constructor(parent: HTMLElement) {
    this.model = createLiveModel({
      quote: (player, request): LiveBillJson | null => (player ? sim.repairQuote(player, billRequestJson(request)) : null),
      onBack: () => this.setBill(false),
      nameOf: (e) => (sim.entities().includes(e) ? `car #${e}` : e === 4294967295 ? "barrier" : `traffic #${e}`),
    });
    const layer = (id: string) => {
      const el = document.createElement("div");
      el.id = id;
      el.style.cssText = "position:absolute;inset:0;pointer-events:none;";
      parent.appendChild(el);
      return el;
    };
    this.chaseEl = layer("ui-chase");
    this.billEl = layer("ui-bill");
    this.billEl.classList.add("hidden");
    mountOverlay(this.chaseEl, this.model, "chaseHud");
    mountOverlay(this.billEl, this.model, "repairBill");
  }

  /** Feed one frame (call after draining events). */
  frame(tick: number, player: number, events: SimEvent[], hours: number, mode: string): void {
    this.chaseEl.classList.toggle("hidden", !layers.on("chase"));
    this.model.push({
      tick,
      player,
      events: events as unknown as KitSimEvent[],
      police: sim.policeStatus(player) as PoliceReadout | null,
      damageMode: mode === "Simulation" ? "Simulation" : "Arcade",
      timeOfDayS: hours * 3600,
    });
  }

  /** New session: forget heat and the old bill. */
  reset(): void {
    this.model.reset();
    if (this.billOpen) this.model.openBill();
  }

  /** Show / hide the repair bill (re-priced on open). */
  setBill(open: boolean): void {
    this.billOpen = open;
    this.billEl.classList.toggle("hidden", !open);
    if (open) this.model.openBill();
  }

  toggleBill(): void {
    this.setBill(!this.billOpen);
  }
}
