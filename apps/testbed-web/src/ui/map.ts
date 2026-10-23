// City map HUD (workstream 16, apps/ui-kit/src/map): the corner minimap (always on, layer
// "minimap") and the full city map (M or Tab). Data from wp_wasm map_static_json() once per
// city and map_dynamic(player) every frame. While the full map is open the testbed sends the
// player's car neutral input with light braking (input paused; the sim keeps running).
import { mountMapHud, type MapHud } from "../../../ui-kit/src/live";
import { chaseRadar, decodeMapDynamic } from "../../../ui-kit/src/map/data";
import type { MapBlip, Vec2 } from "../../../ui-kit/src/model/types";

/** The chase HUD's radar (ChaseHud, PR #32): the player's pose and the blips around them. */
export type Radar = { pos: Vec2; headingRad: number; blips: MapBlip[] };
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

  constructor(parent: HTMLElement, onOpenChange?: (open: boolean) => void) {
    // Bottom-right corner (playtest row 67): clear of the developer panel (bottom-left), the race
    // HUD (top-left), the pursuit banner (centre) and the damage silhouette (top-right).
    this.hud = mountMapHud(parent, { onOpenChange, minimapPlacement: { right: "8px", bottom: "8px", left: "auto", top: "auto" } });
  }

  /** New session: fetch the static map when the city changed. */
  reset(): void {
    this.lastFrame = -Infinity;
    this.lastRadar = null;
    const id = sim.worldInfo()[W.city] > 0.5 ? sim.cityId() : "";
    if (id === this.city) return;
    this.city = id;
    this.hud.setStatic(id ? sim.mapStaticJson() : null);
  }

  /** One frame (after the sim step). `mayRest`: the frame loop is slow (software GL, < 8 fps)
   * and this frame may skip the minimap redraw (its canvas upload + composite cost ~10 ms there). */
  frame(player: number, nowMs: number, mayRest = false): void {
    if (!this.city) return;
    const minimap = layers.on("minimap");
    this.hud.setMinimapVisible(minimap);
    this.hud.setSignals(layers.on("mapSignals"));
    // The chase HUD's radar reads the same frame (PR #32).
    const radar = layers.on("chase");
    // Full map open: every frame (pan / zoom); minimap / radar only: throttled; none: nothing.
    if (!this.hud.isOpen && (!(minimap || radar) || mayRest || nowMs - this.lastFrame < MINIMAP_EVERY_MS)) return;
    this.lastFrame = nowMs;
    // Decoded once for both (the map HUD takes a decoded frame).
    const f = decodeMapDynamic(sim.mapDynamic(player));
    if (this.hud.isOpen || minimap) this.hud.frame(f, nowMs);
    this.lastRadar = radar ? chaseRadar(f) : null;
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
