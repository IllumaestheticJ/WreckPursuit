/**
 * Root component: applies accessibility settings (palette, scale, motion) and renders the
 * current screen (or a fixed one, for hosts that embed a single screen such as the HUD).
 */
import type { JSX } from "preact";
import { useEffect } from "preact/hooks";
import { ModelContext, useSend, useSlice, type GameModel } from "./model/GameModel";
import type { ScreenId } from "./model/types";
import { SCREENS } from "./screens/registry";
import { Button } from "./ui/kit";
import "./ui/theme.css";

/** Props for [`App`]. */
export interface AppProps {
  model: GameModel;
  /** Render this screen regardless of `UiState.screen`. */
  screen?: ScreenId;
  /** Handle Escape (pause/back) and other global keys. Default true. */
  globalKeys?: boolean;
}

/** The kit root. */
export function App(props: AppProps): JSX.Element {
  return (
    <ModelContext.Provider value={props.model}>
      <Root fixed={props.screen} globalKeys={props.globalKeys ?? true} />
    </ModelContext.Provider>
  );
}

function Root({ fixed, globalKeys }: { fixed?: ScreenId; globalKeys: boolean }) {
  const current = useSlice((s) => s.screen);
  const a11y = useSlice((s) => s.settings.accessibility);
  const screen = fixed ?? current;
  const Screen = SCREENS.find((s) => s.id === screen)?.component ?? SCREENS[0].component;
  const send = useSend();

  useEffect(() => {
    if (!globalKeys || fixed) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".wp-modal-back")) return;
      if ((e.target as HTMLElement | null)?.closest?.("[data-capturing]")) return;
      if (screen === "hud") send({ type: "pause" });
      else if (screen === "pause") send({ type: "resume" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [screen, globalKeys, fixed]);

  return (
    <div
      class="wp-ui wp-root"
      data-palette={a11y.palette}
      data-reduce-motion={a11y.reduceMotion}
      data-large-scanner={a11y.largeScannerText}
      data-screen={screen}
      style={{ "--ui-scale": String(a11y.uiScale), position: "absolute", inset: 0 } as JSX.CSSProperties}
    >
      <Screen />
      <Toast />
    </div>
  );
}

function Toast() {
  const notice = useSlice((s) => s.notice);
  const send = useSend();
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => send({ type: "notice.dismiss" }), notice.kind === "error" ? 4000 : 2500);
    return () => clearTimeout(t);
  }, [notice?.id]);
  if (!notice) return null;
  return (
    <div class="wp-toast" data-kind={notice.kind} role={notice.kind === "error" ? "alert" : "status"}>
      <span>{notice.text}</span>
      <Button size="sm" variant="ghost" onClick={() => send({ type: "notice.dismiss" })}>
        ✕
      </Button>
    </div>
  );
}
