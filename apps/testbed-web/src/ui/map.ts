// City map HUD (workstream 16, apps/ui-kit/src/map): the corner minimap (always on, layer
// "minimap") and the full city map (M or Tab). Data from wp_wasm map_static_json() once per
// city and map_dynamic(player) every frame. While the full map is open the testbed sends the
// player's car neutral input with light braking (input paused; the sim keeps running).
import { mountMapHud, type MapHud } from "../../../ui-kit/src/live";
import { layers } from "../settings";
import { W } from "../wasm/layout";
import { sim } from "../wasm/sim";

export class MapOverlay {
  readonly hud: MapHud;
  private city = "";

  constructor(parent: HTMLElement, onOpenChange?: (open: boolean) => void) {
    // Left column, under the input panel: the testbed's bottom-right corner holds the event feed
    // and the audio panel.
    this.hud = mountMapHud(parent, { onOpenChange, minimapPlacement: { left: "8px", top: "312px", right: "auto", bottom: "auto" } });
  }

  /** New session: fetch the static map when the city changed. */
  reset(): void {
    const id = sim.worldInfo()[W.city] > 0.5 ? sim.cityId() : "";
    if (id === this.city) return;
    this.city = id;
    this.hud.setStatic(id ? sim.mapStaticJson() : null);
  }

  /** One frame (after the sim step). */
  frame(player: number, nowMs: number): void {
    if (!this.city) return;
    this.hud.setMinimapVisible(layers.on("minimap"));
    this.hud.setSignals(layers.on("mapSignals"));
    this.hud.frame(sim.mapDynamic(player), nowMs);
  }

  get isOpen(): boolean {
    return this.hud.isOpen;
  }

  toggle(): void {
    this.hud.toggle();
  }
}
