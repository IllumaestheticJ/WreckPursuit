// HUD layout (playtest row 67: the minimap covered the controls panel). Corners:
//
//   top-left      speedo + input panel (.left-col), then the race HUD (or the street race
//                 panel) stacked under it
//   top-centre    chase readout, the weather line under it (between the side columns, wrapping
//                 if it must), the police radio strip under those, pursuit banner
//   top-right     damage silhouette, components, regions, event feed (.right-col), capped so it
//                 ends above the minimap / telemetry
//   bottom-right  minimap (UI kit)
//   bottom-left   developer panel (#controls), the audio panel stacked above it
//   bottom-centre telemetry plots, right of the developer panel
//
// Most of it is CSS; this pass handles the stacks whose sizes change at run time (the race HUD
// appears, panels fold, the feed grows). It only writes styles, reading a handful of rects, and
// runs on resize and at most every 100 ms (`frame()`).

const MARGIN = 8;
const GAP = 6;
/** Widest the top-right column gets (the event feed: max-width 360 px + padding). */
const RIGHT_COL_MAX_W = 380;

const shown = (el: Element | null): el is HTMLElement => {
  if (!(el instanceof HTMLElement) || el.closest("[hidden], .hidden")) return false;
  const cs = getComputedStyle(el);
  return cs.display !== "none" && cs.visibility !== "hidden" && el.getClientRects().length > 0;
};

/** Lays out the testbed HUD columns; call `update()` after anything may have changed size. */
export class HudLayout {
  /** Layout passes run (automation). */
  passes = 0;
  private last = -Infinity;

  constructor() {
    window.addEventListener("resize", () => this.update());
  }

  /** Once per frame: a pass at most every 100 ms of wall time. */
  frame(nowMs: number): void {
    if (nowMs - this.last >= 100) this.update(nowMs);
  }

  /** A layout pass now. */
  update(nowMs = performance.now()): void {
    this.last = nowMs;
    this.passes++;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const q = (s: string) => document.querySelector(s);

    // Top-left stack.
    const left = q(".left-col");
    let leftBottom = shown(left) ? left.getBoundingClientRect().bottom : MARGIN;
    const race = q(".wp-race-hud");
    if (shown(race)) {
      race.style.top = `${Math.round(leftBottom + GAP)}px`;
      leftBottom = race.getBoundingClientRect().bottom;
    }
    // Street race panel (src/ui/street.ts): same slot, never shown with a picker race.
    const street = q("#street-hud");
    if (shown(street)) {
      street.style.top = `${Math.round(leftBottom + GAP)}px`;
      leftBottom = street.getBoundingClientRect().bottom;
    }

    // Bottom-right minimap.
    const mini = q(".wp-minimap");
    const mm = shown(mini) ? mini.getBoundingClientRect() : null;

    // Bottom-left stack: developer panel, audio panel above it; both shrink (and scroll) to fit
    // between the top-left stack and the bottom edge.
    const controls = q("#controls");
    const audio = q("#audio-panel");
    const audioH = shown(audio) ? audio.getBoundingClientRect().height : 0;
    let floor = vh - MARGIN;
    let controlsRight = 0;
    if (shown(controls)) {
      const room = floor - (leftBottom + GAP) - (audioH ? audioH + GAP : 0);
      controls.style.boxSizing = "border-box";
      controls.style.maxHeight = `${Math.max(60, Math.floor(Math.min(room, vh * 0.45)))}px`;
      const r = controls.getBoundingClientRect();
      floor = r.top - GAP;
      controlsRight = r.right;
    }
    if (shown(audio)) {
      Object.assign(audio.style, { boxSizing: "border-box", left: `${MARGIN}px`, right: "auto", top: "auto", bottom: `${Math.round(vh - floor)}px` });
      audio.style.maxHeight = `${Math.max(36, Math.floor(floor - (leftBottom + GAP)))}px`;
    }

    // Telemetry: right of the developer panel, left of the minimap; hidden if it cannot fit.
    const tel = q("canvas.telemetry");
    let telRect: DOMRect | null = null;
    if (tel instanceof HTMLElement && tel.style.display !== "none") {
      tel.style.left = `${Math.round(controlsRight + MARGIN)}px`;
      tel.style.visibility = "";
      telRect = tel.getBoundingClientRect();
      const limit = mm ? mm.left - GAP : vw - MARGIN;
      if (telRect.right > limit) {
        tel.style.visibility = "hidden";
        telRect = null;
      }
    }

    // Top-right column: ends above the minimap (and the telemetry if they share columns).
    const right = q(".right-col");
    if (shown(right)) {
      const top = right.getBoundingClientRect().top;
      // The column grows leftwards up to the event feed's width (max-width 360 px + padding).
      const colLeft = vw - MARGIN - RIGHT_COL_MAX_W;
      let bottom = vh - MARGIN;
      if (mm && mm.right > colLeft) bottom = Math.min(bottom, mm.top - GAP);
      if (telRect && telRect.right > colLeft) bottom = Math.min(bottom, telRect.top - GAP);
      right.style.maxHeight = `${Math.max(40, Math.floor(bottom - top))}px`;
    }

    // Weather line: under the chase readout, centred between the side columns.
    const wx = q("#weather-hud");
    if (shown(wx)) {
      const chase = q(".wp-chase-col");
      const top = shown(chase) ? chase.getBoundingClientRect().bottom + GAP : MARGIN + 4;
      const lo = shown(left) ? left.getBoundingClientRect().right + GAP : MARGIN;
      const hi = shown(right) ? right.getBoundingClientRect().left - GAP : vw - MARGIN;
      Object.assign(wx.style, { top: `${Math.round(top)}px`, transform: "none", whiteSpace: "normal", maxWidth: `${Math.max(120, hi - lo)}px`, boxSizing: "border-box" });
      const w = wx.getBoundingClientRect().width;
      wx.style.left = `${Math.round(Math.max(lo, Math.min(hi - w, vw / 2 - w / 2)))}px`;
    }

    // Police radio strip (row 79): under the chase readout and weather line, centred between
    // the side columns; ends above the pursuit banner and the minimap.
    const strip = q("#radio-strip");
    if (strip instanceof HTMLElement) {
      const chase = q(".wp-chase-col");
      let top = MARGIN + 4;
      if (shown(chase)) top = chase.getBoundingClientRect().bottom + GAP;
      if (shown(wx)) top = Math.max(top, wx.getBoundingClientRect().bottom + GAP);
      const lo = shown(left) ? left.getBoundingClientRect().right + GAP : MARGIN;
      const hi = shown(right) ? right.getBoundingClientRect().left - GAP : vw - MARGIN;
      const width = Math.max(160, Math.min(520, hi - lo));
      let bottom = vh - MARGIN;
      const banner = q("#pursuit-banner");
      if (shown(banner)) {
        const b = banner.getBoundingClientRect();
        if (b.top > top) bottom = Math.min(bottom, b.top - GAP);
      }
      if (mm && mm.left < vw / 2 + width / 2) bottom = Math.min(bottom, mm.top - GAP);
      Object.assign(strip.style, {
        top: `${Math.round(top)}px`,
        left: `${Math.round(Math.max(lo, Math.min(hi - width, vw / 2 - width / 2)))}px`,
        width: `${Math.round(width)}px`,
        maxHeight: `${Math.max(0, Math.floor(bottom - top))}px`,
      });
    }
  }
}
