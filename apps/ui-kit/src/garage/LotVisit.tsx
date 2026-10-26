/**
 * Lot visit (car lots): the overlay shown while the player is on a dealership, used lot or
 * junkyard. The testbed loads the lot itself (today's stock parked in rows, `career_visit_lot`)
 * and points the camera at the selected car; this panel lists the cars, shows the selected one
 * (price, condition, visible damage, tyres; a junkyard hides what is under the hood) and buys
 * it. Driven by a `LotVisitApi`, so it holds no game state of its own.
 */
import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { Badge, Button, Confirm, Meter, Money, Panel, severityColor, Stat } from "../ui/kit";
import { pct, words } from "../ui/format";
import type { CareerView, ListingView, LotKind, LotStockView } from "./types";
import "./garage.css";

/** What the lot overlay can do (the testbed implements it over wp_wasm). */
export interface LotVisitApi {
  /** Today's stock of the lot (sold cars flagged). */
  stock(): LotStockView;
  /** Cash and the rest of the career (refreshed after a purchase). */
  view(): CareerView;
  /** Point the camera at the car in `slot`. */
  focus(slot: number): void;
  /** Buy the car in `slot`; throws with the reason when refused. */
  buy(slot: number): CareerView;
  /** Leave the lot (back to the garage). */
  leave(): void;
}

const KIND: Record<LotKind, string> = {
  Dealership: "DEALERSHIP",
  UsedLot: "USED CARS",
  Junkyard: "JUNKYARD",
};

const WHEEL = ["FL", "FR", "RL", "RR"];

function Detail({ car, kind }: { car: ListingView; kind: LotKind }) {
  const hidden = car.condition === null;
  return (
    <>
      <div class="wp-cg-hero">
        <Stat label="Price" value={<Money value={car.price} />} tone="cash" />
        <Stat
          label="Condition"
          value={hidden ? "?" : pct(car.condition!)}
          tone={hidden ? undefined : car.condition! > 0.8 ? "ok" : "warn"}
        />
        <Stat label="Runs" value={car.runs === null ? "?" : car.runs ? "yes" : "no"} tone={car.runs === false ? "danger" : undefined} />
        {car.est_repairs !== null && car.est_repairs > 0 && <Stat label="Est. repairs" value={<Money value={car.est_repairs} />} tone="warn" />}
        {car.stats && <Stat label="PI" value={`${car.stats.class_index} ${car.stats.bracket}`} />}
      </div>
      {car.mods.length > 0 && (
        <p class="wp-mono" style={{ fontSize: "0.85em" }}>
          Comes with: {car.mods.join(" · ")}
        </p>
      )}
      {hidden && (
        <p class="wp-warn" style={{ fontSize: "0.85em" }}>
          Sold as seen. Nobody here knows whether it runs or what the engine is like: you find out when it's yours.
        </p>
      )}
      {car.tired_parts && car.tired_parts.length > 0 && (
        <p class="wp-dim" style={{ fontSize: "0.85em" }}>
          Worn: {car.tired_parts.map(([p, h]) => `${p} ${pct(h)}`).join(", ")}
        </p>
      )}
      {car.missing.length > 0 && <p class="wp-danger" style={{ fontSize: "0.85em" }}>Missing: {car.missing.join(", ")}</p>}
      {car.dents.length > 0 && <p class="wp-dim" style={{ fontSize: "0.85em" }}>Dents: {car.dents.join(", ")}</p>}
      {kind !== "Dealership" && (
        <div class="wp-row" style={{ gap: "0.6em", flexWrap: "wrap" }} data-testid="lot-tires">
          {car.tread.map((t, i) => (
            <div key={i} style={{ minWidth: "5em" }}>
              <span class="wp-dim wp-mono">{WHEEL[i]}</span> {car.flat[i] ? <span class="wp-danger">flat</span> : null}
              <Meter value={t} color={severityColor(1 - t)} label="Tread" />
            </div>
          ))}
        </div>
      )}
      {kind !== "Dealership" && (
        <p class="wp-dim" style={{ fontSize: "0.8em" }}>
          Body and mechanical damage repair in the garage. Tyres don't: replace them on their own.
        </p>
      )}
    </>
  );
}

function LotScreen({ api }: { api: LotVisitApi }) {
  const [stock, setStock] = useState<LotStockView>(() => api.stock());
  const [view, setView] = useState<CareerView>(() => api.view());
  const unsold = stock.cars.filter((c) => !c.sold);
  const [slot, setSlot] = useState<number | null>(unsold[0]?.slot ?? null);
  const [msg, setMsg] = useState<string | null>(null);
  /** Buying a car always asks first (browsing with the arrows makes a stray click easy). */
  const [ask, setAsk] = useState(false);
  const car = stock.cars.find((c) => c.slot === slot) ?? null;
  const pick = (s: number) => {
    setAsk(false);
    setSlot(s);
    setMsg(null);
    api.focus(s);
  };
  const step = (d: number) => {
    if (unsold.length === 0) return;
    const i = unsold.findIndex((c) => c.slot === slot);
    pick(unsold[(i + d + unsold.length) % unsold.length].slot);
  };
  // Arrow keys / , . browse the rows; Escape leaves.
  const keys = useRef({ step, leave: api.leave });
  keys.current = { step, leave: api.leave };
  useEffect(() => {
    if (slot !== null) api.focus(slot);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft" || e.key === ",") keys.current.step(-1);
      else if (e.key === "ArrowRight" || e.key === ".") keys.current.step(1);
      else if (e.key === "Escape") keys.current.leave();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const buy = () => {
    setAsk(false);
    if (!car) return;
    try {
      setView(api.buy(car.slot));
      setStock(api.stock());
      setMsg(`Bought: ${car.name}. It's waiting in your garage.`);
    } catch (e) {
      setMsg(String((e as Error).message ?? e).replace(/^(Error: )?wp_wasm: /, ""));
    }
  };
  const lot = stock.lot;
  return (
    <div class="wp-cg-lot" data-testid="lot-visit">
      <Panel title={lot.name} tag={`${KIND[lot.kind]} · ${"★".repeat(lot.tier)} · ${words(lot.district)} · night ${stock.day}`}>
        <div class="wp-row" style={{ gap: "0.5em", alignItems: "center", flexWrap: "wrap" }}>
          <span>
            CASH <Money value={view.cash} tone />
          </span>
          <span class="wp-dim">{unsold.length} cars today</span>
          <span style={{ marginLeft: "auto" }} />
          <Button size="sm" onClick={api.leave} hotkey="Esc">
            Back to garage
          </Button>
        </div>
        <div class="wp-cg-lot-list" role="listbox" aria-label="Cars on the lot">
          {stock.cars.map((c) => (
            <button
              type="button"
              key={c.slot}
              class="wp-cg-card"
              aria-selected={c.slot === slot}
              disabled={c.sold}
              onClick={() => pick(c.slot)}
              data-slot={c.slot}
            >
              <div class="wp-spread">
                <b class="wp-upper">{c.name}</b>
                <Money value={c.price} />
              </div>
              <div class="wp-row" style={{ gap: "0.3em", flexWrap: "wrap" }}>
                <span class="wp-dim wp-mono">{words(c.class)}</span>
                {c.rare && <Badge tone="info">rare</Badge>}
                {c.sold && <Badge>sold</Badge>}
                {c.locked && <Badge tone="dim">needs {c.unlock_tier}</Badge>}
                {c.runs === false && <Badge tone="danger">dead</Badge>}
                {c.missing.length > 0 && <Badge tone="warn">parts missing</Badge>}
              </div>
            </button>
          ))}
          {stock.cars.length === 0 && <p class="wp-dim">Nothing on the lot today. Come back tomorrow.</p>}
        </div>
      </Panel>
      {car && !car.sold && (
        <Panel title={car.name} tag={words(car.grade)}>
          <Detail car={car} kind={lot.kind} />
          <div class="wp-row" style={{ gap: "0.5em", marginTop: "0.5em" }}>
            <Button size="sm" onClick={() => step(-1)} title="previous car (←)">
              ◀
            </Button>
            <Button
              variant="primary"
              disabled={car.locked || car.price > view.cash || view.run_active}
              pressed={ask}
              onClick={() => setAsk(!ask)}
              testid="lot-buy"
              title={car.locked ? `needs ${car.unlock_tier} rep tier` : undefined}
            >
              Buy <Money value={car.price} />
            </Button>
            <Button size="sm" onClick={() => step(1)} title="next car (→)">
              ▶
            </Button>
          </div>
          {ask && (
            <Confirm
              key={car.slot}
              testid="lot-confirm"
              text={
                <>
                  Buy the {car.name} for <Money value={car.price} />? You have <Money value={view.cash} />; there are no refunds, only resale.
                </>
              }
              yes="Buy"
              onYes={buy}
              onNo={() => setAsk(false)}
            />
          )}
        </Panel>
      )}
      {msg && (
        <p class="wp-warn" data-testid="lot-msg">
          {msg}
        </p>
      )}
    </div>
  );
}

/** Mounts the lot overlay into `el` while a lot is visited. */
export class LotVisitView {
  /** The overlay is showing. */
  open = false;
  private key = 0;

  constructor(private readonly el: HTMLElement) {}

  /** Show the overlay for the lot `api` describes. */
  show(api: LotVisitApi): void {
    this.open = true;
    this.key++;
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none", zIndex: 70 }}>
        <LotScreen key={this.key} api={api} />
      </div>,
      this.el,
    );
  }

  /** Hide it. */
  hide(): void {
    this.open = false;
    render(null, this.el);
  }
}
