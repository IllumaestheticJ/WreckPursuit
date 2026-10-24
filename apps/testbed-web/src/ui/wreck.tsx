// Tow or limp (feature F17): after a wreck nobody is chasing, with the car still able to move,
// the car is held and this panel asks: "Tow" (the towing charge; the run is over and the
// end-of-run panel follows, as with an automatic tow) or "Limp to a shop" (no towing charge;
// drive the wreck to a garage yourself). Keys: T / pad A = tow, L / pad B = limp. While
// limping a line reminds the player where they are going. wp_wasm `wreck_offer_json()`,
// `wreck_tow()`, `wreck_limp()`.
import { render } from "preact";
import { Button, Money, Panel } from "../../../ui-kit/src/ui/kit";
import "../../../ui-kit/src/ui/theme.css";

/** `wreck_offer_json()`. */
export interface WreckState {
  offer: { tick: number; tow_fee: number; cash: number; career: boolean } | null;
  limping: boolean;
}

export class WreckView {
  /** The open offer (e2e reads it). */
  state: WreckState = { offer: null, limping: false };
  private key = "";
  private error: string | null = null;
  private padWas = [true, true];

  constructor(
    private readonly el: HTMLElement,
    private readonly actions: { tow: () => void; limp: () => void },
  ) {
    window.addEventListener(
      "keydown",
      (e) => {
        if (!this.state.offer || e.repeat) return;
        if (e.code === "KeyT") this.tow();
        else if (e.code === "KeyL") this.limp();
        else return;
        e.preventDefault();
        e.stopImmediatePropagation();
      },
      true,
    );
  }

  /** Call every frame with `wreck_offer_json()`. */
  update(state: WreckState): void {
    if (state.offer) this.pollPad();
    else this.padWas = [true, true];
    const key = `${state.offer?.tick ?? "-"}|${state.limping}|${this.error ?? ""}`;
    this.state = state;
    if (key === this.key) return;
    this.key = key;
    this.draw();
  }

  /** New session. */
  reset(): void {
    this.state = { offer: null, limping: false };
    this.key = "";
    this.error = null;
    render(null, this.el);
  }

  tow(): void {
    try {
      this.actions.tow();
      this.error = null;
    } catch (e) {
      this.error = String(e).replace(/^Error: /, "");
    }
    this.key = "";
  }

  limp(): void {
    try {
      this.actions.limp();
      this.error = null;
    } catch (e) {
      this.error = String(e).replace(/^Error: /, "");
    }
    this.key = "";
  }

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
    if (a && !wasA) this.tow();
    else if (b && !wasB) this.limp();
  }

  private draw(): void {
    const s = this.state;
    if (s.offer) {
      const o = s.offer;
      render(
        <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }} data-testid="wreck-offer">
          <div style={{ position: "absolute", left: "50%", top: "22%", transform: "translateX(-50%)", width: "min(340px, calc(100vw - 32px))", pointerEvents: "auto", zIndex: 72 }}>
            <Panel title="Wrecked" tag="nobody chasing">
              <p style={{ marginTop: 0 }}>The car still moves. Pay for a tow, or limp it to a garage yourself and save the towing charge.</p>
              {this.error && <p class="wp-danger" style={{ fontSize: "0.85em" }}>{this.error}</p>}
              <div style={{ display: "flex", gap: "0.5em", flexWrap: "wrap" }}>
                <Button variant="primary" onClick={() => this.tow()} hotkey="T" testid="wreck-tow">
                  Tow (<Money value={o.tow_fee} />)
                </Button>
                <Button onClick={() => this.limp()} hotkey="L" testid="wreck-limp">
                  Limp to a shop
                </Button>
              </div>
              <p class="wp-dim" style={{ fontSize: "0.8em" }}>{o.career ? "A tow ends the run at your garage." : "A tow ends the run."}</p>
            </Panel>
          </div>
        </div>,
        this.el,
      );
    } else if (s.limping) {
      render(
        <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
          <div class="wp-toast" data-kind="warn" data-testid="wreck-limping">
            Limping a wreck: drive to a garage (the G icons on the map; click one for a route)
          </div>
        </div>,
        this.el,
      );
    } else {
      render(null, this.el);
    }
  }
}
