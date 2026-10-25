// Garage visit panel (lead, greybox demo): stopping inside one of the city's repair shops
// (wp_wasm `garage_visit_json()`, see crates/wp_wasm/src/garage_visit.rs) holds the car and shows
// the shop's RepairAll quote, drawn with the UI kit's primitives:
//  - "Repair" pays from the ledger, repairs the car in place and gives control back;
//  - "Leave" gives control back without repairing.
// The car has to drive out of the shop's radius before the next visit opens.
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

function GaragePanel(props: { offer: GarageOffer; error: string | null; onRepair: () => void; onLeave: () => void }) {
  const o = props.offer;
  const q = o.quote;
  const lines = q ? [...q.lines].sort((a, b) => b.total - a.total) : [];
  return (
    <div style={{ position: "absolute", left: "1em", top: "4em", width: "min(320px, calc(100vw - 32px))", pointerEvents: "auto", zIndex: 70 }}>
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
          <Button variant="primary" disabled={!o.can_repair} onClick={props.onRepair}>
            Repair (<Money value={q?.player_pays ?? 0} />)
          </Button>
          <Button onClick={props.onLeave}>Leave</Button>
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

  constructor(
    private readonly el: HTMLElement,
    private readonly actions: { repair: () => GarageRepairReport; leave: () => void },
  ) {}

  /** Call every frame with `garage_visit_json()`. */
  update(offer: GarageOffer | null, nowMs: number): void {
    if (offer?.seq !== this.offer?.seq) {
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
          <GaragePanel offer={o} error={this.error} onRepair={() => this.repair()} onLeave={() => this.leave()} />
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
    } else {
      render(null, this.el);
    }
  }
}
