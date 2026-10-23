// Live UI-kit overlays (workstream 16, apps/ui-kit) on top of the testbed:
//  - chase readout (top centre): wanted level, heat, bounty, pursuit status, police radio — from
//    the drained SimEvents and, when the sim exposes it, the player's police record;
//  - repair bill (F): the running car's wp_damage estimate priced by wp_economy (wasm
//    repair_quote_json), with shop / district / option / insurance / wanted controls.
//  - pursuit banner (centre): PURSUIT / SEARCHING / ESCAPED / CAPTURED from `police_status()`;
//    a capture (or a total loss mid-chase) opens the repair bill, as does the end of a scripted
//    scenario. The chase end comes from the kit's `ChaseEndDetector`: the typed
//    `PoliceEvent::PursuitEnded` (outcome + level before the reset), else the status turning
//    clear with `police_status().last_outcome`.
// The kit's live model folds events with the same reducer as its mock provider, so these widgets
// behave the same in the testbed and on the kit's demo page. This is the testbed's only police
// readout (the old standalone heat panel was folded in here, round 6).
import { billRequestJson, ChaseEndDetector, createLiveModel, mountOverlay, type ChaseEndInfo, type LiveBillJson, type LiveModel } from "../../../ui-kit/src/live";
import type { SimEvent as KitSimEvent } from "../../../ui-kit/src/generated/contracts";
import { bannerFor, toReadout, type Banner, type PoliceStatus } from "../police";
import { layers } from "../settings";
import { sim, type SimEvent } from "../wasm/sim";
import type { Radar } from "./map";

export class UiOverlay {
  readonly model: LiveModel;
  private readonly chaseEl: HTMLElement;
  private readonly billEl: HTMLElement;
  private readonly bannerEl: HTMLElement;
  private billOpen = false;
  /** The open bill was opened automatically (capture / scenario end), not by the user. */
  private billAuto = false;
  private bannerKey = "";
  private readonly chaseEnd = new ChaseEndDetector();
  /** The latest chase end this session (outcome and level before the reset), for e2e / debugging. */
  lastChaseEnd: (ChaseEndInfo & { tick: number }) | null = null;
  /** Bill already shown automatically for this session's scripted end. */
  private endBillShown = false;
  /** Latest police status of the player (null = no police in this session). */
  status: PoliceStatus | null = null;

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
    this.bannerEl = document.createElement("div");
    this.bannerEl.id = "pursuit-banner";
    this.bannerEl.className = "hidden";
    parent.appendChild(this.bannerEl);
  }

  /** Feed one frame (call after draining events). */
  frame(tick: number, player: number, events: SimEvent[], hours: number, mode: string, radar: Radar | null = null): void {
    const chase = layers.on("chase");
    this.chaseEl.classList.toggle("hidden", !chase);
    const status = sim.policeStatus(player);
    this.status = status;
    this.showBanner(chase ? bannerFor(status) : null);
    // A capture or a wreck ends the run: show what the damage costs. (An escape does not: the
    // player is still driving.)
    const fresh = status?.last_outcome && status.outcome_age_s !== null && status.outcome_age_s < 1 ? status.last_outcome : null;
    const end = this.chaseEnd.update(player, status?.status, status?.level ?? "None", events as unknown as KitSimEvent[], fresh);
    if (end) {
      this.lastChaseEnd = { ...end, tick };
      if ((end.how === "captured" || end.how === "wrecked") && !this.billOpen) this.autoBill();
    }
    this.model.push({
      tick,
      player,
      events: events as unknown as KitSimEvent[],
      police: status ? toReadout(status) : null,
      damageMode: mode === "Simulation" ? "Simulation" : "Arcade",
      timeOfDayS: hours * 3600,
      // The chase radar only matters once police are around (police cars and the helicopter).
      radar: status ? radar : null,
    });
  }

  /** A scripted scenario finished: show the bill once per session. */
  sessionEnded(): void {
    if (this.endBillShown) return;
    this.endBillShown = true;
    if (!this.billOpen) this.autoBill();
  }

  private autoBill(): void {
    this.setBill(true);
    this.billAuto = true;
  }

  /** New session: forget heat, the old bill and the banner. A bill the user opened stays open
   * (re-priced for the new car); one that popped up automatically closes. */
  reset(): void {
    this.model.reset();
    this.chaseEnd.reset();
    this.lastChaseEnd = null;
    this.endBillShown = false;
    this.showBanner(null);
    if (this.billAuto) this.setBill(false);
    else if (this.billOpen) this.model.openBill();
  }

  private showBanner(b: Banner | null): void {
    const key = b ? `${b.text}|${b.sub}` : "";
    if (key === this.bannerKey) return;
    this.bannerKey = key;
    this.bannerEl.classList.toggle("hidden", !b);
    if (!b) return;
    this.bannerEl.dataset.kind = b.kind;
    this.bannerEl.innerHTML = "";
    const head = document.createElement("b");
    head.textContent = b.text;
    const sub = document.createElement("span");
    sub.textContent = b.sub;
    this.bannerEl.append(head, sub);
  }

  /** Show / hide the repair bill (re-priced on open). */
  setBill(open: boolean): void {
    this.billOpen = open;
    this.billAuto = false;
    this.billEl.classList.toggle("hidden", !open);
    if (open) this.model.openBill();
  }

  toggleBill(): void {
    this.setBill(!this.billOpen);
  }
}
