/**
 * The tow service (owner 2026-10-28: "tow broken cars to repair shops"): the destination list
 * with the price of each (`TowOffer` from wp_wasm's `career_tow_offer_json`). Used by the career
 * garage ("Tow to …" for any car not in the pound; open at once for one that cannot be driven) and by the testbed's
 * in-run "Call a tow truck" pop-up (`TowPickerView`).
 *
 * The nearest repair shop is always on offer: without the cash the tow still happens and the
 * shortfall becomes debt. Other shops and the player's garages need the cash.
 */
import { render } from "preact";
import { Badge, Button, Modal, Money } from "../ui/kit";
import type { CareerPlace, LedgerLine, TowDestination, TowNeed, TowOffer } from "./types";

/** Why the car needs a tow, for the prompt. */
export const TOW_NEED: Record<TowNeed, string> = {
  Wrecked: "The car is wrecked.",
  EngineDead: "The engine is dead.",
  WheelOff: "A wheel came off.",
  Flipped: "The car is on its side.",
  Stuck: "The car is stuck.",
};

/** "4.2 km" from metres. */
export function km(metres: number): string {
  return `${(metres / 1000).toFixed(1)} km`;
}

/** The ledger label of a tow line ("Tow: Gantry Garage → 4.2 km"), or null for other lines. */
export function towLedgerLabel(l: Pick<LedgerLine, "detail">): string | null {
  const t = (l.detail as { Tow?: { to?: string; metres?: number } } | null)?.Tow;
  if (!t) return null;
  return `Tow: ${t.to ?? "?"} → ${km(t.metres ?? 0)}`;
}

/** One line for the result: "Towed to Gantry Garage: 4.2 km, $480 (now $1,200 in debt)." */
export function towDoneLine(place: CareerPlace, metres: number, price: number, cashAfter: number): string {
  const debt = cashAfter < 0 ? ` You owe $${(-cashAfter).toLocaleString("en-US")}.` : "";
  return `Towed to ${place.name}: ${km(metres)}, $${price.toLocaleString("en-US")}.${debt}`;
}

/** The destination table: pick one to tow there. */
export function TowList(props: { offer: TowOffer; onPick: (d: TowDestination) => void; busy?: boolean }) {
  const { offer } = props;
  if (offer.refused) {
    return (
      <p class="wp-warn" data-testid="tow-refused">
        {offer.refused}
      </p>
    );
  }
  return (
    <table class="wp-table" data-testid="tow-destinations">
      <tbody>
        {offer.destinations.map((d) => (
          <tr key={`${d.place.kind}:${d.place.id}`} data-tow={d.place.id} style={{ opacity: d.available ? 1 : 0.55 }}>
            <td>
              <b>{d.place.name}</b> {d.nearest && <Badge tone="ok">nearest</Badge>} {d.place.kind === "Home" && <Badge tone="info">your garage</Badge>}
              <div class="wp-dim" style={{ fontSize: "0.8em" }}>
                {d.city_label} · {d.place.kind === "Garage" ? "repair shop" : "no mechanic: repairs are shop work"}
              </div>
            </td>
            <td class="num wp-mono">{km(d.metres)}</td>
            <td class="num">
              <Money value={d.price} />
            </td>
            <td>
              <Button
                size="sm"
                variant={d.nearest ? "primary" : undefined}
                disabled={!d.available || props.busy}
                testid={`tow-to-${d.place.id}`}
                title={d.available ? (d.nearest && d.price > offer.cash ? "you are short: the rest becomes debt" : undefined) : "not enough cash (only the nearest shop tows on account)"}
                onClick={() => props.onPick(d)}
              >
                Tow
              </Button>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Notes under the list: what happens, and the debt rule. */
export function TowNotes({ offer }: { offer: TowOffer }) {
  if (offer.refused) return null;
  const nearest = offer.destinations.find((d) => d.nearest);
  return (
    <p class="wp-dim" style={{ fontSize: "0.8em", margin: "0.4em 0 0" }}>
      Call-out fee plus a charge per km. The car arrives with its damage; you are then at that place.
      {nearest && nearest.price > offer.cash && <> Short of cash: the nearest shop still tows, and the rest becomes debt.</>}
    </p>
  );
}

/** The in-run "Call a tow truck" pop-up. */
function TowPopup(props: { offer: TowOffer; onPick: (d: TowDestination) => void; onClose: () => void; error: string | null }) {
  const { offer } = props;
  return (
    <Modal title="Call a tow truck" onClose={props.onClose}>
      <div data-testid="tow-picker" style={{ minWidth: "min(34em, 90vw)" }}>
        <p style={{ marginTop: 0 }}>
          {offer.need ? `${TOW_NEED[offer.need]} ` : ""}
          Pick where the {offer.car_name} goes{offer.ends_run ? ": this ends the run" : ""}. Cash <Money value={offer.cash} tone />.
        </p>
        <TowList offer={offer} onPick={props.onPick} />
        <TowNotes offer={offer} />
        {props.error && <p class="wp-danger">{props.error}</p>}
        <div class="wp-row" style={{ justifyContent: "flex-end", marginTop: "0.6em" }}>
          <Button onClick={props.onClose} hotkey="Esc">
            Cancel
          </Button>
        </div>
      </div>
    </Modal>
  );
}

/** Mounts the in-run tow pop-up into `el`. */
export class TowPickerView {
  /** The pop-up is showing. */
  open = false;

  constructor(private readonly el: HTMLElement) {}

  /** Show `offer`; `pick` tows (throws an Error with the reason on refusal); `close` cancels. */
  show(offer: TowOffer, pick: (d: TowDestination) => void, close: () => void, error: string | null = null): void {
    this.open = true;
    const tryPick = (d: TowDestination) => {
      try {
        pick(d);
      } catch (e) {
        this.show(offer, pick, close, String((e as Error).message ?? e).replace(/^(Error: )?wp_wasm: /, ""));
      }
    };
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "auto", zIndex: 85 }}>
        <TowPopup offer={offer} onPick={tryPick} onClose={close} error={error} />
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

/** The in-run prompt when the car cannot be driven on: "The car is stuck. Call a tow truck". */
export class TowPromptView {
  private shown = "";

  constructor(private readonly el: HTMLElement) {}

  /** Call often: `need` null hides it. `key` names the tow key; `call` opens the pop-up. */
  update(need: TowNeed | null, key: string, call: () => void): void {
    const id = need ? `${need}|${key}` : "";
    if (id === this.shown) return;
    this.shown = id;
    render(
      need ? (
        <div class="wp-ui wp-root" style={{ position: "absolute", left: 0, right: 0, bottom: "14%", display: "flex", justifyContent: "center", pointerEvents: "none" }}>
          <div class="wp-panel" data-testid="tow-prompt" style={{ pointerEvents: "auto", display: "flex", gap: "0.8em", alignItems: "center", padding: "0.5em 0.9em" }}>
            <span>{TOW_NEED[need]}</span>
            <Button variant="primary" size="sm" onClick={call} hotkey={key}>
              Call a tow truck
            </Button>
          </div>
        </div>
      ) : null,
      this.el,
    );
  }
}
