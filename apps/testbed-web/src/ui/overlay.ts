// Live UI-kit overlays (workstream 16, apps/ui-kit) on top of the testbed:
//  - chase readout (top centre): wanted level, heat, bounty, pursuit status, police radio — from
//    the drained SimEvents and, when the sim exposes it, the player's police record;
//  - repair bill (F): the running car's wp_damage estimate priced by wp_economy (wasm
//    repair_quote_json), with shop / district / option / insurance / wanted controls.
//  - pursuit banner (centre): PURSUIT / SEARCHING / ESCAPED / CAPTURED from `police_status()`;
//    a capture (or a total loss mid-chase) opens the repair bill, as does the end of a scripted
//    scenario. A capture / wreck is sequenced (playtest B5): the banner alone for 2.5 s, then
//    the end-of-run card (career / quick run result, banner hidden), then the bill 1.5 s later. The chase end comes from the kit's `ChaseEndDetector`: the typed
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

/** End sequence (playtest B5): the CAPTURED / wreck banner alone this long, ms. */
const END_BANNER_MS = 2500;
/** Then the end-of-run card alone this long before the repair bill opens, ms. */
const END_CARD_MS = 1500;
/** An ongoing pursuit / search banner shrinks to a corner chip after this long, ms (F44). */
const BANNER_CHIP_MS = 3000;

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
  /** Player-facing name of a simulated car ("a police car", "Lindwurm 4T"); null = unknown
   * (set by the host, which knows the views). */
  vehicleName: ((entity: number) => string | null) | null = null;
  /** The player's entity (latest frame). */
  private player = -1;
  /** Hide the chase readout while the player is clear (player mode; set by the debug switch). */
  quietWhenClear = false;
  /** Capture / wreck sequence (playtest B5): the outcome banner alone first, then the
   * end-of-run card (when the run has one), then the repair bill; never all three at once.
   * `start`: performance.now() of the end; `card`: the end-of-run card asked to open;
   * `cardAt`: when it was shown. Null when no sequence is running. */
  private endSeq: { start: number; card: boolean; cardAt: number | null } | null = null;
  /** The end-of-run card is up (the outcome banner stays hidden meanwhile). */
  private cardOpen = false;
  /** Banner kind showing ("" none) and since when (performance.now ms), for the chip (F44). */
  private bannerKind = "";
  private bannerSince = 0;

  constructor(parent: HTMLElement) {
    this.model = createLiveModel({
      quote: (player, request): LiveBillJson | null => (player ? sim.repairQuote(player, billRequestJson(request)) : null),
      onBack: () => this.setBill(false),
      // Player-facing names (playtest B11): no entity numbers.
      nameOf: (e) =>
        e === this.player ? "you" : e === 4294967295 ? "a barrier" : (this.vehicleName?.(e) ?? (sim.entities().includes(e) ? "a car" : "a traffic car")),
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
    this.player = player;
    const chase = layers.on("chase");
    const status = sim.policeStatus(player);
    this.status = status;
    // Player mode: no "CLEAR / no police interest" box while nobody is after the player (no
    // police in the session, or clear with no heat and no fresh outcome). Debug keeps it.
    const quiet = this.quietWhenClear && (!status || (status.status === "Clear" && status.heat <= 0 && bannerFor(status) === null));
    this.chaseEl.classList.toggle("hidden", !chase || quiet);
    const now = performance.now();
    const cardShown = this.sequence(now);
    // The banner steps aside for the end-of-run card and the bill (it covered the bill's header).
    const banner = chase && !cardShown && !this.billOpen ? bannerFor(status) : null;
    this.showBanner(banner);
    // F44: an ongoing PURSUIT / SEARCHING banner condenses to a chip after 3 s (outcomes stay big).
    const kind = banner?.kind ?? "";
    if (kind !== this.bannerKind) {
      this.bannerKind = kind;
      this.bannerSince = now;
    }
    const chip = (kind === "pursuit" || kind === "searching") && now - this.bannerSince > BANNER_CHIP_MS;
    if (this.bannerEl.classList.contains("chip") !== chip) this.bannerEl.classList.toggle("chip", chip);
    // A capture or a wreck ends the run: show what the damage costs. (An escape does not: the
    // player is still driving.)
    const fresh = status?.last_outcome && status.outcome_age_s !== null && status.outcome_age_s < 1 ? status.last_outcome : null;
    const end = this.chaseEnd.update(player, status?.status, status?.level ?? "None", events as unknown as KitSimEvent[], fresh);
    if (end) {
      this.lastChaseEnd = { ...end, tick };
      if ((end.how === "captured" || end.how === "wrecked") && !this.billOpen) this.startEnd(now, false);
    }
    this.model.push({
      tick,
      player,
      events: events as unknown as KitSimEvent[],
      police: status ? toReadout(status) : null,
      damageMode: mode === "Simulation" || mode === "Extreme" ? mode : "Arcade",
      timeOfDayS: hours * 3600,
      // The chase radar only matters once police are around (police cars and the helicopter).
      radar: status ? radar : null,
    });
  }

  /** Begin (or join) the end sequence: `card` = the end-of-run card wants to open. */
  private startEnd(now: number, card: boolean): void {
    if (this.endSeq) this.endSeq.card ||= card;
    // A card without a chase end (a wreck outside a pursuit) has no banner to wait for.
    else this.endSeq = { start: card ? now - END_BANNER_MS : now, card, cardAt: null };
  }

  /** Step the end sequence; true while the end-of-run card is showing. */
  private sequence(now: number): boolean {
    const runEnd = document.getElementById("ui-run-end");
    const seq = this.endSeq;
    if (!seq) {
      if (runEnd && runEnd.style.visibility) runEnd.style.visibility = "";
      return this.cardOpen;
    }
    const bannerPhase = now - seq.start < END_BANNER_MS;
    if (runEnd) runEnd.style.visibility = bannerPhase ? "hidden" : "";
    if (bannerPhase) return false;
    if (!seq.card) {
      // No end-of-run card (a run without a result panel): the bill follows the banner.
      this.endSeq = null;
      if (!this.billOpen) this.autoBill();
      return false;
    }
    seq.cardAt ??= now;
    if (now - seq.cardAt >= END_CARD_MS) {
      this.endSeq = null;
      if (!this.billOpen) this.autoBill();
    }
    return true;
  }

  /** The end-of-run card opened (run over): it shows after the outcome banner, the bill after it. */
  runEndOpened(): void {
    this.cardOpen = true;
    this.startEnd(performance.now(), true);
  }

  /** The end-of-run card closed (repair & continue): its bill closes with it. */
  runEndClosed(): void {
    this.cardOpen = false;
    this.endSeq = null;
    this.setBill(false);
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
    this.endSeq = null;
    this.cardOpen = false;
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
