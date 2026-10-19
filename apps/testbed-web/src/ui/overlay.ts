// Live UI-kit overlays (workstream 16, apps/ui-kit) on top of the testbed:
//  - chase readout (top centre): the one wanted / heat / bounty / pursuit panel (consolidated
//    with C's heat panel in round 6) — from wp_wasm `police_status` (see ./police.ts) plus the
//    police radio from the drained SimEvents;
//  - repair bill (F): the running car's wp_damage estimate priced by G's wp_economy
//    (`Economy::quote_repair` via wasm repair_quote_json), with shop / district / option /
//    insurance / wanted controls. It opens on its own when a chase ends (escaped or written off):
//    the demo's end-of-chase moment.
// The kit's live model folds events with the same reducer as its mock provider, so these widgets
// behave the same in the testbed and on the kit's demo page.
import { billRequestJson, ChaseEndDetector, createLiveModel, type ChaseEnd, mountOverlay, type LiveBillJson, type LiveModel } from "../../../ui-kit/src/live";
import type { SimEvent as KitSimEvent } from "../../../ui-kit/src/generated/contracts";
import { layers } from "../settings";
import { sim, type SimEvent } from "../wasm/sim";
import { toReadout } from "./police";

export class UiOverlay {
  readonly model: LiveModel;
  private readonly chaseEl: HTMLElement;
  private readonly billEl: HTMLElement;
  private billOpen = false;
  private readonly chaseEnd = new ChaseEndDetector();
  /** How the last chase ended this session, if one did (automation reads it). */
  lastChaseEnd: ChaseEnd | null = null;

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
    const police = sim.policeStatus(player);
    this.model.push({
      tick,
      player,
      events: events as unknown as KitSimEvent[],
      police: police ? toReadout(police) : null,
      damageMode: mode === "Simulation" ? "Simulation" : "Arcade",
      timeOfDayS: hours * 3600,
    });
    const end = this.chaseEnd.update(police?.status, this.model.get().hud.totalLoss);
    if (end) {
      this.lastChaseEnd = end;
      this.setBill(true);
    }
  }

  /** New session: forget heat and the old bill. */
  reset(): void {
    this.model.reset();
    this.chaseEnd.reset();
    this.lastChaseEnd = null;
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
