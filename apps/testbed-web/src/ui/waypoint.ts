// Career waypoint (owner 2026-10-27): dealers, garages and workshops have to be driven to, and
// the career menu can only mark one on the map. This keeps that mark: the session's drive-to
// places (`places_json()`) as map dots, the waypoint as a flag on its place, and a GPS line
// from the car along the lanes (`gps_route`, refreshed every ROUTE_EVERY_MS). Pulling into the
// place clears it.
import type { MapHud } from "../../../ui-kit/src/live";
import type { MapMarker } from "../../../ui-kit/src/map/extras";
import type { CareerPlace } from "../../../ui-kit/src/garage/types";
import type { MapPlace } from "./placeVisit";
import { sim } from "../wasm/sim";

/** GPS line refresh interval (route planning is a lane search over the whole world). */
const ROUTE_EVERY_MS = 1500;

/** Map icon for the places the kit's map does not draw itself (it has its own repair shop,
 * safehouse and police symbols): car lots as a showroom car, a car with a price tag or a wreck
 * stack by kind, the player's garages as a garage door (`drawPoi`). */
function dot(p: MapPlace): Pick<MapMarker, "colour" | "label" | "icon"> | null {
  if (p.kind === "Lot") return { colour: "#ff7ac8", icon: p.icon === "UsedLot" || p.icon === "Junkyard" ? p.icon : "Dealership" };
  if (p.kind === "Home") return { colour: "#f2f2f2", icon: "Home" };
  return null;
}

export class CareerWaypoint {
  /** The place the waypoint marks (kind + id), or null. */
  target: CareerPlace | null = null;
  private places: MapPlace[] = [];
  private route: [number, number][] = [];
  private lastRoute = -Infinity;

  constructor(private readonly map: MapHud) {}

  /** New session: its places (empty outside city sessions with an economy run). */
  reset(places: MapPlace[]): void {
    this.places = places;
    this.route = [];
    this.lastRoute = -Infinity;
    this.draw();
  }

  /** Mark `target` (null clears). */
  set(target: CareerPlace | null): void {
    this.target = target;
    this.route = [];
    this.lastRoute = -Infinity;
    this.draw();
  }

  /** The player pulled into `kind` / `id`: the waypoint is done when it marks that place. */
  arrived(kind: string, id: string): void {
    if (this.target && this.target.kind === kind && this.target.id === id) this.set(null);
  }

  /** The waypoint's place in this session (null: none, or not in this world). */
  private spot(): MapPlace | null {
    const t = this.target;
    return t ? (this.places.find((p) => p.kind === t.kind && p.id === t.id) ?? null) : null;
  }

  /** One frame: refresh the GPS line now and then. */
  frame(nowMs: number): void {
    const at = this.spot();
    if (!at || nowMs - this.lastRoute < ROUTE_EVERY_MS) return;
    this.lastRoute = nowMs;
    try {
      this.route = sim.gpsRoute(at.pos[0], at.pos[1]);
    } catch {
      this.route = [];
    }
    this.draw();
  }

  private draw(): void {
    if (this.places.length === 0) return void this.map.setPlaceExtras(null);
    const at = this.spot();
    this.map.setPlaceExtras({
      markers: this.places.flatMap((p) => {
        const d = dot(p);
        return d ? [{ x: p.pos[0], y: p.pos[1], ...d, hot: p === at }] : [];
      }),
      flags: at ? [{ x: at.pos[0], y: at.pos[1], label: at.name }] : [],
      route: at && this.route.length > 1 ? this.route : null,
    });
  }
}
