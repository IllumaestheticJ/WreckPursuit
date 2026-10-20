// HUD layout probe: bounding boxes of every visible HUD element and their overlaps (shared by
// e2e/menu.mjs). Export `hudBoxes(page)` → [{ name, x, y, w, h }] and `overlaps(boxes)`.
/** Selectors of HUD elements that must never overlap one another (name → selector). */
export const HUD_ELEMENTS = {
  speedo: ".hud-main",
  inputs: ".inputs",
  silhouette: "canvas.silhouette",
  components: ".comps",
  regions: ".regions",
  feed: ".feed",
  devPanel: "#controls",
  telemetry: "canvas.telemetry",
  audio: "#audio-panel",
  minimap: ".wp-minimap",
  chaseHud: ".wp-chase-col",
  banner: "#pursuit-banner",
  weather: "#weather-hud",
  raceHud: ".wp-race-hud",
};

/** Visible boxes (CSS px, viewport space). */
export async function hudBoxes(page) {
  return page.evaluate((els) => {
    const out = [];
    for (const [name, sel] of Object.entries(els)) {
      for (const el of document.querySelectorAll(sel)) {
        const cs = getComputedStyle(el);
        if (cs.display === "none" || cs.visibility === "hidden" || el.closest("[hidden], .hidden")) continue;
        // Union of the element and its visible descendants (absolutely positioned children
        // can sit outside the parent's own box).
        let r = el.getBoundingClientRect();
        let x0 = r.left, y0 = r.top, x1 = r.right, y1 = r.bottom;
        for (const c of el.querySelectorAll("*")) {
          // Skip what a scrolling / clipping ancestor (inside the element) hides.
          let clipped = false;
          for (let p = c.parentElement; p && p !== el.parentElement; p = p.parentElement) {
            if (getComputedStyle(p).overflow !== "visible") clipped = true;
          }
          if (clipped) continue;
          const cr = c.getBoundingClientRect();
          if (cr.width === 0 || cr.height === 0 || getComputedStyle(c).display === "none") continue;
          x0 = Math.min(x0, cr.left); y0 = Math.min(y0, cr.top); x1 = Math.max(x1, cr.right); y1 = Math.max(y1, cr.bottom);
        }
        if (x1 - x0 < 1 || y1 - y0 < 1) continue;
        out.push({ name, x: Math.round(x0), y: Math.round(y0), w: Math.round(x1 - x0), h: Math.round(y1 - y0) });
      }
    }
    return out;
  }, HUD_ELEMENTS);
}

/** Pairs of boxes that intersect by more than 1 px either way, or that leave the viewport. */
export function overlaps(boxes, vw, vh) {
  const bad = [];
  for (let i = 0; i < boxes.length; i++) {
    const a = boxes[i];
    if (a.x < -1 || a.y < -1 || a.x + a.w > vw + 1 || a.y + a.h > vh + 1) bad.push(`${a.name} off-screen (${a.x},${a.y} ${a.w}×${a.h})`);
    for (let j = i + 1; j < boxes.length; j++) {
      const b = boxes[j];
      const ox = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
      const oy = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
      if (ox > 1 && oy > 1) bad.push(`${a.name} × ${b.name} (${ox}×${oy} px)`);
    }
  }
  return bad;
}
