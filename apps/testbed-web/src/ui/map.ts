// City map HUD (workstream 16, apps/ui-kit/src/map): the corner minimap (always on, layer
// "minimap") and the full city map (M or Tab). Data from wp_wasm map_static_json() once per
// city and map_dynamic(player) every frame. While the full map is open the testbed sends the
// player's car neutral input with light braking (input paused; the sim keeps running).
import { mountMapHud, type MapHud } from "../../../ui-kit/src/live";
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

  constructor(parent: HTMLElement, onOpenChange?: (open: boolean) => void) {
    // Bottom-right corner (playtest row 67): clear of the developer panel (bottom-left), the race
    // HUD (top-left), the pursuit banner (centre) and the damage silhouette (top-right).
    this.hud = mountMapHud(parent, { onOpenChange, minimapPlacement: { right: "8px", bottom: "8px", left: "auto", top: "auto" } });
  }

  /** New session: fetch the static map when the city changed. */
  reset(): void {
    this.lastFrame = -Infinity;
    const id = sim.worldInfo()[W.city] > 0.5 ? sim.cityId() : "";
    if (id === this.city) return;
    this.city = id;
    this.hud.setStatic(id ? sim.mapStaticJson() : null);
  }

  /** One frame (after the sim step). */
  frame(player: number, nowMs: number): void {
    if (!this.city) return;
    const minimap = layers.on("minimap");
    this.hud.setMinimapVisible(minimap);
    this.hud.setSignals(layers.on("mapSignals"));
    // Full map open: every frame (pan / zoom); minimap only: throttled; neither: nothing to draw.
    if (!this.hud.isOpen && (!minimap || nowMs - this.lastFrame < MINIMAP_EVERY_MS)) return;
    this.lastFrame = nowMs;
    this.hud.frame(sim.mapDynamic(player), nowMs);
  }

  get isOpen(): boolean {
    return this.hud.isOpen;
  }

  toggle(): void {
    this.hud.toggle();
  }
}
