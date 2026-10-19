// End-of-run screen (lead, greybox demo): the result of a chase from wp_wasm's
// `run_result_json()` (G's EconomySession settled outside the sim step), drawn with the UI kit's
// primitives next to the kit's repair bill.
//  - capture / wreck: the run is over (the sim holds the car on the brakes). The panel shows the
//    outcome, fine, bail, impound fee, repair quote and cash before → after, with
//    "Repair & continue" (pays impound + repair from the ledger, repairs the car in place) and
//    "Restart";
//  - escape / safehouse: a short toast with the bounty; the player keeps driving.
import { render } from "preact";
import { Badge, Button, Money, Panel, Stat } from "../../../ui-kit/src/ui/kit";
import "../../../ui-kit/src/ui/theme.css";

/** `wp_wasm::run::LedgerLine`. */
export interface LedgerLine {
  seq: number;
  day: number;
  reason: string;
  detail: unknown;
  cash_delta: number;
  rep_delta: number;
  bounty_delta: number;
  cash_after: number;
}

/** `wp_wasm::run::RunReport`. */
export interface RunReport {
  seq: number;
  tick: number;
  outcome: "Escaped" | "Safehouse" | "Captured" | "CapturedTotalLoss" | "TotalLoss";
  run_over: boolean;
  level: string;
  mode: "Arcade" | "Simulation";
  bounty_payout: number;
  rep_delta: number;
  fines: number;
  bail: number;
  towing: number;
  car_status: "Garage" | "Impounded" | "Seized" | "Destroyed";
  impound_fee: number | null;
  impound_fee_per_day: number | null;
  days_until_auction: number | null;
  repair: { player_pays: number; gross: number; lines: unknown[] } | null;
  repair_cost: number;
  health: number;
  total_loss: boolean;
  cash_before: number;
  cash_after: number;
  continue_cost: number;
  can_continue: boolean;
  ledger: LedgerLine[];
}

/** `wp_wasm::run::ContinueReport`. */
export interface ContinueReport {
  impound_paid: number;
  repair_paid: number;
  cash_after: number;
  ledger: LedgerLine[];
}

const TITLE: Record<RunReport["outcome"], string> = {
  Escaped: "Escaped",
  Safehouse: "Laid low",
  Captured: "Busted",
  CapturedTotalLoss: "Busted · car wrecked",
  TotalLoss: "Wrecked",
};

/** Ledger reason → short label. */
const REASON: Record<string, string> = {
  Fine: "Fine",
  Bail: "Bail",
  Towing: "Towing",
  CarImpounded: "Impounded",
  ImpoundRetrieved: "Impound fee",
  RepairPaid: "Repair",
  Escape: "Escape bounty",
  BountyCleared: "Bounty cleared",
  CaptureRepLoss: "Rep lost",
  DamageRecorded: "Damage recorded",
  AccountOpened: "Account opened",
};

function Ledger({ lines }: { lines: LedgerLine[] }) {
  const shown = lines.filter((l) => l.cash_delta !== 0 || l.bounty_delta !== 0 || l.rep_delta !== 0);
  if (!shown.length) return null;
  return (
    <table class="wp-mono" style={{ width: "100%", fontSize: "0.85em", marginTop: "0.5em" }} data-testid="run-ledger">
      <tbody>
        {shown.map((l) => (
          <tr key={l.seq}>
            <td class="wp-dim">{REASON[l.reason] ?? l.reason}</td>
            <td style={{ textAlign: "right" }}>
              {l.cash_delta !== 0 ? <Money value={l.cash_delta} signed tone /> : l.bounty_delta !== 0 ? `bounty +${l.bounty_delta}` : `rep ${l.rep_delta > 0 ? "+" : ""}${l.rep_delta}`}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function RunEndPanel(props: { report: RunReport; error: string | null; onRepair: () => void; onRestart: () => void }) {
  const r = props.report;
  const caught = r.outcome === "Captured" || r.outcome === "CapturedTotalLoss";
  return (
    <div style={{ position: "absolute", left: "1em", top: "4em", width: "min(320px, calc(100vw - 32px))", pointerEvents: "auto", zIndex: 70 }}>
      <Panel title={TITLE[r.outcome]} tag={`${r.level} · ${r.mode.toUpperCase()}`}>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.5em" }}>
          {caught && <Stat label="Fine" value={<Money value={-r.fines} />} tone="danger" />}
          {r.bail > 0 && <Stat label="Bail" value={<Money value={-r.bail} />} tone="danger" />}
          {r.towing > 0 && <Stat label="Towing" value={<Money value={-r.towing} />} tone="danger" />}
          {r.impound_fee !== null && <Stat label="Impound fee" value={<Money value={r.impound_fee} />} tone="warn" />}
          <Stat label="Repair (quote)" value={<Money value={r.repair_cost} />} tone="warn" />
          <Stat label="Condition" value={`${Math.round(r.health * 100)}%`} tone={r.total_loss ? "danger" : "ok"} />
        </div>
        <div class="wp-spread" style={{ marginTop: "0.6em" }}>
          <span class="wp-dim">Cash</span>
          <span>
            <Money value={r.cash_before} /> → <Money value={r.cash_after} tone />
          </span>
        </div>
        {r.car_status !== "Garage" && (
          <div style={{ marginTop: "0.4em" }}>
            <Badge tone={r.car_status === "Impounded" ? "warn" : "danger"} solid>
              {r.car_status === "Impounded" ? `In the pound · auction in ${r.days_until_auction ?? "?"} days` : `Car ${r.car_status.toLowerCase()}`}
            </Badge>
          </div>
        )}
        <Ledger lines={r.ledger} />
        {props.error && (
          <p class="wp-danger" style={{ fontSize: "0.85em" }} data-testid="run-error">
            {props.error}
          </p>
        )}
        <div style={{ display: "flex", gap: "0.5em", marginTop: "0.8em", flexWrap: "wrap" }}>
          <Button variant="primary" disabled={!r.can_continue} onClick={props.onRepair} title={`impound + repair: $${r.continue_cost}`}>
            Repair &amp; continue (<Money value={r.continue_cost} />)
          </Button>
          <Button onClick={props.onRestart}>Restart</Button>
        </div>
      </Panel>
    </div>
  );
}

function Toast({ report }: { report: RunReport }) {
  return (
    <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
      <div class="wp-toast" data-kind="ok" data-testid="run-toast">
        {TITLE[report.outcome]} at {report.level} · bounty +{report.bounty_payout.toLocaleString("en-US")}
      </div>
    </div>
  );
}

/** How long the escape toast stays up, ms. */
const TOAST_MS = 5000;

/** Mounts the end-of-run panel / toast into `el` and keeps it in sync with `run_result_json()`. */
export class RunEndView {
  /** Latest report seen (e2e reads it). */
  report: RunReport | null = null;
  /** The end-of-run panel is showing (run over). */
  open = false;
  private error: string | null = null;
  private toastUntil = 0;

  constructor(
    private readonly el: HTMLElement,
    private readonly actions: { repair: () => ContinueReport; restart: () => void; onOpen: () => void; onClose: () => void },
  ) {}

  /** Call every frame with `run_result_json()`. */
  update(report: RunReport | null, nowMs: number): void {
    if (report && report.seq !== this.report?.seq) {
      this.report = report;
      this.error = null;
      if (report.run_over) {
        this.open = true;
        this.actions.onOpen();
      } else {
        this.toastUntil = nowMs + TOAST_MS;
      }
      this.draw(nowMs);
    } else if (!this.open && this.toastUntil && nowMs > this.toastUntil) {
      this.toastUntil = 0;
      this.draw(nowMs);
    }
  }

  /** New session: forget everything shown. */
  reset(): void {
    this.report = null;
    this.open = false;
    this.error = null;
    this.toastUntil = 0;
    render(null, this.el);
  }

  /** "Repair & continue". */
  repair(): void {
    try {
      this.actions.repair();
      this.open = false;
      this.error = null;
      this.actions.onClose();
    } catch (e) {
      this.error = String(e).replace(/^Error: /, "");
    }
    this.draw(performance.now());
  }

  private draw(nowMs: number): void {
    const r = this.report;
    if (r && this.open) {
      render(
        <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }} data-testid="run-end">
          <RunEndPanel report={r} error={this.error} onRepair={() => this.repair()} onRestart={() => this.actions.restart()} />
        </div>,
        this.el,
      );
    } else if (r && nowMs < this.toastUntil) {
      render(<Toast report={r} />, this.el);
    } else {
      render(null, this.el);
    }
  }
}
