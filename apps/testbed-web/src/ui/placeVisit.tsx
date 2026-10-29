// Career places: stopping inside a garage or a safehouse ring on a career run (wp_wasm
// `place_json()`, see crates/wp_wasm/src/garage_visit.rs) offers what the place is for while the
// car stays in the ring (it can still drive: driving out closes the panel), drawn with the UI
// kit's primitives:
//  - garage: "Workshop" (work on the car part by part) or "Garage" (repairs, mods, tuning, cars);
//  - safehouse: "End the night here" (the run ends safely and the career is saved) or "Workshop";
//  - car lot: "Walk the lot" (the lot visit: browse and buy);
//  - the player's own garage (career runs start in it): "Workshop", "Garage" (cars, mods, tuning)
//    or "End the night here";
//  - any: "Back on the road" gives control back (drive out of the ring before it reopens).
// Dealers, garages and workshops are only reached this way (owner 2026-10-27): the career menu
// only sets a waypoint to them.
// During a pursuit a safehouse cannot be entered, but laying low inside its ring shakes the police
// (the police safehouse rule); a strip shows how long is left.
import { render } from "preact";
import { Badge, Button, Meter, Panel } from "../../../ui-kit/src/ui/kit";
import "../../../ui-kit/src/ui/theme.css";

/** `wp_wasm::garage_visit::SafehouseSite`. */
export interface SafehouseSite {
  name: string;
  pos: [number, number, number];
  radius_m: number;
}

/** `wp_wasm::garage_visit::PlaceKind`. */
export type PlaceKind = "Garage" | "Safehouse" | "Lot" | "Home";

/** `wp_wasm::garage_visit::PlaceVisit`. */
export interface PlaceVisit {
  seq: number;
  kind: PlaceKind;
  name: string;
  /** Lot / home garage id (null for shops and safehouses). */
  id: string | null;
  shop: "Legit" | "Crooked" | null;
  wanted: string;
}

/** `wp_wasm::garage_visit::MapPlace` (`places_json()`): a drive-to place on the map. */
export interface MapPlace {
  kind: PlaceKind;
  /** Lot / home id, else the name. */
  id: string;
  name: string;
  pos: [number, number, number];
  radius_m: number;
  shop: "Legit" | "Crooked" | null;
  /** Map icon (`ui-kit` `drawPoi` kind). */
  icon: "Garage" | "Safehouse" | "Dealership" | "UsedLot" | "Junkyard" | "Home";
}

/** `wp_wasm::garage_visit::PlaceStatus`. */
export interface PlaceStatus {
  visit: PlaceVisit | null;
  laying_low: { name: string; dwell_s: number; need_s: number; clear_radius_m: number } | null;
}

/** What the panel's buttons do (main.ts ends the run into the garage screens). */
export interface PlaceActions {
  /** End the run here and open the workshop with the car. */
  workshop: (v: PlaceVisit) => void;
  /** End the run here and open the career garage (a safehouse or home: end the night). */
  garage: (v: PlaceVisit) => void;
  /** End the run here and walk the car lot `v.id`. */
  lot: (v: PlaceVisit) => void;
  /** Give the car back. */
  leave: () => void;
}

const TAG: Record<PlaceKind, string> = { Garage: "GARAGE", Safehouse: "SAFEHOUSE", Lot: "CAR LOT", Home: "YOUR GARAGE" };

const BLURB: Record<PlaceKind, string> = {
  Garage: "Pull in to work on the car yourself, or have the shop repair, upgrade and tune it.",
  Safehouse: "Your hideout. Park up for the night (the run ends and your career is saved), or work on the car.",
  Lot: "Park up and walk the rows: today's stock, ready to buy.",
  Home: "Your own garage. Work on the car, swap cars, or park up for the night.",
};

function PlacePanel(props: { visit: PlaceVisit; actions: PlaceActions }) {
  const v = props.visit;
  const a = props.actions;
  const tag = v.kind === "Garage" && v.shop ? `${TAG.Garage} · ${v.shop.toUpperCase()}` : TAG[v.kind];
  return (
    <div style={{ position: "absolute", left: "1em", top: "4em", width: "min(320px, calc(100vw - 32px))", pointerEvents: "auto", zIndex: 70 }}>
      <Panel title={v.name} tag={tag}>
        <p class="wp-dim" style={{ margin: "0 0 0.6em" }}>
          {BLURB[v.kind]}
        </p>
        {v.wanted !== "None" && (
          <p style={{ margin: "0 0 0.6em" }}>
            <Badge tone="warn">wanted {v.wanted}</Badge> <span class="wp-dim">the heat stays with the car until it cools off</span>
          </p>
        )}
        <div style={{ display: "grid", gap: "0.4em" }}>
          {v.kind === "Safehouse" && (
            <>
              <Button variant="primary" testid="place-end-night" onClick={() => a.garage(v)}>
                End the night here
              </Button>
              <Button testid="place-workshop" onClick={() => a.workshop(v)}>
                Workshop
              </Button>
            </>
          )}
          {v.kind === "Garage" && (
            <>
              <Button variant="primary" testid="place-workshop" onClick={() => a.workshop(v)}>
                Workshop
              </Button>
              <Button testid="place-garage" onClick={() => a.garage(v)} title="repairs, mods, tuning, your cars">
                Garage: repair, mods, tuning
              </Button>
            </>
          )}
          {v.kind === "Home" && (
            <>
              <Button variant="primary" testid="place-workshop" onClick={() => a.workshop(v)}>
                Workshop
              </Button>
              <Button testid="place-garage" onClick={() => a.garage(v)} title="your cars, mods, tuning; the run ends here">
                Garage: cars, mods, tuning
              </Button>
            </>
          )}
          {v.kind === "Lot" && (
            <Button variant="primary" testid="place-lot" onClick={() => a.lot(v)}>
              Walk the lot
            </Button>
          )}
          <Button testid="place-leave" onClick={a.leave}>
            Back on the road
          </Button>
        </div>
      </Panel>
    </div>
  );
}

function LayLowStrip(props: { low: NonNullable<PlaceStatus["laying_low"]> }) {
  const l = props.low;
  return (
    <div style={{ position: "absolute", left: "50%", top: "4.5em", transform: "translateX(-50%)", width: "min(360px, calc(100vw - 32px))", pointerEvents: "none", zIndex: 70 }}>
      <Panel title={`Laying low · ${l.name}`}>
        <p class="wp-dim" style={{ margin: "0 0 0.4em" }}>
          Stay inside with no police within {Math.round(l.clear_radius_m)} m to lose them.
        </p>
        <Meter value={l.dwell_s / Math.max(0.1, l.need_s)} label="hidden" />
      </Panel>
    </div>
  );
}

/** Mounts the place panel into `el` and keeps it in sync with `place_json()`. */
export class PlaceVisitView {
  /** The open visit (e2e reads it). */
  visit: PlaceVisit | null = null;
  private low = false;
  private lastDrawLow = 0;

  constructor(
    private readonly el: HTMLElement,
    private readonly actions: PlaceActions,
  ) {}

  /** Call every frame with `place_json()` (null outside career runs). */
  update(status: PlaceStatus | null, nowMs: number): void {
    const visit = status?.visit ?? null;
    const low = status?.laying_low ?? null;
    if (visit?.seq !== this.visit?.seq || !!low !== this.low || (low && nowMs - this.lastDrawLow > 150)) {
      this.visit = visit;
      this.low = !!low;
      this.lastDrawLow = nowMs;
      this.draw(status);
    }
  }

  /** New session: forget everything shown. */
  reset(): void {
    this.visit = null;
    this.low = false;
    render(null, this.el);
  }

  private draw(status: PlaceStatus | null): void {
    const v = status?.visit ?? null;
    const low = status?.laying_low ?? null;
    if (!v && !low) return void render(null, this.el);
    const wrap = (a: PlaceActions): PlaceActions => ({
      workshop: (x) => (this.reset(), a.workshop(x)),
      garage: (x) => (this.reset(), a.garage(x)),
      lot: (x) => (this.reset(), a.lot(x)),
      leave: () => (this.reset(), a.leave()),
    });
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none" }} data-testid="place-visit">
        {v && <PlacePanel visit={v} actions={wrap(this.actions)} />}
        {low && <LayLowStrip low={low} />}
      </div>,
      this.el,
    );
  }
}
