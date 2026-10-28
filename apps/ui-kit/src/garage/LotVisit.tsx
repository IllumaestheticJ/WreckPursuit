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
import { keyAction, PadNav, type NavAction } from "../menu/nav";
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
  const [msg, setMsg] = useState<{ text: string; ok: boolean } | null>(null);
  /** Buying a car always asks first (browsing with the arrows makes a stray click easy). */
  const [ask, setAsk] = useState(false);
  const car = stock.cars.find((c) => c.slot === slot) ?? null;
  const listRef = useRef<HTMLDivElement>(null);
  const canBuy = !!car && !car.sold && !car.locked && car.price <= view.cash && !view.run_active;
  const pick = (s: number) => {
    setAsk(false);
    setSlot(s);
    setMsg(null);
    api.focus(s);
  };
  const step = (d: number) => {
    if (unsold.length === 0) return;
    const i = unsold.findIndex((c) => c.slot === slot);
    // From nothing selected (the last car was just bought), forward starts at the top.
    const next = i < 0 ? (d > 0 ? 0 : unsold.length - 1) : (i + d + unsold.length) % unsold.length;
    pick(unsold[next].slot);
  };
  // The highlighted card stays in view in the list (only the list scrolls, not the page).
  useEffect(() => {
    const list = listRef.current;
    const card = list?.querySelector<HTMLElement>(`[data-slot="${slot}"]`);
    if (!list || !card) return;
    // The list is the cards' offset parent (position: relative in garage.css).
    const top = card.offsetTop;
    const pad = 6;
    if (top - pad < list.scrollTop) list.scrollTop = top - pad;
    else if (top + card.offsetHeight + pad > list.scrollTop + list.clientHeight) list.scrollTop = top + card.offsetHeight + pad - list.clientHeight;
  }, [slot]);
  const buy = () => {
    setAsk(false);
    if (!car) return;
    try {
      setView(api.buy(car.slot));
      const next = api.stock();
      setStock(next);
      // Move on to the next car still for sale (the bought one leaves the rows).
      const left = next.cars.filter((c) => !c.sold);
      const after = left.find((c) => c.slot > car.slot) ?? left[0] ?? null;
      setSlot(after?.slot ?? null);
      if (after) api.focus(after.slot);
      setMsg({ text: `Bought: ${car.name}. It's waiting in your garage.`, ok: true });
    } catch (e) {
      setMsg({ text: String((e as Error).message ?? e).replace(/^(Error: )?wp_wasm: /, ""), ok: false });
    }
  };
  /** One browse action from the keyboard or a gamepad. */
  const nav = (a: NavAction) => {
    if (a === "up" || a === "left") step(-1);
    else if (a === "down" || a === "right") step(1);
    else if (a === "activate") {
      if (ask) buy();
      else if (canBuy) setAsk(true);
    } else if (a === "back") {
      if (ask) setAsk(false);
      else api.leave();
    }
  };
  const navRef = useRef(nav);
  navRef.current = nav;
  // Arrow keys (and , .) browse the cars, Enter buys (asks first), Escape leaves. A gamepad
  // does the same: d-pad / left stick browse, A buys, B goes back.
  useEffect(() => {
    if (slot !== null) api.focus(slot);
    const onKey = (e: KeyboardEvent) => {
      // The buy confirmation takes its own keys (Y / N / Esc, Enter on its focused button).
      if (document.querySelector(".wp-cg-lot .wp-confirm")) return;
      let a: NavAction | null = null;
      if (e.key === ",") a = "left";
      else if (e.key === ".") a = "right";
      else if (e.code === "Escape") a = "back";
      else if (e.code === "Enter" || e.code === "NumpadEnter") a = "activate";
      else if (e.code.startsWith("Arrow")) a = keyAction(e.code);
      if (!a) return;
      e.preventDefault();
      if (a === "activate" && e.repeat) return;
      navRef.current(a);
    };
    window.addEventListener("keydown", onKey);
    const pad = new PadNav();
    const p0 = firstPad();
    pad.rest(p0?.buttons.map((b) => b.value) ?? [], p0?.axes ?? []);
    let raf = requestAnimationFrame(function poll() {
      raf = requestAnimationFrame(poll);
      const p = firstPad();
      if (!p) return;
      for (const a of pad.update(
        p.buttons.map((b) => b.value),
        p.axes,
        performance.now(),
      ))
        navRef.current(a);
    });
    return () => {
      window.removeEventListener("keydown", onKey);
      cancelAnimationFrame(raf);
    };
  }, []);
  const lot = stock.lot;
  const index = unsold.findIndex((c) => c.slot === slot);
  return (
    <div class="wp-cg-lot" data-testid="lot-visit">
      <Panel class="wp-cg-lot-stock" title={lot.name} tag={`${KIND[lot.kind]} · ${"★".repeat(lot.tier)} · ${words(lot.district)} · night ${stock.day}`}>
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
        <div class="wp-cg-lot-list" role="listbox" aria-label="Cars on the lot" ref={listRef}>
          {stock.cars.map((c) => (
            <button
              type="button"
              key={c.slot}
              class="wp-cg-card"
              role="option"
              aria-selected={c.slot === slot}
              disabled={c.sold}
              tabIndex={-1}
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
                {!c.sold && !c.locked && c.price > view.cash && <Badge tone="dim">can't afford</Badge>}
                {c.runs === false && <Badge tone="danger">dead</Badge>}
                {c.missing.length > 0 && <Badge tone="warn">parts missing</Badge>}
              </div>
            </button>
          ))}
          {stock.cars.length === 0 && <p class="wp-dim">Nothing on the lot today. Come back tomorrow.</p>}
          {stock.cars.length > 0 && unsold.length === 0 && <p class="wp-dim">Sold out. Come back tomorrow.</p>}
        </div>
      </Panel>
      {(car && !car.sold) || msg ? (
        <Panel class="wp-cg-lot-detail" title={car && !car.sold ? car.name : "Lot"} tag={car && !car.sold ? `${words(car.grade)} · ${index + 1} / ${unsold.length}` : undefined}>
          {msg && (
            <p class={msg.ok ? "wp-ok" : "wp-warn"} data-testid="lot-msg" style={{ marginTop: 0 }}>
              {msg.text}
            </p>
          )}
          {car && !car.sold && (
            <>
              <Detail car={car} kind={lot.kind} />
              <div class="wp-row wp-cg-lot-buy">
                <Button size="sm" onClick={() => step(-1)} title="previous car (↑ / ←)">
                  ◀
                </Button>
                <Button
                  variant="primary"
                  disabled={!canBuy}
                  pressed={ask}
                  onClick={() => setAsk(!ask)}
                  testid="lot-buy"
                  hotkey="Enter"
                  title={car.locked ? `needs ${car.unlock_tier} rep tier` : car.price > view.cash ? "not enough cash" : undefined}
                >
                  Buy <Money value={car.price} />
                </Button>
                <Button size="sm" onClick={() => step(1)} title="next car (↓ / →)">
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
            </>
          )}
          <p class="wp-dim wp-mono wp-cg-lot-keys">↑ ↓ browse · Enter buy · Esc back</p>
        </Panel>
      ) : null}
    </div>
  );
}

function firstPad(): Gamepad | null {
  try {
    for (const p of navigator.getGamepads?.() ?? []) if (p && p.connected !== false) return p;
  } catch {
    // Gamepad API blocked (permissions policy): keyboard and mouse only.
  }
  return null;
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
