/**
 * Light mount helpers for hosts that only want a few screens over their own UI (the testbed).
 * Unlike `mountUi` they do not import the screen registry, so the bundle carries no fixtures.
 *
 * ```ts
 * import { createLiveModel, mountOverlay } from "../../ui-kit/src/live";
 * const model = createLiveModel({ quote: ... });
 * const hud = mountOverlay(el, model, "chaseHud");
 * const bill = mountOverlay(billEl, model, "repairBill");
 * ```
 */
import type { JSX } from "preact";
import { render } from "preact";
import { ModelContext, useSlice, type GameModel } from "../model/GameModel";
import { ChaseHud } from "../screens/hud/ChaseHud";
import { RepairBill } from "../screens/garage/RepairBill";
import "../ui/theme.css";

/** Screens available as overlays. */
export type OverlayScreen = "chaseHud" | "repairBill";

const OVERLAYS: Record<OverlayScreen, () => JSX.Element> = { chaseHud: ChaseHud, repairBill: RepairBill };

function Root({ screen }: { screen: OverlayScreen }) {
  const a11y = useSlice((s) => s.settings.accessibility);
  const notice = useSlice((s) => s.notice);
  const Screen = OVERLAYS[screen];
  return (
    <div
      class="wp-ui wp-root"
      data-palette={a11y.palette}
      data-reduce-motion={a11y.reduceMotion}
      data-screen={screen}
      style={{ "--ui-scale": String(a11y.uiScale), position: "absolute", inset: 0, pointerEvents: screen === "repairBill" ? "auto" : "none" } as JSX.CSSProperties}
    >
      <Screen />
      {notice && screen === "repairBill" && (
        <div class="wp-toast" data-kind={notice.kind}>
          {notice.text}
        </div>
      )}
    </div>
  );
}

/** Render one overlay screen into `el` (which must be positioned). Returns an unmount function. */
export function mountOverlay(el: HTMLElement, model: GameModel, screen: OverlayScreen): () => void {
  render(
    <ModelContext.Provider value={model}>
      <Root screen={screen} />
    </ModelContext.Provider>,
    el,
  );
  return () => render(null, el);
}
