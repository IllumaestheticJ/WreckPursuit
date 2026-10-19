/**
 * Mount API for host apps (the browser testbed, later the game client).
 *
 * ```ts
 * import { mountUi, createMockModel } from "@wreckpursuit/ui-kit";
 * const ui = mountUi(document.getElementById("ui")!, { model: createMockModel() });
 * // HUD only, over the 3D canvas:
 * const hud = mountUi(overlayEl, { model, screen: "hud", globalKeys: false });
 * ui.unmount();
 * ```
 *
 * The element should be positioned (`position: relative/absolute/fixed`): the kit fills it.
 */
import { render } from "preact";
import { App } from "./App";
import type { GameModel } from "./model/GameModel";
import type { ScreenId } from "./model/types";

/** Options for [`mountUi`]. */
export interface MountOptions {
  /** The data source: `createMockModel()` today, the sim/net bridge later. */
  model: GameModel;
  /** Pin one screen (e.g. `"hud"` as an overlay). Default: follow `UiState.screen`. */
  screen?: ScreenId;
  /** Let the kit handle Escape (pause / resume). Default true; pass false when the host owns input. */
  globalKeys?: boolean;
}

/** Handle returned by [`mountUi`]. */
export interface UiHandle {
  /** Remove the UI from the element. */
  unmount(): void;
  /** Re-render with a different pinned screen (or `undefined` to follow the model). */
  setScreen(screen: ScreenId | undefined): void;
}

/** Render the UI kit into `el`. */
export function mountUi(el: HTMLElement, opts: MountOptions): UiHandle {
  let screen = opts.screen;
  const draw = () => render(<App model={opts.model} screen={screen} globalKeys={opts.globalKeys} />, el);
  draw();
  return {
    unmount: () => render(null, el),
    setScreen: (s) => {
      screen = s;
      draw();
    },
  };
}
