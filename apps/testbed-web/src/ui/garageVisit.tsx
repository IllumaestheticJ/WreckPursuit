// Garage visit panel (lead, greybox demo): stopping inside one of the city's repair shops
// (wp_wasm `garage_visit_json()`, see crates/wp_wasm/src/garage_visit.rs) holds the car and shows
// the shop's RepairAll quote, drawn with the UI kit's primitives:
//  - "Repair" pays from the ledger, repairs the car in place and gives control back;
//  - "Leave" gives control back without repairing.
// The car has to drive out of the shop's radius before the next visit opens.
// In a career run the panel offers "Enter garage" instead of Repair: the testbed ends the run
// at this shop and opens the career garage priced at it (district, legit / crooked).
// Keys: Enter / pad A = the primary button, Esc / Backspace / pad B = Leave (the car is held
// while the panel is open, so it must work without a mouse).
import { render } from "preact";
import { Badge, Button, Money, Panel, Stat } from "../../../ui-kit/src/ui/kit";
import { repairItemLabel } from "../../../ui-kit/src/data/economy";
import type { RepairItem } from "../../../ui-kit/src/generated/contracts";
import "../../../ui-kit/src/ui/theme.css";
import type { LedgerLine } from "./runEnd";

/** `wp_wasm::garage_visit::GarageSite`. */
export interface GarageSite {
  index: number;
  name: string;
  pos: [number, number, number];
  heading_rad: number;
  radius_m: number;
  kind: "Legit" | "Crooked";
  district: string;
}

/** `wp_economy::RepairQuote` (the fields the panel shows). */
interface Quote {
  lines: { item: RepairItem; severity: number; parts: number; labour: number; total: number }[];
  labour_multiplier: number;
  gross: number;
  insurance_covers: number;
  claim_denied: boolean;
  player_pays: number;
  reports_to_police: boolean;
}

/** `wp_wasm::garage_visit::GarageOffer`. */
export interface GarageOffer {
  seq: number;
  tick: number;
  site: GarageSite;
  wanted: string;
  health: number;
  quote: Quote | null;
  note: string | null;
  cash: number;
  can_repair: boolean;
  /** A career run: "Enter garage" (the career garage at this shop) instead of Repair. */
  career: boolean;
}

/** `wp_wasm::garage_visit::GarageRepairReport`. */
export interface GarageRepairReport {
  shop: string;
  paid: number;
  insurance_covers: number;
  reported_to_police: boolean;
  cash_after: number;
  ledger: LedgerLine[];
}

/** Lines shown before "+ N more". */
const MAX_LINES = 6;

/** How long the "repaired" toast stays up, ms. */
const TOAST_MS = 4000;

function GaragePanel(props: { offer: GarageOffer; error: string | null; onRepair: () => void; onEnter: () => void; onLeave: () => void }) {
  const o = props.offer;
  const q = o.quote;
  const lines = q ? [...q.lines].sort((a, b) => b.total - a.total) : [];
  // Centred under the pursuit banner: clear of the speed / HUD block (top-left).
  return (
    <div style={{ position: "absolute", left: "50%", top: "22%", transform: "translateX(-50%)", width: "min(340px, calc(100vw - 32px))", pointerEvents: "auto", zIndex: 70 }}>
      <Panel title={o.site.name} tag={`${o.site.kind.toUpperCase()} · ${o.site.district}`}>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.5em" }}>
          <Stat label="Condition" value={`${Math.round(o.health * 100)}%`} tone={o.health < 0.5 ? "danger" : "ok"} />
          <Stat label="Repair" value={<Money value={q?.player_pays ?? 0} />} tone="warn" />
        </div>
        {lines.length > 0 && (
          <table class="wp-mono" style={{ width: "100%", fontSize: "0.85em", marginTop: "0.5em" }} data-testid="garage-lines">
            <tbody>
              {lines.slice(0, MAX_LINES).map((l, i) => (
                <tr key={i}>
                  <td class="wp-dim">{repairItemLabel(l.item)}</td>
                  <td style={{ textAlign: "right" }}>
                    <Money value={l.total} />
                  </td>
                </tr>
              ))}
              {lines.length > MAX_LINES && (
                <tr>
                  <td class="wp-dim" colSpan={2}>
                    + {lines.length - MAX_LINES} more
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
        <div style={{ display: "flex", gap: "0.4em", flexWrap: "wrap", marginTop: "0.5em" }}>
          {q && q.labour_multiplier > 1.001 && <Badge tone="warn">labour ×{q.labour_multiplier.toFixed(2)}</Badge>}
          {q && q.insurance_covers > 0 && <Badge tone="ok">insurance −{q.insurance_covers.toLocaleString("en-US")}</Badge>}
          {q?.claim_denied && <Badge tone="danger">claim denied</Badge>}
          {q?.reports_to_police && (
            <Badge tone="danger" solid title="Legit shops report wanted cars (a ShopReported ledger line; police heat does not react yet)">
              reports wanted cars
            </Badge>
          )}
          {o.wanted !== "None" && !q?.reports_to_police && <Badge tone="info">wanted {o.wanted} · no questions</Badge>}
        </div>
        {o.note && (
          <p class="wp-dim" style={{ fontSize: "0.85em" }} data-testid="garage-note">
            {o.note.charAt(0).toUpperCase() + o.note.slice(1)}.
          </p>
        )}
        <p class="wp-dim" style={{ fontSize: "0.8em", marginTop: "0.5em" }} data-testid="garage-account">
          {o.career
            ? "Career: repairs, parts and tuning are done inside, at this shop's prices."
            : "Quick play: a demo account for this drive only, not your career."}
        </p>
        <div class="wp-spread" style={{ marginTop: "0.6em" }}>
          <span class="wp-dim">Cash</span>
          <span>
            <Money value={o.cash} />
            {q && (
              <>
                {" "}
                → <Money value={o.cash - q.player_pays} tone />
              </>
            )}
          </span>
        </div>
        {props.error && (
          <p class="wp-danger" style={{ fontSize: "0.85em" }} data-testid="garage-error">
            {props.error}
          </p>
        )}
        <div style={{ display: "flex", gap: "0.5em", marginTop: "0.8em", flexWrap: "wrap" }}>
          {o.career ? (
            <Button variant="primary" onClick={props.onEnter} hotkey="Enter" testid="garage-enter">
              Enter garage
            </Button>
          ) : (
            <Button variant="primary" disabled={!o.can_repair} onClick={props.onRepair} hotkey="Enter" testid="garage-repair">
              Repair (<Money value={q?.player_pays ?? 0} />)
            </Button>
          )}
          <Button onClick={props.onLeave} hotkey="Esc" testid="garage-leave">
            Leave
          </Button>
        </div>
      </Panel>
    </div>
  );
}

/** Mounts the garage panel into `el` and keeps it in sync with `garage_visit_json()`. */
export class GarageVisitView {
  /** The open visit (e2e reads it). */
  offer: GarageOffer | null = null;
  /** The latest repair (e2e reads it). */
  lastRepair: GarageRepairReport | null = null;
  private error: string | null = null;
  private toastUntil = 0;
  /** Pad buttons held last frame (A, B), for press edges. */
  private padWas = [true, true];

  constructor(
    private readonly el: HTMLElement,
    private readonly actions: { repair: () => GarageRepairReport; leave: () => void; enter: (offer: GarageOffer) => void },
  ) {
    // Capture phase: Enter / Esc act on the panel before the pause menu or driving keys see them.
    window.addEventListener("keydown", (e) => this.onKey(e), true);
  }

  /** Enter = the primary button, Esc / Backspace = Leave, while a visit is open. */
  private onKey(e: KeyboardEvent): void {
    if (!this.offer || e.repeat) return;
    if (e.code === "Enter" || e.code === "NumpadEnter") this.primary();
    else if (e.code === "Escape" || e.code === "Backspace") this.leave();
    else return;
    e.preventDefault();
    // Not the pause menu's Esc listener on the same window either.
    e.stopImmediatePropagation();
  }

  /** The panel's primary button: "Enter garage" in a career run, else Repair (when payable). */
  primary(): void {
    const o = this.offer;
    if (!o) return;
    if (o.career) this.enter();
    else if (o.can_repair) this.repair();
  }

  /** Gamepad: A = primary, B = Leave (press edges; the first frame of a visit ignores held buttons). */
  private pollPad(): void {
    const pads = typeof navigator !== "undefined" && navigator.getGamepads ? navigator.getGamepads() : [];
    let a = false;
    let b = false;
    for (const p of pads) {
      if (!p) continue;
      a ||= !!p.buttons[0]?.pressed;
      b ||= !!p.buttons[1]?.pressed;
    }
    const [wasA, wasB] = this.padWas;
    this.padWas = [a, b];
    if (!this.offer) return;
    if (a && !wasA) this.primary();
    else if (b && !wasB) this.leave();
  }

  /** This session's shops (`garages()`), for the "stop inside" hint. */
  sites: GarageSite[] = [];
  /** The shop whose ring the car is in without a visit (hint shown), or null. */
  near: GarageSite | null = null;
  /** The shop of the last visit, while the car is still in its ring (no hint). */
  private lastShop: string | null = null;

  /** Call every frame with `garage_visit_json()` and the player's position (sim m). */
  update(offer: GarageOffer | null, nowMs: number, player: [number, number, number] | null = null): void {
    if (this.offer) this.pollPad();
    else this.padWas = [true, true];
    const inRing = player ? (this.sites.find((g) => Math.hypot(player[0] - g.pos[0], player[1] - g.pos[1]) <= g.radius_m && Math.abs(player[2] - g.pos[2]) <= 4) ?? null) : null;
    // After a visit the shop does not reopen until the car has left its ring: no hint there.
    if (this.lastShop && inRing?.name !== this.lastShop) this.lastShop = null;
    const near = !offer && inRing && inRing.name !== this.lastShop ? inRing : null;
    if (near !== this.near) {
      this.near = near;
      if (!offer) this.draw(nowMs);
    }
    if (offer?.seq !== this.offer?.seq) {
      if (offer) this.lastShop = offer.site.name;
      this.offer = offer;
      this.error = null;
      this.draw(nowMs);
    } else if (!offer && this.toastUntil && nowMs > this.toastUntil) {
      this.toastUntil = 0;
      this.draw(nowMs);
    }
  }

  /** New session: forget everything shown. */
  reset(): void {
    this.near = null;
    this.lastShop = null;
    this.offer = null;
    this.lastRepair = null;
    this.error = null;
    this.toastUntil = 0;
    render(null, this.el);
  }

  /** "Repair". */
  repair(): void {
    try {
      this.lastRepair = this.actions.repair();
      this.offer = null;
      this.error = null;
      this.toastUntil = performance.now() + TOAST_MS;
    } catch (e) {
      this.error = String(e).replace(/^Error: /, "");
    }
    this.draw(performance.now());
  }

  /** "Enter garage" (career runs): the host ends the run here and opens the career garage. */
  enter(): void {
    const o = this.offer;
    if (!o) return;
    this.offer = null;
    this.error = null;
    render(null, this.el);
    this.actions.enter(o);
  }

  /** "Leave". */
  leave(): void {
    this.actions.leave();
    this.offer = null;
    this.error = null;
    this.draw(performance.now());
  }

  private draw(nowMs: number): void {
    const o = this.offer;
    const r = this.lastRepair;
    if (o) {
      render(
        <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }} data-testid="garage-visit">
          <GaragePanel offer={o} error={this.error} onRepair={() => this.repair()} onEnter={() => this.enter()} onLeave={() => this.leave()} />
        </div>,
        this.el,
      );
    } else if (r && nowMs < this.toastUntil) {
      render(
        <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
          <div class="wp-toast" data-kind="ok" data-testid="garage-toast">
            Repaired at {r.shop} · −{r.paid.toLocaleString("en-US")}
          </div>
        </div>,
        this.el,
      );
    } else if (this.near) {
      render(
        <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
          <div class="wp-toast" data-kind="info" data-testid="garage-hint">
            {this.near.name} · stop inside to visit
          </div>
        </div>,
        this.el,
      );
    } else {
      render(null, this.el);
    }
  }
}
