/**
 * Game menu for players (owner request 2026-10-20, playtest row 68): main menu, pause menu and
 * settings, keyboard + gamepad navigable. Host-agnostic: the host (the browser testbed today)
 * starts sessions, pauses its sim and applies settings through `GameMenuHost`.
 *
 * ```ts
 * import { mountGameMenu, defaultGameSettings, loadGameSettings } from "../../ui-kit/src/live";
 * const menu = mountGameMenu(overlay, { host, settings: loadGameSettings(defaultGameSettings()), cameras });
 * menu.openMain();             // title screen
 * menu.openPause();            // Esc / gamepad Start while driving
 * ```
 *
 * While open the menu owns the keyboard (a capture-phase listener that stops every keydown from
 * reaching the game) and reads the first gamepad itself; the host should block driving input in
 * `onOpenChange(true)`.
 */
import type { ComponentChildren } from "preact";
import { Fragment, render } from "preact";
import { keyAction, PadNav, stepIndex, type NavAction } from "./nav";
import { ASSIST_LEVELS, DRAW_DISTANCES, defaultGameSettings, saveGameSettings, type AssistLevel, type GameSettings, type RadioFilter, type SettingsStorage, type Volumes } from "./settings";
import { CONTROL_DEVICES, CONTROL_RANGES, defaultDeviceControls, type ControlDevice, type DeviceControls, type InputProbe } from "./controls";
import { paintInputBars } from "./bars";
import "../ui/theme.css";
import "./menu.css";

/** What the main menu can start. */
export type PlayChoice = "chase" | "chaseRain" | "races" | "free" | "garage";

/** Which screen is showing ("closed" = none). */
export type MenuScreen = "closed" | "main" | "pause" | "settings";

/** What the host does for the menu. */
export interface GameMenuHost {
  /** Main menu choice: start that session (the menu has already closed). */
  play(choice: PlayChoice): void;
  /** Pause → Restart: the same session again (the menu has already closed). */
  restart(): void;
  /** Pause → Garage (career). The menu has already closed. */
  garage(): void;
  /** Show "Garage" in the pause menu (e.g. only when a career exists). Default: always. */
  canGarage?(): boolean;
  /** Pause → Quit to main menu: end the run (the main menu is shown next). */
  quitToMain(): void;
  /** Open the controls (rebinding) panel; the menu hides until `controlsOpen()` turns false. */
  openControls(): void;
  /** Is the controls panel still open? */
  controlsOpen(): boolean;
  /** Apply settings live. `prev` is null for the first call (apply everything). */
  apply(next: GameSettings, prev: GameSettings | null): void;
  /** The menu opened / closed / changed screen (pause the sim and block input while open). */
  onOpenChange?(open: boolean, screen: MenuScreen): void;
  /** Processed driving input right now (after the device's shaping), for the Controls test
   * bars; polled every animation frame while the settings screen is open. Must work while
   * game input is blocked by the menu and must not fire any actions. null = nothing to show. */
  inputProbe?(): InputProbe | null;
}

/** Mount options. */
export interface GameMenuOptions {
  host: GameMenuHost;
  /** Initial settings (usually `loadGameSettings(defaultGameSettings(dev))`). */
  settings: GameSettings;
  /** Camera angles for "default camera" (host ids + labels). */
  cameras?: { id: string; label: string }[];
  /** The host can set the countersteer strength (machine A's wasm setter); else the slider is disabled. */
  countersteer?: boolean;
  /** Developer default used by "Reset to defaults". */
  developerDefault?: boolean;
  /** Storage for persistence (default localStorage; null = none). */
  storage?: SettingsStorage | null;
}

/** Element that Left / Right adjust (cycle buttons, toggles). */
type Steppable = HTMLElement & { wpStep?: (dir: 1 | -1) => void };

/** The mounted menu. */
export class GameMenu {
  /** Current settings (the menu's copy; persisted on every change). */
  settings: GameSettings;
  /** Showing screen. */
  screen: MenuScreen = "closed";
  /** The controls panel is open on top (the menu is hidden meanwhile). */
  suspended = false;
  private settingsFrom: "main" | "pause" = "pause";
  private readonly root: HTMLElement;
  private readonly pad = new PadNav();
  private raf = 0;
  private focusId: string | null = null;
  private countersteer: boolean;
  /** Which device's profile the Controls section edits. */
  private controlsDevice: ControlDevice = "keyboard";
  /** "Test input": navigation keys are ignored (except Esc) so driving keys move the bars. */
  private testInput = false;
  private lastProbeDevice: ControlDevice | null = null;
  private readonly onKey = (e: KeyboardEvent) => this.key(e);

  constructor(
    parent: HTMLElement,
    private readonly opts: GameMenuOptions,
  ) {
    this.settings = opts.settings;
    this.countersteer = !!opts.countersteer;
    this.root = document.createElement("div");
    this.root.id = "wp-game-menu";
    this.root.className = "wp-ui wp-root wp-gm-root";
    this.root.hidden = true;
    parent.appendChild(this.root);
    opts.host.apply(this.settings, null);
  }

  /** Is "Test input" on (navigation keys ignored except Esc)? */
  get testingInput(): boolean {
    return this.testInput;
  }

  /** Which device the Controls section edits. */
  get editedDevice(): ControlDevice {
    return this.controlsDevice;
  }

  /** Turn "Test input" on or off (Esc also turns it off). */
  setTestInput(on: boolean): void {
    if (this.testInput === on) return;
    this.testInput = on;
    this.draw();
    // Nothing keeps focus while testing, so Space / Enter cannot press a button.
    if (on) (document.activeElement as HTMLElement | null)?.blur?.();
    else document.getElementById("gm-ctl-test")?.focus();
  }

  /** Choose the device whose profile the Controls section edits. */
  editDevice(d: ControlDevice): void {
    this.controlsDevice = d;
    this.draw();
  }

  /** A screen is showing (the controls panel opened from the menu counts as open). */
  get isOpen(): boolean {
    return this.screen !== "closed";
  }

  /** Title screen. */
  openMain(): void {
    this.show("main", "gm-play-chase");
  }

  /** Pause menu (pauses the host via onOpenChange). */
  openPause(): void {
    this.show("pause", "gm-resume");
  }

  /** Settings, returning to `from`. */
  openSettings(from: "main" | "pause" = this.screen === "main" ? "main" : "pause"): void {
    this.settingsFrom = from;
    this.show("settings", null);
  }

  /** Close whatever is showing (pause → the game resumes). */
  close(): void {
    if (this.screen === "closed") return;
    this.screen = "closed";
    this.suspended = false;
    this.testInput = false;
    this.detach();
    this.root.hidden = true;
    render(null, this.root);
    this.opts.host.onOpenChange?.(false, "closed");
  }

  /** The countersteer setter became available (or went away). */
  setCountersteerAvailable(on: boolean): void {
    this.countersteer = on;
    this.draw();
  }

  /** Change settings from outside (e.g. the developer panel's assist boxes); applies + saves. */
  update(patch: Partial<GameSettings>): void {
    this.change({ ...this.settings, ...patch });
  }

  /** Remove from the page. */
  destroy(): void {
    this.close();
    this.root.remove();
  }

  // ----- internals ---------------------------------------------------------------------------

  private show(screen: Exclude<MenuScreen, "closed">, focus: string | null): void {
    const wasOpen = this.isOpen;
    if (screen === "settings" && this.screen !== "settings") {
      // Edit the profile of the device in use.
      const p = this.probe();
      if (p) this.controlsDevice = p.device;
    }
    this.screen = screen;
    this.suspended = false;
    this.testInput = false;
    this.focusId = focus;
    this.root.hidden = false;
    this.draw();
    this.attach();
    this.focusFirst();
    if (!wasOpen || screen !== "settings") this.opts.host.onOpenChange?.(true, screen);
  }

  private change(next: GameSettings): void {
    const prev = this.settings;
    this.settings = next;
    saveGameSettings(next, this.opts.storage === undefined ? undefined : this.opts.storage);
    this.opts.host.apply(next, prev);
    this.draw();
  }

  private attach(): void {
    window.removeEventListener("keydown", this.onKey, true);
    window.addEventListener("keydown", this.onKey, true);
    const p = firstPad();
    this.pad.rest(p?.buttons.map((b) => b.value) ?? [], p?.axes ?? []);
    cancelAnimationFrame(this.raf);
    this.raf = requestAnimationFrame(() => this.poll());
  }

  private detach(): void {
    window.removeEventListener("keydown", this.onKey, true);
    cancelAnimationFrame(this.raf);
  }

  private poll(): void {
    if (!this.isOpen) return;
    this.raf = requestAnimationFrame(() => this.poll());
    if (this.suspended) {
      if (this.opts.host.controlsOpen()) return;
      // Back from the controls panel: take the keyboard again and re-block the game.
      this.suspended = false;
      this.root.hidden = false;
      window.addEventListener("keydown", this.onKey, true);
      const p = firstPad();
      this.pad.rest(p?.buttons.map((b) => b.value) ?? [], p?.axes ?? []);
      this.opts.host.onOpenChange?.(true, this.screen);
      this.focusFirst();
      return;
    }
    if (this.screen === "settings") this.paintProbe();
    const p = firstPad();
    if (!p) return;
    for (const a of this.pad.update(
      p.buttons.map((b) => b.value),
      p.axes,
      performance.now(),
    ))
      this.nav(a);
  }

  private key(e: KeyboardEvent): void {
    if (!this.isOpen || this.suspended) return;
    // The game never sees a key while the menu is open.
    e.stopImmediatePropagation();
    if (this.testInput) {
      // Test input: driving keys only move the test bars; Esc ends the test.
      if (e.code === "Escape") {
        e.preventDefault();
        this.setTestInput(false);
      } else if (keyAction(e.code) || e.code === "Tab") e.preventDefault();
      return;
    }
    let a = keyAction(e.code);
    if (e.code === "Tab") a = e.shiftKey ? "up" : "down";
    if (!a) return;
    e.preventDefault();
    // Enter / Space on a focused button: let the browser's own click happen once, not twice.
    if (a === "activate" && e.repeat) return;
    this.nav(a);
  }

  /** Apply one navigation action. */
  nav(a: NavAction): void {
    if (this.testInput) {
      // The pad drives the bars; B / Start end the test.
      if (a === "back" || a === "start") this.setTestInput(false);
      return;
    }
    const items = this.items();
    const cur = items.indexOf(document.activeElement as HTMLElement);
    const el = cur >= 0 ? items[cur] : null;
    switch (a) {
      case "up":
      case "down": {
        const i = stepIndex(cur, items.length, a === "down" ? 1 : -1);
        items[i]?.focus();
        items[i]?.scrollIntoView?.({ block: "nearest" });
        break;
      }
      case "left":
      case "right": {
        const dir = a === "right" ? 1 : -1;
        if (el instanceof HTMLInputElement && el.type === "range") {
          const step = Number(el.step) || 0.05;
          const v = Math.min(Number(el.max), Math.max(Number(el.min), Number(el.value) + dir * step));
          el.value = String(Math.round(v / step) * step);
          el.dispatchEvent(new Event("input", { bubbles: true }));
        } else if (el && (el as Steppable).wpStep) (el as Steppable).wpStep!(dir);
        break;
      }
      case "activate":
        if (el && !(el instanceof HTMLInputElement && el.type === "range")) el.click();
        else if (!el) items[0]?.focus();
        break;
      case "back":
        this.back();
        break;
      case "start":
        if (this.screen === "pause" || (this.screen === "settings" && this.settingsFrom === "pause")) this.close();
        break;
    }
  }

  private back(): void {
    if (this.screen === "settings") this.show(this.settingsFrom, "gm-settings");
    else if (this.screen === "pause") this.close();
  }

  private probe(): InputProbe | null {
    try {
      return this.opts.host.inputProbe?.() ?? null;
    } catch {
      return null;
    }
  }

  /** Paint the Controls test bars from the host's probe (every frame on the settings screen). */
  private paintProbe(): void {
    const box = this.root.querySelector<HTMLElement>(".wp-gm-bars");
    if (!box) return;
    const p = this.probe();
    paintInputBars(box, p);
    if ((p?.device ?? null) !== this.lastProbeDevice) {
      this.lastProbeDevice = p?.device ?? null;
      const inUse = this.root.querySelector<HTMLElement>("#gm-ctl-inuse");
      if (inUse) inUse.textContent = p ? `in use: ${DEVICE_LABEL[p.device]}` : "no input";
    }
  }

  private items(): HTMLElement[] {
    return [...this.root.querySelectorAll<HTMLElement>("[data-nav]")].filter((e) => !(e as HTMLButtonElement).disabled && e.offsetParent !== null);
  }

  private focusFirst(): void {
    const items = this.items();
    const want = this.focusId ? items.find((e) => e.id === this.focusId) : null;
    (want ?? items[0])?.focus();
  }

  private act(f: () => void): void {
    this.close();
    f();
  }

  private draw(): void {
    if (this.screen === "closed") return;
    const h = this.opts.host;
    let body: ComponentChildren;
    if (this.screen === "main") {
      body = (
        <MainScreen
          play={(c) => this.act(() => h.play(c))}
          settings={() => this.openSettings("main")}
          controls={() => this.controls()}
        />
      );
    } else if (this.screen === "pause") {
      body = (
        <PauseScreen
          garage={h.canGarage ? h.canGarage() : true}
          resume={() => this.close()}
          restart={() => this.act(() => h.restart())}
          openGarage={() => this.act(() => h.garage())}
          settings={() => this.openSettings("pause")}
          controls={() => this.controls()}
          quit={() => {
            h.quitToMain();
            this.openMain();
          }}
        />
      );
    } else {
      body = (
        <SettingsScreen
          s={this.settings}
          cameras={this.opts.cameras ?? []}
          countersteer={this.countersteer}
          change={(s) => this.change(s)}
          reset={() => this.change(defaultGameSettings(this.opts.developerDefault ?? true))}
          device={this.controlsDevice}
          editDevice={(d) => this.editDevice(d)}
          testInput={this.testInput}
          setTestInput={(on) => this.setTestInput(on)}
          back={() => this.back()}
        />
      );
    }
    this.lastProbeDevice = null;
    render(
      <div class="wp-gm" data-screen={this.screen} onMouseOver={(e) => hoverFocus(e.target as HTMLElement)}>
        {body}
      </div>,
      this.root,
    );
  }

  private controls(): void {
    this.suspended = true;
    this.root.hidden = true;
    window.removeEventListener("keydown", this.onKey, true);
    this.opts.host.openControls();
  }
}

/** Mount the game menu into `parent` (a positioned element). Closed until `openMain` / `openPause`. */
export function mountGameMenu(parent: HTMLElement, opts: GameMenuOptions): GameMenu {
  return new GameMenu(parent, opts);
}

function firstPad(): Gamepad | null {
  try {
    for (const p of navigator.getGamepads?.() ?? []) if (p && p.connected !== false) return p;
  } catch {
    // Gamepad API blocked (permissions policy): keyboard and mouse only.
  }
  return null;
}

/** Pointer hover moves the keyboard focus, so mouse and keys never point at two items. */
function hoverFocus(t: HTMLElement | null): void {
  const el = t?.closest?.("[data-nav]") as HTMLElement | null;
  if (el && el !== document.activeElement && !(el as HTMLButtonElement).disabled) el.focus({ preventScroll: true });
}

// ----- screens --------------------------------------------------------------------------------

function Item(props: { id: string; label: string; hint?: string; primary?: boolean; danger?: boolean; onClick: () => void }) {
  return (
    <button type="button" id={props.id} data-nav class={`wp-gm-item${props.primary ? " is-primary" : ""}${props.danger ? " is-danger" : ""}`} onClick={props.onClick}>
      <span class="wp-gm-item-label">{props.label}</span>
      {props.hint && <small>{props.hint}</small>}
    </button>
  );
}

function Brand(props: { kicker: string }) {
  return (
    <div class="wp-gm-brand">
      <div class="wp-mono wp-gm-kicker">{props.kicker}</div>
      <h1>
        WRECK<span>PURSUIT</span>
      </h1>
    </div>
  );
}

const PLAY: { id: PlayChoice; label: string; hint: string }[] = [
  { id: "chase", label: "City chase", hint: "Night in Port Vesper. You're speeding; the police will come." },
  { id: "chaseRain", label: "Rainy night", hint: "Same chase on a soaked road" },
  { id: "races", label: "Street races", hint: "Races against AI drivers" },
  { id: "free", label: "Free drive", hint: "Explore the city with traffic" },
  { id: "garage", label: "Garage", hint: "Your career: cars, repairs, mods" },
];

function MainScreen(props: { play: (c: PlayChoice) => void; settings: () => void; controls: () => void }) {
  return (
    <div class="wp-gm-card wp-gm-main" role="dialog" aria-label="Main menu">
      <Brand kicker="// PORT VESPER · NIGHT SHIFT" />
      <p class="wp-gm-tag">Street racing, soft-body crashes and police chases in a living city.</p>
      <nav class="wp-gm-list" aria-label="Main menu">
        {PLAY.map((p, i) => (
          <Item key={p.id} id={`gm-play-${p.id}`} label={p.label} hint={p.hint} primary={i === 0} onClick={() => props.play(p.id)} />
        ))}
        <Item id="gm-settings" label="Settings" hint="Graphics · audio · assists · units" onClick={props.settings} />
        <Item id="gm-controls" label="Controls" hint="Keyboard and gamepad bindings" onClick={props.controls} />
      </nav>
      <Keys />
    </div>
  );
}

function Keys() {
  return (
    <div class="wp-gm-keys">
      <b>W / S</b>
      <span>throttle / brake</span>
      <b>A / D</b>
      <span>steer</span>
      <b>Space</b>
      <span>handbrake</span>
      <b>C · R · M</b>
      <span>camera · reset car · city map</span>
      <b>Esc</b>
      <span>pause menu · gamepad Start</span>
    </div>
  );
}

function PauseScreen(props: {
  garage: boolean;
  resume: () => void;
  restart: () => void;
  openGarage: () => void;
  settings: () => void;
  controls: () => void;
  quit: () => void;
}) {
  return (
    <div class="wp-gm-card wp-gm-pause" role="dialog" aria-label="Paused">
      <Brand kicker="// PAUSED" />
      <nav class="wp-gm-list" aria-label="Pause menu">
        <Item id="gm-resume" label="Resume" primary onClick={props.resume} />
        <Item id="gm-restart" label="Restart" onClick={props.restart} />
        {props.garage && <Item id="gm-garage" label="Garage" hint="Career: ends the current career run" onClick={props.openGarage} />}
        <Item id="gm-settings" label="Settings" onClick={props.settings} />
        <Item id="gm-controls" label="Controls" onClick={props.controls} />
        <Item id="gm-quit" label="Quit to main menu" danger onClick={props.quit} />
      </nav>
      <div class="wp-gm-foot wp-mono">Esc / Start resume · ↑↓ move · Enter / A select</div>
    </div>
  );
}

function Row(props: { label: string; hint?: string; children: ComponentChildren }) {
  return (
    <div class="wp-gm-row">
      <div class="wp-gm-row-label">
        {props.label}
        {props.hint && <small>{props.hint}</small>}
      </div>
      <div class="wp-gm-row-ctl">{props.children}</div>
    </div>
  );
}

/** Off / Half / Full choices for the TCS and ABS rows (reference game levels). */
const LEVEL_OPTIONS = ASSIST_LEVELS.map((l) => ({ id: l, label: l }));
const RADIO_FILTER_OPTIONS: { id: RadioFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "important", label: "Important only" },
];
const TCS_HINT: Record<AssistLevel, string> = {
  Off: "The throttle you press: wheelspin is yours",
  Half: "Lets the wheels spin a little (slides, smoke)",
  Full: "Barely any wheelspin",
};
const ABS_HINT: Record<AssistLevel, string> = {
  Off: "Wheels lock under hard braking",
  Half: "Short lock-ups, still steers",
  Full: "No lock-ups, shortest stops",
};

/** A value picked from a list: click / Enter / Right = next, Left = previous. */
function Cycle<T extends string | number>(props: { id: string; label: string; value: T; options: { id: T; label: string }[]; onChange: (v: T) => void }) {
  const i = Math.max(
    0,
    props.options.findIndex((o) => o.id === props.value),
  );
  const step = (dir: 1 | -1) => props.onChange(props.options[(i + dir + props.options.length) % props.options.length].id);
  return (
    <button
      type="button"
      id={props.id}
      data-nav
      class="wp-gm-cycle"
      aria-label={`${props.label}: ${props.options[i]?.label}`}
      ref={(el) => {
        if (el) (el as Steppable).wpStep = step;
      }}
      onClick={() => step(1)}
    >
      <span aria-hidden="true">◀</span>
      <b>{props.options[i]?.label}</b>
      <span aria-hidden="true">▶</span>
    </button>
  );
}

/** On / off: click / Enter toggles, Left = off, Right = on. */
function Toggle(props: { id: string; label: string; value: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      type="button"
      id={props.id}
      data-nav
      role="switch"
      aria-checked={props.value}
      aria-label={props.label}
      class={`wp-gm-switch${props.value ? " is-on" : ""}`}
      ref={(el) => {
        if (el) (el as Steppable).wpStep = (dir) => props.onChange(dir > 0);
      }}
      onClick={() => props.onChange(!props.value)}
    >
      <i />
      <b>{props.value ? "On" : "Off"}</b>
    </button>
  );
}

function Slider(props: { id: string; label: string; value: number; min?: number; max?: number; disabled?: boolean; onChange: (v: number) => void }) {
  return (
    <span class="wp-gm-slider">
      <input
        id={props.id}
        data-nav
        type="range"
        min={props.min ?? 0}
        max={props.max ?? 1}
        step={0.05}
        value={props.value}
        disabled={props.disabled}
        aria-label={props.label}
        onInput={(e) => props.onChange(Number((e.target as HTMLInputElement).value))}
      />
      <span class="wp-mono">{Math.round(props.value * 100)}%</span>
    </span>
  );
}

const VOLUMES: { id: keyof Volumes; label: string }[] = [
  { id: "master", label: "Master" },
  { id: "engine", label: "Engine" },
  { id: "effects", label: "Effects" },
  { id: "police", label: "Police radio" },
  { id: "ambience", label: "Ambience" },
];

function SettingsScreen(props: {
  s: GameSettings;
  cameras: { id: string; label: string }[];
  countersteer: boolean;
  change: (s: GameSettings) => void;
  reset: () => void;
  back: () => void;
  device: ControlDevice;
  editDevice: (d: ControlDevice) => void;
  testInput: boolean;
  setTestInput: (on: boolean) => void;
}) {
  const { s, change } = props;
  const assist = (k: keyof GameSettings["assists"], v: boolean | number | AssistLevel) => change({ ...s, assists: { ...s.assists, [k]: v } });
  const cams = [{ id: "", label: "Default" }, ...props.cameras];
  return (
    <div class="wp-gm-card wp-gm-settings" role="dialog" aria-label="Settings">
      <div class="wp-gm-head">
        <h2>Settings</h2>
        <span class="wp-mono wp-dim">saved on this device · applies now</span>
      </div>
      <div class="wp-gm-scroll">
        <h3>Graphics</h3>
        <Row label="Quality" hint="Anti-aliasing changes after a reload">
          <Cycle
            id="gm-gfx"
            label="Quality"
            value={s.gfx}
            options={[
              { id: "auto", label: "Auto" },
              { id: "high", label: "High" },
              { id: "low", label: "Low" },
            ]}
            onChange={(gfx) => change({ ...s, gfx })}
          />
        </Row>
        <Row label="Draw distance">
          <Cycle
            id="gm-draw"
            label="Draw distance"
            value={s.drawDistanceM}
            options={DRAW_DISTANCES.map((m) => ({ id: m as number, label: m === 0 ? "From quality" : `${m} m` }))}
            onChange={(drawDistanceM) => change({ ...s, drawDistanceM })}
          />
        </Row>
        <h3>Audio</h3>
        {VOLUMES.map((v) => (
          <Row key={v.id} label={v.label}>
            <Slider id={`gm-vol-${v.id}`} label={`${v.label} volume`} value={s.volumes[v.id]} onChange={(x) => change({ ...s, volumes: { ...s.volumes, [v.id]: x } })} />
          </Row>
        ))}
        <Row label="Police radio voice" hint={s.radioVoice ? "Units and dispatch speak their lines" : "Squelch and text only"}>
          <Toggle id="gm-radio-voice" label="Police radio voice" value={s.radioVoice} onChange={(radioVoice) => change({ ...s, radioVoice })} />
        </Row>
        <Row label="Radio" hint={s.radioFilter === "important" ? "Warnings, heat changes and crashes only" : "Every transmission"}>
          <Cycle id="gm-radio-filter" label="Radio" value={s.radioFilter} options={RADIO_FILTER_OPTIONS} onChange={(radioFilter) => change({ ...s, radioFilter })} />
        </Row>
        <h3>Assists</h3>
        <Row label="ABS" hint={ABS_HINT[s.assists.abs]}>
          <Cycle id="gm-abs" label="ABS" value={s.assists.abs} options={LEVEL_OPTIONS} onChange={(v) => assist("abs", v)} />
        </Row>
        <Row label="Traction control" hint={TCS_HINT[s.assists.tcs]}>
          <Cycle id="gm-tcs" label="Traction control" value={s.assists.tcs} options={LEVEL_OPTIONS} onChange={(v) => assist("tcs", v)} />
        </Row>
        <Row label="Stability control">
          <Toggle id="gm-esc" label="Stability control" value={s.assists.esc} onChange={(v) => assist("esc", v)} />
        </Row>
        <Row label="Automatic gears">
          <Toggle id="gm-auto" label="Automatic gears" value={s.assists.autoGears} onChange={(v) => assist("autoGears", v)} />
        </Row>
        <Row label="Steering help" hint="Less steering lock at speed">
          <Toggle id="gm-steer" label="Steering help" value={s.assists.steerHelp} onChange={(v) => assist("steerHelp", v)} />
        </Row>
        <Row label="Countersteer help" hint={props.countersteer ? "Hands off in a slide: the wheels follow it (up to 30 % lock)" : "Coming soon (needs a vehicle update)"}>
          <Slider id="gm-cstr" label="Countersteer help" value={s.assists.countersteer} disabled={!props.countersteer} onChange={(v) => assist("countersteer", v)} />
        </Row>
        <ControlsSection
          s={s}
          change={change}
          device={props.device}
          editDevice={props.editDevice}
          testInput={props.testInput}
          setTestInput={props.setTestInput}
        />
        <h3>Gameplay</h3>
        <Row label="Damage" hint={s.damageMode === "Simulation" ? "One hard hit at speed is a total loss" : "Punishing but survivable"}>
          <Cycle
            id="gm-mode"
            label="Damage mode"
            value={s.damageMode}
            options={[
              { id: "Arcade", label: "Arcade" },
              { id: "Simulation", label: "Simulation" },
            ]}
            onChange={(damageMode) => change({ ...s, damageMode })}
          />
        </Row>
        <Row label="Speed units">
          <Cycle
            id="gm-units"
            label="Speed units"
            value={s.units}
            options={[
              { id: "kmh", label: "km/h" },
              { id: "mph", label: "mph" },
            ]}
            onChange={(units) => change({ ...s, units })}
          />
        </Row>
        <h3>Camera</h3>
        <Row label="Default camera" hint="C changes it while driving">
          <Cycle id="gm-camera" label="Default camera" value={s.camera} options={cams} onChange={(camera) => change({ ...s, camera })} />
        </Row>
        <h3>Developer</h3>
        <Row label="Developer panels" hint="Scenario menu, telemetry, event feed, audio mixer">
          <Toggle id="gm-dev" label="Developer panels" value={s.developer} onChange={(developer) => change({ ...s, developer })} />
        </Row>
      </div>
      <div class="wp-gm-actions">
        <button type="button" id="gm-reset" data-nav class="wp-gm-item is-small" onClick={props.reset}>
          Reset to defaults
        </button>
        <button type="button" id="gm-back" data-nav class="wp-gm-item is-small is-primary" onClick={props.back}>
          Back
        </button>
      </div>
    </div>
  );
}

// ----- controls (per-device input shaping + test bars) ----------------------------------------

const DEVICE_LABEL: Record<ControlDevice, string> = { keyboard: "Keyboard", gamepad: "Gamepad", wheel: "Wheel" };

/** One slider of the Controls section. */
interface ControlRow {
  key: keyof DeviceControls;
  label: string;
  hint: string;
}

const CONTROL_ROWS: ControlRow[] = [
  { key: "steerDeadzone", label: "Steering dead zone", hint: "Ignore small steering input near centre" },
  { key: "steerSaturation", label: "Steering saturation", hint: "Full lock is reached at this much input" },
  { key: "steerSensitivity", label: "Steering sensitivity", hint: "100 % = linear · lower = finer near centre" },
  { key: "steerSpeedSensitivity", label: "Steering speed sensitivity", hint: "Less steering lock at speed (keyboard default 75 %)" },
  { key: "throttleSaturation", label: "Accelerator saturation", hint: "Full throttle at this much pedal" },
  { key: "throttleDeadzone", label: "Accelerator dead zone", hint: "Ignore the first part of the pedal" },
  { key: "brakeSaturation", label: "Brake saturation", hint: "Full brake at this much pedal" },
  { key: "brakeDeadzone", label: "Brake dead zone", hint: "Ignore the first part of the pedal" },
  { key: "clutchSaturation", label: "Clutch saturation", hint: "Clutch fully pressed at this much pedal" },
  { key: "clutchDeadzone", label: "Clutch dead zone", hint: "Ignore the first part of the pedal" },
];

const BARS: { ch: "steer" | "throttle" | "brake" | "clutch"; label: string }[] = [
  { ch: "steer", label: "Steering" },
  { ch: "throttle", label: "Throttle" },
  { ch: "brake", label: "Braking" },
  { ch: "clutch", label: "Clutch" },
];

/** Settings → Controls: which device to edit, its shaping sliders, test mode and live bars. */
function ControlsSection(props: {
  s: GameSettings;
  change: (s: GameSettings) => void;
  device: ControlDevice;
  editDevice: (d: ControlDevice) => void;
  testInput: boolean;
  setTestInput: (on: boolean) => void;
}) {
  const { s, change, device } = props;
  const c = s.controls[device];
  const set = (patch: Partial<DeviceControls>) => change({ ...s, controls: { ...s.controls, [device]: { ...c, ...patch } } });
  return (
    <>
      <h3>Controls</h3>
      <Row label="Device" hint="Which device's settings you are editing">
        <Cycle
          id="gm-ctl-device"
          label="Controls device"
          value={device}
          options={CONTROL_DEVICES.map((d) => ({ id: d, label: DEVICE_LABEL[d] }))}
          onChange={props.editDevice}
        />
      </Row>
      {CONTROL_ROWS.map((r) => {
        const hint = r.key === "steerSpeedSensitivity" && !s.assists.steerHelp ? "No effect while Steering help is off" : r.hint;
        const [min, max] = CONTROL_RANGES[r.key];
        return (
          <Row key={r.key} label={r.label} hint={hint}>
            <Slider id={`gm-ctl-${r.key}`} label={r.label} value={c[r.key]} min={min} max={max} onChange={(v) => set({ [r.key]: v })} />
          </Row>
        );
      })}
      <Row label="Test input" hint={props.testInput ? "Hold your driving keys or pedals · Esc / B to stop" : "Turn on, then hold driving keys to watch the bars"}>
        <Toggle id="gm-ctl-test" label="Test input" value={props.testInput} onChange={props.setTestInput} />
      </Row>
      <div class={`wp-gm-bars${props.testInput ? " is-testing" : ""}`} aria-label="Processed input">
        {BARS.map((b) => (
          <Fragment key={b.ch}>
            <span>{b.label}</span>
            <div class={`wp-gm-bar${b.ch === "steer" ? " is-bipolar" : ""}`} data-bar={b.ch}>
              <i />
            </div>
            <b class="wp-mono" data-bar-value={b.ch}>
              0%
            </b>
          </Fragment>
        ))}
        <div class="wp-gm-bars-note wp-mono" id="gm-ctl-inuse">
          …
        </div>
      </div>
      <Row label="Reset device" hint={`Default values for ${DEVICE_LABEL[device]}`}>
        <button type="button" id="gm-ctl-reset" data-nav class="wp-gm-item is-small" onClick={() => set(defaultDeviceControls(device))}>
          Reset {DEVICE_LABEL[device]}
        </button>
      </Row>
    </>
  );
}
