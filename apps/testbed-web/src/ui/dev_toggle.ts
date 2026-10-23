// Debug tools switch (owner, 2026-10-23: "clean up the HUD to be more player friendly, keep the
// debugging tools but hide the buttons when not being used").
//
// One switch shows or hides every debug tool: the developer drawer (#controls, folded or not),
// the debug overlays (telemetry, inputs, component / region / event panels, perf lines, the big
// damage silhouette) and the debug-layer hotkeys. It is the game menu's `developer` setting
// (Settings → Developer panels), so it is saved with the other settings. Off by default.
//
// Which value wins (first match):
//   1. a toggle in this page (debug key, ⚙ button, the drawer's ✕);
//   2. `?dev=1` / `?dev=0`;
//   3. `?menu=1` or the demo build (they open on the title screen): off;
//   4. automation (`navigator.webdriver`, unless `?dev=auto`): on, so the e2e suite keeps
//      clicking the drawer's buttons;
//   5. the saved setting (off unless the player turned it on).
//
// With the switch off ("player mode", `body.wp-player`): only the player HUD shows, the debug
// layers are forced off, the other layers read at their defaults (a stray X no longer hides the
// traffic) and only the reduced-flashing key (F6) stays live. A small ⚙ button fades in at the
// top right while the mouse moves (never on touch or in the demo build) and turns the tools on.

import { SETTINGS_KEY } from "../../../ui-kit/src/live";
import { DEBUG_LAYERS, LAYERS, PLAYER_PINNED_LAYERS, layers, store } from "../settings";
import { keyName } from "../bindings";

/** localStorage: the drawer is folded to its header. */
const FOLD_KEY = "wp.testbed.devFolded.v1";
/** localStorage: the one-time reset of the stored `developer` setting has run. */
const RESET_KEY = "wp.testbed.devReset.v1";
/** The ⚙ button hides this long after the mouse stops, ms. */
const BUTTON_IDLE_MS = 3000;

/** True when a browser automation tool drives the page (Playwright, WebDriver). */
export const isAutomation = (): boolean => typeof navigator !== "undefined" && navigator.webdriver === true;

/** One-time reset (2026-10-23): until now the testbed defaulted to the developer panels, so
 * almost every browser has `developer: true` saved. Turn it off once; a later choice sticks. */
export function resetStoredDeveloperOnce(): void {
  if (store.get(RESET_KEY)) return;
  store.set(RESET_KEY, "1");
  const text = store.get(SETTINGS_KEY);
  if (!text) return;
  try {
    const saved = JSON.parse(text) as Record<string, unknown>;
    if (saved && typeof saved === "object" && saved.developer === true) {
      saved.developer = false;
      store.set(SETTINGS_KEY, JSON.stringify(saved));
    }
  } catch {
    // Unreadable settings: the menu falls back to its defaults anyway.
  }
}

/** Options of the debug switch. */
export interface DevSwitchOptions {
  /** URL parameters of the page (`dev`). */
  params: URLSearchParams;
  /** The page opens on the title screen (`?menu=1` or the demo build): off unless `?dev=1`. */
  titleOnLoad: boolean;
  /** The HUD overlay (the ⚙ button goes here). */
  overlay: HTMLElement;
  /** Ask for the switch to change (the host saves it as the `developer` setting and calls
   * `apply` with the new value). */
  request: (on: boolean) => void;
  /** Called after every `apply` with the resulting state (per-mode HUD details). */
  onApply?: (on: boolean) => void;
}

/** The debug tools switch, the developer drawer's header / layer row and the ⚙ button. */
export class DevSwitch {
  /** Debug tools shown right now. */
  on = false;
  /** Default of the saved setting (first run, menu reset): on only under automation. */
  readonly defaultOn: boolean;
  private readonly params: URLSearchParams;
  /** A toggle made in this page (wins over the URL). */
  private override: boolean | null = null;
  /** The saved setting at the last `apply` (null before the first). */
  private lastSaved: boolean | null = null;
  private readonly button: HTMLButtonElement;
  private lastMouse = -Infinity;
  private layerBoxes: HTMLInputElement[] = [];
  private lastSync = -Infinity;

  constructor(private readonly opts: DevSwitchOptions) {
    this.params = opts.params;
    this.defaultOn = !opts.titleOnLoad && this.automationOn();
    this.button = document.createElement("button");
    this.button.id = "dev-toggle";
    this.button.type = "button";
    this.button.textContent = "⚙";
    this.button.title = "debug tools (` key)";
    this.button.setAttribute("aria-label", "show debug tools");
    this.button.onclick = () => this.toggle();
    opts.overlay.appendChild(this.button);
    window.addEventListener("mousemove", () => (this.lastMouse = performance.now()), { passive: true });
    this.mountDrawer();
  }

  private automationOn(): boolean {
    return isAutomation() && this.params.get("dev") !== "auto";
  }

  /** The state for a saved `developer` setting (see the file comment for the order). */
  resolve(saved: boolean): boolean {
    if (this.override !== null) return this.override;
    const p = this.params.get("dev");
    if (p === "1") return true;
    if (p === "0") return false;
    if (this.opts.titleOnLoad) return false;
    if (this.automationOn()) return true;
    return saved;
  }

  /** Apply the saved setting (from the game menu's settings); returns the resulting state. */
  apply(saved: boolean): boolean {
    // The saved setting changed in this page (Settings → Developer panels, or `toggle`): that
    // choice wins over the URL and automation from now on.
    if (this.lastSaved !== null && saved !== this.lastSaved) this.override = saved;
    this.lastSaved = saved;
    return this.set(this.resolve(saved));
  }

  /** Show or hide the debug tools now. */
  private set(on: boolean): boolean {
    this.on = on;
    document.body.classList.toggle("wp-player", !on);
    if (on) document.body.classList.remove("demo");
    layers.suppress(on ? [] : DEBUG_LAYERS);
    layers.pin(on ? [] : PLAYER_PINNED_LAYERS);
    this.syncLayers();
    this.opts.onApply?.(on);
    return on;
  }

  /** Flip the switch (debug key, ⚙, ✕). The choice is saved as the `developer` setting. */
  toggle(): void {
    const next = !this.on;
    this.override = next;
    this.opts.request(next);
    // The saved value may already be `next` (the URL or automation had overridden it): then
    // nothing re-applies the settings, so apply here.
    if (this.on !== next) this.set(next);
  }

  /** Once per frame: `covered` = a menu or full-screen panel is up (the player HUD hides). */
  frame(nowMs: number, covered: boolean): void {
    const body = document.body;
    if (body.classList.contains("wp-covered") !== covered) body.classList.toggle("wp-covered", covered);
    const show = !this.on && !covered && nowMs - this.lastMouse < BUTTON_IDLE_MS;
    if (this.button.classList.contains("shown") !== show) this.button.classList.toggle("shown", show);
    if (this.on && nowMs - this.lastSync > 250) this.syncLayers(nowMs);
  }

  /** Drawer folded to its header (saved per viewer). */
  get folded(): boolean {
    return document.body.classList.contains("wp-dev-folded");
  }

  setFolded(folded: boolean): void {
    document.body.classList.toggle("wp-dev-folded", folded);
    store.set(FOLD_KEY, folded ? "1" : "");
    const fold = document.getElementById("dev-fold");
    if (fold) {
      fold.textContent = folded ? "»" : "«";
      fold.title = folded ? "unfold the developer drawer" : "fold the developer drawer (the overlays stay)";
    }
  }

  /** Header (section links, fold, close) and the layer row of the developer drawer. All the
   * drawer's existing controls keep their ids and stay in one scrolling column. */
  private mountDrawer(): void {
    const drawer = document.getElementById("controls");
    if (!drawer) return;
    const head = document.createElement("div");
    head.className = "dev-head";
    const title = document.createElement("b");
    title.textContent = "developer";
    head.append(title);
    const jump = (label: string, target: () => Element | null | undefined) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "dev-jump";
      b.textContent = label;
      b.onclick = () => {
        if (this.folded) this.setFolded(false);
        target()?.scrollIntoView({ block: "start" });
      };
      head.append(b);
    };
    const rowOf = (id: string) => document.getElementById(id)?.closest(".row");
    jump("session", () => rowOf("sc-select"));
    jump("car", () => rowOf("mode"));
    jump("replay", () => rowOf("rec"));
    jump("layers", () => document.getElementById("layer-row"));
    const fold = document.createElement("button");
    fold.type = "button";
    fold.id = "dev-fold";
    fold.onclick = () => this.setFolded(!this.folded);
    const close = document.createElement("button");
    close.type = "button";
    close.id = "dev-close";
    close.textContent = "✕";
    close.title = "hide all debug tools (` key; ⚙ top right brings them back)";
    close.onclick = () => this.toggle();
    head.append(fold, close);
    drawer.prepend(head);

    // Layer checkboxes (until now the layers were hotkeys only, listed in F1).
    const row = document.createElement("div");
    row.className = "row";
    row.id = "layer-row";
    row.append("layers");
    for (const l of LAYERS) {
      const label = document.createElement("label");
      label.title = `${l.label} (key ${keyName(l.key)})`;
      const box = document.createElement("input");
      box.type = "checkbox";
      box.dataset.layer = l.id;
      box.onchange = () => {
        if (box.checked !== layers.saved(l.id)) layers.toggle(l.id);
      };
      const short = l.label.split(/ \(|:/)[0];
      label.append(box, `${short.length > 24 ? `${short.slice(0, 23)}…` : short}`);
      row.append(label);
      this.layerBoxes.push(box);
    }
    const replayRow = rowOf("rec");
    if (replayRow) drawer.insertBefore(row, replayRow);
    else drawer.append(row);
    this.setFolded(store.get(FOLD_KEY) === "1");
  }

  /** Layer checkboxes follow the saved toggles (hotkeys change them too). */
  private syncLayers(nowMs = performance.now()): void {
    this.lastSync = nowMs;
    for (const box of this.layerBoxes) {
      const want = layers.saved(box.dataset.layer!);
      if (box.checked !== want) box.checked = want;
    }
  }
}
