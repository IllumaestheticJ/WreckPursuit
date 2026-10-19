/**
 * Demo page (`npm run demo`): a Storybook-style shell around the kit.
 * Left: every screen. Top: mock playback (run, play/pause, speed) and accessibility toggles.
 * The stage shows the selected screen over a fake night-street backdrop so the HUD can be judged.
 *
 * URL params: ?screen=<id>&run=<fixture>&t=<seconds>&palette=<p>&scale=<n>&paused=1
 */
import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { App } from "../src/App";
import { ModelContext, useSlice } from "../src/model/GameModel";
import type { Palette, ScreenId } from "../src/model/types";
import { createMockModel } from "../src/mock/MockProvider";
import { FIXTURES } from "../src/mock/fixtures";
import { SCREENS } from "../src/screens/registry";
import "./demo.css";

const params = new URLSearchParams(location.search);
const model = createMockModel({
  run: params.get("run") ?? "synthetic_pursuit",
  screen: (params.get("screen") as ScreenId) ?? "hud",
  autoplay: params.get("paused") !== "1",
});
if (params.get("t")) model.send({ type: "replay.seek", tick: Math.round(Number(params.get("t")) * 240) });
if (params.get("palette")) model.send({ type: "settings.patch", patch: { accessibility: { palette: params.get("palette") as Palette } } });
if (params.get("scale")) model.send({ type: "settings.patch", patch: { accessibility: { uiScale: Number(params.get("scale")) } } });
// Expose for e2e tests and console poking.
(window as unknown as { wpModel: typeof model }).wpModel = model;

function Shell() {
  const screen = useSlice((s) => s.screen);
  const replay = useSlice((s) => s.replay);
  const a11y = useSlice((s) => s.settings.accessibility);
  const [chrome, setChrome] = useState(params.get("chrome") !== "0");

  useEffect(() => {
    const p = new URLSearchParams(location.search);
    p.set("screen", screen);
    history.replaceState(null, "", `?${p}`);
  }, [screen]);

  const groups = [...new Set(SCREENS.map((s) => s.group))];
  return (
    <div class="demo" data-chrome={chrome}>
      {chrome && (
        <aside class="demo-nav">
          <div class="demo-brand">
            WRECKPURSUIT <span>UI KIT</span>
          </div>
          {groups.map((g) => (
            <div key={g}>
              <div class="demo-group">{g}</div>
              {SCREENS.filter((s) => s.group === g).map((s) => (
                <button type="button" key={s.id} class="demo-link" aria-current={s.id === screen} onClick={() => model.send({ type: "navigate", screen: s.id })}>
                  {s.label}
                </button>
              ))}
            </div>
          ))}
          <div class="demo-group">Mock playback</div>
          <label class="demo-field">
            Run
            <select value={replay.source} onChange={(e) => model.send({ type: "replay.load", source: (e.target as HTMLSelectElement).value })}>
              {FIXTURES.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.label}
                </option>
              ))}
            </select>
          </label>
          <div class="demo-row">
            <button type="button" onClick={() => model.send({ type: replay.playing ? "replay.pause" : "replay.play" })}>
              {replay.playing ? "Pause" : "Play"}
            </button>
            <button type="button" onClick={() => model.send({ type: "replay.seek", tick: 0 })}>
              Restart
            </button>
            <span class="demo-time">{(replay.tick / replay.ticksPerSecond).toFixed(1)}s</span>
          </div>
          <div class="demo-group">Accessibility</div>
          <label class="demo-field">
            Palette
            <select value={a11y.palette} onChange={(e) => model.send({ type: "settings.patch", patch: { accessibility: { palette: (e.target as HTMLSelectElement).value as Palette } } })}>
              {["default", "deuteranopia", "protanopia", "tritanopia", "highContrast"].map((p) => (
                <option key={p}>{p}</option>
              ))}
            </select>
          </label>
          <label class="demo-field">
            Scale {Math.round(a11y.uiScale * 100)}%
            <input type="range" min={0.75} max={1.5} step={0.05} value={a11y.uiScale} onInput={(e) => model.send({ type: "settings.patch", patch: { accessibility: { uiScale: Number((e.target as HTMLInputElement).value) } } })} />
          </label>
          <button type="button" class="demo-link" onClick={() => setChrome(false)}>
            Hide demo chrome (press ` to restore)
          </button>
        </aside>
      )}
      <main class="demo-stage" data-screen={screen}>
        <div class="demo-backdrop" aria-hidden="true" />
        <App model={model} />
      </main>
      <KeyChrome onToggle={() => setChrome((c) => !c)} />
    </div>
  );
}

function KeyChrome({ onToggle }: { onToggle: () => void }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "`" && onToggle();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onToggle]);
  return null;
}

render(
  <ModelContext.Provider value={model}>
    <Shell />
  </ModelContext.Provider>,
  document.getElementById("demo")!,
);
