/**
 * Map HUD for live hosts (the testbed): the always-on corner minimap plus the full city map
 * overlay with a legend. Plain DOM + canvas 2D (no framework) so it costs one blit and a few
 * dozen glyphs per frame.
 *
 * ```ts
 * import { mountMapHud } from "../../ui-kit/src/live";
 * const map = mountMapHud(document.getElementById("app")!, { onOpenChange: (open) => (inputPaused = open) });
 * map.setStatic(JSON.parse(wasm.map_static_json()));   // once per city
 * map.frame(wasm.map_dynamic(player), performance.now()); // every frame
 * // host keys: M / Tab → map.toggle()
 * ```
 */
import { decodeMapDynamic, EMPTY_FRAME, type MapFrame, type MapStatic, type MapUnit } from "./data";
import { CITY_NAMES, districtName, zoneAt } from "./districts";
import { drawPlayer, drawPoi, drawRacer, drawTool, drawUnit, readPalette, ROAD_COLOR, DECK_CASING } from "./draw";
import { drawMarker, type MapExtras, type MapLegendExtra } from "./extras";
import { FullMapRenderer, type FullMapStats } from "./FullMap";
import { MinimapRenderer, type MinimapStats } from "./Minimap";
import { fullToWorld } from "./projection";
import { speedIn, unitLabel, type SpeedUnits } from "../menu/settings";
import "../ui/theme.css";
import "./map.css";

/** Mount options. */
export interface MapHudOptions {
  /** Show signalised junctions on the minimap. Default false. */
  signals?: boolean;
  /** Called when the full map opens or closes (the host pauses driving input while open). */
  onOpenChange?: (open: boolean) => void;
  /** Minimap placement override (CSS, e.g. `{ left: "8px", top: "320px", right: "auto", bottom: "auto" }`).
   * Default: bottom-right corner. */
  minimapPlacement?: Partial<Record<"left" | "right" | "top" | "bottom", string>>;
}

/** What both views drew (e2e). */
export interface MapHudStats {
  open: boolean;
  city: string | null;
  minimap: MinimapStats;
  full: FullMapStats;
  /** District under the player (minimap caption). */
  district: string | null;
  /** A host picker is armed (`setPicker`). */
  picking: boolean;
}

/** A drag shorter than this (CSS px) is a tap: it picks when a picker is armed. */
const TAP_PX = 6;

const FAKE_UNIT = (over: Partial<MapUnit>): MapUnit => ({
  id: 1,
  x: 0,
  y: 0,
  yaw: 0,
  speed: 0,
  state: "patrol",
  heli: false,
  lights: false,
  onPlayer: false,
  wrecked: false,
  human: false,
  target: null,
  ...over,
});

/** The mounted map HUD. */
export class MapHud {
  readonly root: HTMLElement;
  private readonly mini: MinimapRenderer;
  private readonly full: FullMapRenderer;
  private readonly fullEl: HTMLElement;
  private readonly miniEl: HTMLElement;
  private readonly caption: HTMLElement;
  private readonly title: HTMLElement;
  private map: MapStatic | null = null;
  private frameData: MapFrame = EMPTY_FRAME;
  private openFlag = false;
  private units: SpeedUnits = "kmh";
  private follow = false;
  private district: string | null = null;
  private lastCaptionMs = -Infinity;
  private drag: { x: number; y: number; id: number; x0: number; y0: number } | null = null;
  private onPick: ((x: number, y: number) => void) | null = null;
  private readonly hostLegend: HTMLElement;
  /** Host panel slot over the full map (top-left; e.g. a challenge card). Empty = hidden. */
  readonly panel: HTMLElement;
  private readonly onKey = (e: KeyboardEvent) => this.key(e);

  constructor(
    parent: HTMLElement,
    private readonly opts: MapHudOptions = {},
  ) {
    const root = document.createElement("div");
    root.className = "wp-ui wp-map-root";
    root.dataset.testid = "map-hud";
    this.root = root;

    this.miniEl = document.createElement("div");
    this.miniEl.className = "wp-minimap";
    this.miniEl.id = "wp-minimap";
    const mc = document.createElement("canvas");
    mc.setAttribute("aria-label", "Minimap");
    this.miniEl.appendChild(mc);
    this.caption = document.createElement("div");
    this.caption.className = "wp-minimap-cap wp-mono";
    this.miniEl.appendChild(this.caption);
    Object.assign(this.miniEl.style, opts.minimapPlacement ?? {});
    root.appendChild(this.miniEl);

    this.fullEl = document.createElement("div");
    this.fullEl.className = "wp-map-full";
    this.fullEl.id = "wp-map-full";
    this.fullEl.hidden = true;
    const fc = document.createElement("canvas");
    fc.setAttribute("aria-label", "City map");
    this.fullEl.appendChild(fc);
    const head = document.createElement("header");
    head.className = "wp-map-head";
    this.title = document.createElement("b");
    const keys = document.createElement("span");
    keys.textContent = "M / Tab / Esc close · drag or arrows pan · wheel or + − zoom · 0 whole city · F follow";
    head.append(this.title, keys);
    this.fullEl.appendChild(head);
    root.appendChild(this.fullEl);
    parent.appendChild(root);

    this.mini = new MinimapRenderer(mc, { signals: opts.signals });
    this.full = new FullMapRenderer(fc);
    this.panel = document.createElement("aside");
    this.panel.className = "wp-panel wp-map-host";
    this.panel.hidden = true;
    this.fullEl.appendChild(this.panel);
    const legend = this.legend();
    this.hostLegend = document.createElement("div");
    this.hostLegend.className = "wp-map-legend-host";
    legend.insertBefore(this.hostLegend, legend.children[1] ?? null);
    this.fullEl.appendChild(legend);
    this.wirePointer(fc);
  }

  /** Static map once per city (`map_static_json()` text or parsed; `null` = no city). */
  setStatic(map: MapStatic | string | null): void {
    const m = typeof map === "string" ? (JSON.parse(map) as MapStatic | null) : map;
    this.map = m;
    this.mini.setMap(m);
    this.full.setMap(m);
    this.miniEl.hidden = !m;
    this.title.textContent = m ? `${(CITY_NAMES[m.city] ?? m.city).toUpperCase()} · CITY MAP` : "NO CITY";
    this.district = null;
    if (!m && this.openFlag) this.close();
  }

  /** Per-frame data (`map_dynamic()` array or a decoded frame). Draws both views. */
  frame(dyn: ArrayLike<number> | MapFrame | null, nowMs: number): void {
    this.frameData = !dyn ? EMPTY_FRAME : "units" in dyn ? dyn : decodeMapDynamic(dyn);
    if (!this.map) return;
    const f = this.frameData;
    this.mini.draw(f, nowMs);
    if (nowMs - this.lastCaptionMs > 400) {
      this.lastCaptionMs = nowMs;
      const z = f.player ? zoneAt(this.map, f.player.x, f.player.y) : -1;
      this.district = z >= 0 ? districtName(this.map.city, this.map.zones[z]) : null;
      const speed = f.player ? `${Math.round(speedIn(this.units, f.player.speed))} ${unitLabel(this.units)}` : "";
      this.caption.textContent = [this.district?.toUpperCase(), speed].filter(Boolean).join(" · ");
    }
    if (this.openFlag) {
      if (this.follow && f.player) this.full.centreOn(f.player.x, f.player.y);
      this.full.draw(f, nowMs);
    }
  }

  /** The full map is open. */
  get isOpen(): boolean {
    return this.openFlag;
  }

  /** Open the full map (fitted to the whole city). */
  open(): void {
    if (this.openFlag || !this.map) return;
    this.openFlag = true;
    this.fullEl.hidden = false;
    this.follow = false;
    this.full.fit();
    window.addEventListener("keydown", this.onKey, true);
    this.full.draw(this.frameData, performance.now());
    this.opts.onOpenChange?.(true);
  }

  /** Close the full map. */
  close(): void {
    if (!this.openFlag) return;
    this.openFlag = false;
    this.fullEl.hidden = true;
    window.removeEventListener("keydown", this.onKey, true);
    this.opts.onOpenChange?.(false);
  }

  /** Open / close the full map. */
  toggle(): void {
    if (this.openFlag) this.close();
    else this.open();
  }

  /** Host overlay on both maps (markers, route, flags); `null` clears it. Drawn from the next
   * `frame()` (the full map redraws at once while open). */
  setExtras(extras: MapExtras | null): void {
    this.mini.extras = extras;
    this.full.extras = extras;
    if (this.openFlag) this.full.draw(this.frameData, performance.now());
  }

  /** Arm a picker: a tap / click on the full map (not a drag) calls `onPick` with the world
   * point, and a crosshair marks the centre for `pickCentre()` (gamepad). `null` disarms. */
  setPicker(onPick: ((x: number, y: number) => void) | null): void {
    this.onPick = onPick;
    this.full.crosshair = !!onPick;
    this.fullEl.classList.toggle("wp-map-picking", !!onPick);
  }

  /** Pick the world point under the full map's centre (crosshair). No-op without a picker. */
  pickCentre(): void {
    if (!this.onPick || !this.openFlag) return;
    const v = this.full.view;
    const [x, y] = fullToWorld(v, v.w / 2, v.h / 2);
    this.onPick(x, y);
  }

  /** Pan the full map by CSS px (content follows, like a drag; gamepad stick). */
  panBy(dxCss: number, dyCss: number): void {
    if (!this.openFlag) return;
    this.follow = false;
    this.full.panBy(dxCss, dyCss);
  }

  /** World point under a CSS client position on the full map (null when closed). */
  worldAt(clientX: number, clientY: number): [number, number] | null {
    if (!this.openFlag) return null;
    const c = this.full.canvas;
    const r = c.getBoundingClientRect();
    const dpr = (typeof devicePixelRatio === "number" && devicePixelRatio) || 1;
    return fullToWorld(this.full.view, (clientX - r.left) * dpr, (clientY - r.top) * dpr);
  }

  /** CSS client position of a world point on the full map (null when closed; e2e). */
  clientAt(x: number, y: number): [number, number] | null {
    if (!this.openFlag) return null;
    const v = this.full.view;
    const r = this.full.canvas.getBoundingClientRect();
    const dpr = (typeof devicePixelRatio === "number" && devicePixelRatio) || 1;
    return [r.left + (v.w / 2 + (x - v.cx) * v.pxPerM) / dpr, r.top + (v.h / 2 - (y - v.cy) * v.pxPerM) / dpr];
  }

  /** Extra legend rows from the host (a titled group above the built-in rows); empty clears. */
  setLegendExtras(title: string, rows: MapLegendExtra[]): void {
    this.hostLegend.replaceChildren();
    if (rows.length === 0) return;
    const t = document.createElement("div");
    t.className = "wp-map-legend-row wp-map-legend-sub";
    t.textContent = title;
    this.hostLegend.appendChild(t);
    const dpr = (typeof devicePixelRatio === "number" && devicePixelRatio) || 1;
    for (const r of rows) {
      const li = document.createElement("div");
      li.className = "wp-map-legend-row";
      const cv = document.createElement("canvas");
      const s = 22 * dpr;
      cv.width = s;
      cv.height = s;
      const ctx = cv.getContext("2d");
      if (ctx) drawMarker(ctx, { colour: r.colour, label: r.label }, s / 2, s / 2, s * 0.36);
      const tx = document.createElement("span");
      tx.textContent = r.text;
      li.append(cv, tx);
      this.hostLegend.appendChild(li);
    }
  }

  /** Show or hide the corner minimap. */
  setMinimapVisible(on: boolean): void {
    this.miniEl.classList.toggle("wp-hidden", !on);
  }

  /** Speed units of the minimap caption (game menu setting). */
  setUnits(units: SpeedUnits): void {
    this.units = units;
    this.lastCaptionMs = -Infinity;
  }

  /** Draw traffic signals on the minimap. */
  setSignals(on: boolean): void {
    this.mini.options.signals = on;
  }

  /** Re-read the palette (colour-blind setting changed). */
  refreshPalette(): void {
    this.mini.refreshPalette();
    this.full.setMap(this.map);
  }

  /** What was drawn (e2e). */
  stats(): MapHudStats {
    return { open: this.openFlag, city: this.map?.city ?? null, minimap: this.mini.stats, full: this.full.stats, district: this.district, picking: !!this.onPick };
  }

  /** Remove from the page. */
  destroy(): void {
    this.close();
    this.root.remove();
  }

  private key(e: KeyboardEvent): void {
    if (!this.openFlag) return;
    const step = 90;
    let used = true;
    switch (e.code) {
      case "Escape":
        this.close();
        break;
      case "ArrowLeft":
      case "KeyA":
        this.follow = false;
        this.full.panBy(step, 0);
        break;
      case "ArrowRight":
      case "KeyD":
        this.follow = false;
        this.full.panBy(-step, 0);
        break;
      case "ArrowUp":
      case "KeyW":
        this.follow = false;
        this.full.panBy(0, step);
        break;
      case "ArrowDown":
      case "KeyS":
        this.follow = false;
        this.full.panBy(0, -step);
        break;
      case "Equal":
      case "NumpadAdd":
        this.full.zoomBy(1.4);
        break;
      case "Minus":
      case "NumpadSubtract":
        this.full.zoomBy(1 / 1.4);
        break;
      case "Digit0":
      case "Numpad0":
      case "Home":
        this.follow = false;
        this.full.fit();
        break;
      case "KeyF":
        this.follow = !this.follow;
        break;
      default:
        used = false;
    }
    if (used) {
      // The host's own shortcuts (camera, layers, bill) don't fire under the map.
      e.preventDefault();
      e.stopImmediatePropagation();
      this.full.draw(this.frameData, performance.now());
    }
  }

  private wirePointer(c: HTMLCanvasElement): void {
    c.addEventListener("pointerdown", (e) => {
      this.drag = { x: e.clientX, y: e.clientY, id: e.pointerId, x0: e.clientX, y0: e.clientY };
      this.follow = false;
      c.setPointerCapture?.(e.pointerId);
    });
    c.addEventListener("pointermove", (e) => {
      if (!this.drag || this.drag.id !== e.pointerId) return;
      this.full.panBy(e.clientX - this.drag.x, e.clientY - this.drag.y);
      this.drag = { ...this.drag, x: e.clientX, y: e.clientY };
    });
    c.addEventListener("pointerup", (e) => {
      const d = this.drag;
      this.drag = null;
      if (!d || d.id !== e.pointerId || !this.onPick) return;
      if (Math.hypot(e.clientX - d.x0, e.clientY - d.y0) > TAP_PX) return;
      const w = this.worldAt(e.clientX, e.clientY);
      if (w) this.onPick(w[0], w[1]);
    });
    c.addEventListener("pointercancel", () => (this.drag = null));
    c.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        const r = c.getBoundingClientRect();
        this.full.zoomBy(Math.exp(-e.deltaY * 0.0015), e.clientX - r.left, e.clientY - r.top);
      },
      { passive: false },
    );
  }

  /** Legend panel; swatches use the same drawing code as the map. */
  private legend(): HTMLElement {
    const el = document.createElement("aside");
    el.className = "wp-panel wp-map-legend";
    const pal = readPalette(this.root);
    const title = document.createElement("div");
    title.className = "wp-panel-title";
    title.textContent = "Legend";
    el.appendChild(title);
    const row = (label: string, paint: (ctx: CanvasRenderingContext2D, s: number) => void) => {
      const li = document.createElement("div");
      li.className = "wp-map-legend-row";
      const cv = document.createElement("canvas");
      const s = 22 * ((typeof devicePixelRatio === "number" && devicePixelRatio) || 1);
      cv.width = s;
      cv.height = s;
      const ctx = cv.getContext("2d");
      if (ctx) paint(ctx, s);
      const t = document.createElement("span");
      t.textContent = label;
      li.append(cv, t);
      el.appendChild(li);
    };
    const now = 0;
    row("You", (c, s) => drawPlayer(c, s / 2, s / 2, 0, s * 0.36, pal));
    row("Race opponent (position; leader ringed)", (c, s) =>
      drawRacer(c, { id: 0, x: 0, y: 0, yaw: 0, position: 2, finished: false, dnf: false, captured: false, dq: false, player: false }, s * 0.38, s * 0.58, 0, s * 0.26, pal),
    );
    row("Police, pursuing / responding (blinks)", (c, s) => {
      drawUnit(c, FAKE_UNIT({ id: 0, lights: true, state: "pursue" }), s * 0.3, s / 2, 0, s * 0.18, now, pal);
      drawUnit(c, FAKE_UNIT({ id: 1, lights: true, state: "pursue" }), s * 0.72, s / 2, 0, s * 0.18, 140, pal);
    });
    row("Police, patrol (steady)", (c, s) => drawUnit(c, FAKE_UNIT({}), s / 2, s / 2, 0, s * 0.22, now, pal));
    row("Helicopter + spotlight range", (c, s) => drawUnit(c, FAKE_UNIT({ heli: true, lights: true }), s / 2, s / 2, 0, s * 0.2, now, pal));
    row("Police sight (while wanted)", (c, s) => {
      c.fillStyle = pal.policeB;
      c.globalAlpha = 0.25;
      c.beginPath();
      c.arc(s / 2, s / 2, s * 0.42, 0, Math.PI * 2);
      c.fill();
    });
    row("Search zone (last known spot)", (c, s) => {
      c.strokeStyle = pal.warn;
      c.setLineDash([3, 3]);
      c.lineWidth = 1.5;
      c.beginPath();
      c.arc(s / 2, s / 2, s * 0.38, 0, Math.PI * 2);
      c.stroke();
    });
    const toolM = (s: number) => [1, 0, 0, -1, s / 2, s / 2] as [number, number, number, number, number, number];
    row("Roadblock", (c, s) => drawTool(c, { kind: "roadblock", x: 0, y: 0, dir: [1, 0], halfLenM: s * 0.4 }, toolM(s), 1.6, pal));
    row("Spike strip", (c, s) => drawTool(c, { kind: "spikes", x: 0, y: 0, dir: [1, 0], halfLenM: s * 0.4 }, toolM(s), 1.6, pal));
    row("Garage", (c, s) => drawPoi(c, "Garage", s / 2, s / 2, s * 0.32, pal));
    row("Safehouse", (c, s) => drawPoi(c, "Safehouse", s / 2, s / 2, s * 0.32, pal));
    row("Police station", (c, s) => drawPoi(c, "PoliceStation", s / 2, s / 2, s * 0.32, pal));
    row("Race start", (c, s) => drawPoi(c, "RaceStartGrid", s / 2, s / 2, s * 0.32, pal));
    const road = (cls: number, deck = false) => (c: CanvasRenderingContext2D, s: number) => {
      c.lineCap = "butt";
      if (deck) {
        c.strokeStyle = ROAD_COLOR[3];
        c.lineWidth = s * 0.16;
        c.beginPath();
        c.moveTo(s / 2, 2);
        c.lineTo(s / 2, s - 2);
        c.stroke();
        c.strokeStyle = DECK_CASING;
        c.lineWidth = s * 0.42;
        c.beginPath();
        c.moveTo(2, s / 2);
        c.lineTo(s - 2, s / 2);
        c.stroke();
      }
      c.strokeStyle = ROAD_COLOR[cls];
      c.lineWidth = [0.3, 0.22, 0.16, 0.12, 0.08][cls] * s;
      c.beginPath();
      c.moveTo(2, s / 2);
      c.lineTo(s - 2, s / 2);
      c.stroke();
    };
    row("Highway", road(0));
    row("Arterial", road(1));
    row("Collector / local", road(2));
    row("Overpass / bridge", road(0, true));
    return el;
  }
}

/** Mount the map HUD into `parent` (which must be positioned). */
export function mountMapHud(parent: HTMLElement, opts: MapHudOptions = {}): MapHud {
  return new MapHud(parent, opts);
}
