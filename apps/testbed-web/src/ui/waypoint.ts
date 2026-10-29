// Career waypoint (owner 2026-10-27): dealers, garages and workshops have to be driven to, and
// the career menu can only mark one on the map. This keeps that mark: the session's drive-to
// places (`places_json()`) as map dots, the waypoint as a flag on its place, and a GPS line
// from the car along the lanes (`gps_route`, refreshed every ROUTE_EVERY_MS). Pulling into the
// place clears it.
//
// Free Roam (owner 2026-10-29: "need to be able to set waypoints in free roam too"): while the
// city map (M) is open in a career run, a click on a place icon marks that place, a click on
// the map anywhere else drops a pin on the nearest road (`road_point`); a click on the waypoint
// again, or the bar's "Clear", clears it. It is the same waypoint the career screen sets
// (`CareerApi.waypoint` / `setWaypoint`). A road pin clears when the car gets within
// ROAD_ARRIVE_M of it. In the world the waypoint stands as a gold beam (`SceneView.setWaypoint`).
import type { MapHud } from "../../../ui-kit/src/live";
import type { MapMarker } from "../../../ui-kit/src/map/extras";
import type { WaypointTarget } from "../../../ui-kit/src/garage/types";
import type { MapPlace } from "./placeVisit";
import { sim } from "../wasm/sim";

/** GPS line refresh interval (route planning is a lane search over the whole world). */
const ROUTE_EVERY_MS = 1500;
/** A map click this close to a place (m) marks the place rather than the road. */
const PICK_PLACE_M = 45;
/** A map click this close to the waypoint (m) clears it. */
const PICK_CLEAR_M = 30;
/** A road pin is reached (and cleared) this close, m. */
const ROAD_ARRIVE_M = 20;

/** Map icon for the places the kit's map does not draw itself (it has its own repair shop,
 * safehouse and police symbols): car lots as a showroom car, a car with a price tag or a wreck
 * stack by kind, the player's garages as a garage door (`drawPoi`). */
function dot(p: MapPlace): Pick<MapMarker, "colour" | "label" | "icon"> | null {
  if (p.kind === "Lot") return { colour: "#ff7ac8", icon: p.icon === "UsedLot" || p.icon === "Junkyard" ? p.icon : "Dealership" };
  if (p.kind === "Home") return { colour: "#f2f2f2", icon: "Home" };
  return null;
}

/** Where the waypoint is, as the map and the world draw it. */
interface Spot {
  name: string;
  pos: [number, number, number];
}

export class CareerWaypoint {
  /** The place or road spot the waypoint marks, or null. */
  target: WaypointTarget | null = null;
  /** Called after every change of the target (the 3D beam follows it). */
  onChange: ((at: Spot | null) => void) | null = null;
  private places: MapPlace[] = [];
  private route: [number, number][] = [];
  private lastRoute = -Infinity;
  private armed = false;
  private bar: HTMLElement | null = null;

  constructor(private readonly map: MapHud) {}

  /** New session: its places (empty outside city sessions with an economy run). */
  reset(places: MapPlace[]): void {
    this.places = places;
    this.route = [];
    this.lastRoute = -Infinity;
    this.draw();
  }

  /** Mark `target` (null clears). */
  set(target: WaypointTarget | null): void {
    this.target = target;
    this.route = [];
    this.lastRoute = -Infinity;
    this.draw();
  }

  /** The player pulled into `kind` / `id`: the waypoint is done when it marks that place. */
  arrived(kind: string, id: string): void {
    if (this.target && this.target.kind === kind && this.target.id === id) this.set(null);
  }

  /** The waypoint's spot in this session (null: none, or a place not in this world). */
  private spot(): Spot | null {
    const t = this.target;
    if (!t) return null;
    if (t.kind === "Road") return t.pos ? { name: t.name, pos: t.pos } : null;
    return this.places.find((p) => p.kind === t.kind && p.id === t.id) ?? null;
  }

  /** One frame of a career run: refresh the GPS line now and then; `player` (`[x, y]`, null
   * unknown) reaching a road pin clears it. */
  frame(nowMs: number, player: [number, number] | null = null): void {
    const at = this.spot();
    if (at && this.target?.kind === "Road" && player && Math.hypot(player[0] - at.pos[0], player[1] - at.pos[1]) < ROAD_ARRIVE_M) {
      return this.set(null);
    }
    if (!at || nowMs - this.lastRoute < ROUTE_EVERY_MS) return;
    this.lastRoute = nowMs;
    try {
      this.route = sim.gpsRoute(at.pos[0], at.pos[1]);
    } catch {
      this.route = [];
    }
    this.draw();
  }

  /** Every frame: the Free Roam map picker is armed while the full map is open in a career run
   * (`career`) and nothing else picks on it (`busy`: the street-race finish picker). */
  mapFrame(career: boolean, busy: boolean): void {
    const want = career && this.map.isOpen && !busy && this.places.length > 0;
    if (want && !this.armed) {
      this.armed = true;
      this.map.setPicker((x, y) => this.pick(x, y));
    } else if (!want && this.armed) {
      this.armed = false;
      // The street picker took over: leave its picker alone.
      if (!busy) this.map.setPicker(null);
    }
    this.drawBar(want);
  }

  /** A map pick: the place under it, else (on the waypoint) clear, else a road pin. */
  pick(x: number, y: number): void {
    const near = (p: [number, number, number] | number[]) => Math.hypot(p[0] - x, p[1] - y);
    const at = this.spot();
    let best: MapPlace | null = null;
    for (const p of this.places) if (near(p.pos) < PICK_PLACE_M && (!best || near(p.pos) < near(best.pos))) best = p;
    if (best) {
      const same = this.target?.kind === best.kind && this.target.id === best.id;
      return this.set(same ? null : { kind: best.kind, id: best.id, name: best.name });
    }
    if (at && near(at.pos) < PICK_CLEAR_M) return this.set(null);
    let road: [number, number, number] | null = null;
    try {
      road = sim.roadPoint(x, y);
    } catch {
      road = null;
    }
    if (road) this.set({ kind: "Road", id: "road", name: "Map pin", pos: road });
  }

  /** The bar at the foot of the open city map: how to pick, and "Clear". */
  private drawBar(show: boolean): void {
    const host = this.map.panel.parentElement;
    if (!host) return;
    if (!this.bar) {
      const bar = document.createElement("div");
      bar.className = "wp-panel wp-mono";
      bar.dataset.testid = "map-waypoint";
      bar.style.cssText =
        "position:absolute;left:50%;bottom:12px;transform:translateX(-50%);max-width:calc(100% - 32px);display:flex;gap:0.6em;align-items:center;flex-wrap:wrap;font-size:0.85em;padding:0.4em 0.7em;pointer-events:auto;";
      host.appendChild(bar);
      this.bar = bar;
    }
    const bar = this.bar;
    bar.hidden = !show;
    if (!show) return;
    const t = this.target;
    const key = t ? `${t.kind}|${t.id}|${t.pos?.join(",") ?? ""}` : "";
    if (bar.dataset.key === key) return;
    bar.dataset.key = key;
    bar.textContent = "";
    const text = document.createElement("span");
    text.textContent = t ? `Waypoint: ${t.name}. Click a road or a place to move it, or the flag to clear it.` : "Click a road or a place icon to set a waypoint.";
    bar.appendChild(text);
    if (t) {
      const clear = document.createElement("button");
      clear.className = "wp-btn wp-btn--sm";
      clear.dataset.testid = "map-waypoint-clear";
      clear.textContent = "Clear";
      clear.onclick = () => this.set(null);
      bar.appendChild(clear);
    }
  }

  private draw(): void {
    const at = this.spot();
    this.onChange?.(this.places.length > 0 ? at : null);
    if (this.places.length === 0) return void this.map.setPlaceExtras(null);
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
