// City map HUD (workstream 16, apps/ui-kit/src/map): the corner minimap (always on, layer
// "minimap") and the full city map (M or Tab). Data from wp_wasm map_static_json() once per
// city and map_dynamic(player) every frame. While the full map is open the testbed sends the
// player's car neutral input with light braking (input paused; the sim keeps running).
import { mountMapHud, type MapHud } from "../../../ui-kit/src/live";
import { chaseRadar, decodeMapDynamic, type MapPoi, type MapStatic } from "../../../ui-kit/src/map/data";
import type { MapBlip, Vec2 } from "../../../ui-kit/src/model/types";

/** The chase HUD's radar (ChaseHud, PR #32): the player's pose and the blips around them. */
export type Radar = { pos: Vec2; headingRad: number; blips: MapBlip[]; hidden?: boolean };
import { layers } from "../settings";
import { W } from "../wasm/layout";
import { sim } from "../wasm/sim";

/** Minimap redraw interval while the full map is closed (ms): 30 Hz is smooth for a heading-up
 * map and halves its canvas + map_dynamic cost at 60 fps (browser perf pass). */
const MINIMAP_EVERY_MS = 33;

export class MapOverlay {
  readonly hud: MapHud;
  private city = "";
  private lastFrame = -Infinity;
  /** The chase radar from the last map_dynamic() fetch (null: none yet / nothing to show). */
  private lastRadar: Radar | null = null;
  /** The city's static map as `map_static_json()` gave it (POIs before the session's own). */
  private cityMap: MapStatic | null = null;
  /** Hide the corner minimap for now (a picker race draws its own in the race panel). */
  minimapHeld = false;

  constructor(parent: HTMLElement, onOpenChange?: (open: boolean) => void) {
    // Bottom-right corner (playtest row 67): clear of the developer panel (bottom-left), the race
    // HUD (top-left), the pursuit banner (centre) and the damage silhouette (top-right).
    this.hud = mountMapHud(parent, { onOpenChange, minimapPlacement: { right: "8px", bottom: "8px", left: "auto", top: "auto" } });
    // F40: tap a shop, safehouse or race meet on the full map for a GPS route there (tap the
    // same one again to clear it).
    this.hud.setPoiHandler((poi) => this.routeTo(poi));
  }

  /** The player the last frame was drawn for (GPS routes start at their car). */
  private player = 0;
  /** The GPS goal (F40) and the drive left, or null. */
  gps: { name: string; x: number; y: number; length_m: number } | null = null;

  /** Set (or, for the current goal, clear) a GPS route to `poi`. */
  routeTo(poi: MapPoi): void {
    if (this.gps && this.gps.name === poi.name) return this.clearGps();
    const r = sim.gpsRoute(this.player, poi.pos[0], poi.pos[1]);
    if (!r) return this.clearGps();
    this.gps = { name: poi.name, x: r.finish[0], y: r.finish[1], length_m: r.length_m };
    this.hud.setGps(r.polyline);
  }

  /** Drop the GPS route. */
  clearGps(): void {
    this.gps = null;
    this.hud.setGps(null);
  }

  /** New session: fetch the static map when the city changed, and put this session's shops
   * and race meets on it (garage audit): garages from `garages()` where a stop opens a visit
   * (legit / crooked colour; none in picker races or sessions without an economy), race meets
   * from `race_spots()` instead of the city's `RaceStartGrid` POIs (many are scenario route
   * ends with no race, and some race meets had no icon). */
  reset(): void {
    this.clearGps();
    this.lastFrame = -Infinity;
    this.lastRadar = null;
    const id = sim.worldInfo()[W.city] > 0.5 ? sim.cityId() : "";
    if (id !== this.city) {
      this.city = id;
      this.cityMap = id ? (JSON.parse(sim.mapStaticJson()) as MapStatic | null) : null;
    }
    this.hud.setStatic(this.cityMap ? { ...this.cityMap, pois: sessionPois(this.cityMap.pois) } : null);
  }

  /** One frame (after the sim step). `mayRest`: the frame loop is slow (software GL, < 8 fps)
   * and this frame may skip the minimap redraw (its canvas upload + composite cost ~10 ms there). */
  frame(player: number, nowMs: number, mayRest = false): void {
    if (!this.city) {
      // No city (flat test world): no empty minimap ring.
      this.hud.setMinimapVisible(false);
      return;
    }
    const minimap = layers.on("minimap") && !this.minimapHeld;
    this.hud.setMinimapVisible(minimap);
    this.hud.setSignals(layers.on("mapSignals"));
    // The chase HUD's radar reads the same frame (PR #32).
    const radar = layers.on("chase");
    // Full map open: every frame (pan / zoom); minimap / radar only: throttled; none: nothing.
    if (!this.hud.isOpen && (!(minimap || radar) || mayRest || nowMs - this.lastFrame < MINIMAP_EVERY_MS)) return;
    this.lastFrame = nowMs;
    // Decoded once for both (the map HUD takes a decoded frame).
    const f = decodeMapDynamic(sim.mapDynamic(player));
    this.player = player;
    // Arrived (within 25 m of the goal): the GPS route goes away.
    if (this.gps && f.player && Math.hypot(f.player.x - this.gps.x, f.player.y - this.gps.y) < 25) this.clearGps();
    if (this.hud.isOpen || minimap) this.hud.frame(f, nowMs);
    // The corner minimap already shows all of it: then the chase HUD only keeps its "on you"
    // note (lead, 2026-10-23).
    const r = radar ? chaseRadar(f) : null;
    this.lastRadar = r ? { ...r, hidden: minimap } : null;
  }

  /** The chase radar (police cars, racers and the helicopter with its follow tether) from the
   * latest map frame; null when there is nothing around or the chase layer is off. */
  radar(): Radar | null {
    return this.lastRadar;
  }

  get isOpen(): boolean {
    return this.hud.isOpen;
  }

  toggle(): void {
    this.hud.toggle();
  }
}

/** The city's POIs as this session uses them: shops at their street-lane spot with their kind,
 * race meets from the session's race spots, everything else as the city gives it. */
export function sessionPois(city: MapPoi[]): MapPoi[] {
  const out = city.filter((p) => p.kind !== "Garage" && p.kind !== "RaceStartGrid");
  for (const g of sim.garages()) out.push({ kind: "Garage", name: g.name, pos: [g.pos[0], g.pos[1]], shop: g.kind });
  for (const r of sim.raceSpots()) out.push({ kind: "RaceSpot", name: r.name, pos: [r.x, r.y] });
  return out;
}
