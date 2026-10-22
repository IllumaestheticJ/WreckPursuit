/**
 * Host overlay for the minimap and the full map (`MapHud.setExtras`): coloured markers (e.g.
 * street racers in their level colour, number = level), flags (a race's A / B, the finish
 * chequered) and a route polyline. The host owns the data and the colours; this file only draws
 * them, in screen pixels, on top of the kit's own layers.
 */

/** Route line colour (gold: not police red / blue, not the scanner green, not a level colour). */
export const ROUTE_COLOUR = "#ffd23f";

/** A host marker (world position, CSS colour, optional short label drawn inside). */
export interface MapMarker {
  /** Host id (e.g. the sim entity); not drawn. */
  id?: number;
  /** World x, m. */
  x: number;
  /** World y, m. */
  y: number;
  /** Fill colour (CSS). */
  colour: string;
  /** Short label inside the dot (1–2 characters read best). */
  label?: string;
  /** Highlighted (racing / challenged): a white ring around the dot. */
  hot?: boolean;
}

/** A flag on the map (a race's start / finish). */
export interface MapFlag {
  /** World x, m. */
  x: number;
  /** World y, m. */
  y: number;
  /** Letter on the pennant ("A", "B"). */
  label: string;
  /** Chequered finish pennant instead of a plain one. */
  finish?: boolean;
}

/** One host legend row (`MapHud.setLegendExtras`): a marker swatch and its text. */
export interface MapLegendExtra {
  /** Swatch colour (CSS). */
  colour: string;
  /** Label inside the swatch. */
  label?: string;
  /** Row text. */
  text: string;
}

/** The host overlay drawn on both maps. */
export interface MapExtras {
  /** Markers (drawn over the kit's units and racers). */
  markers: MapMarker[];
  /** Route polyline, world [x, y] points; null / fewer than 2 points = none. */
  route?: [number, number][] | null;
  /** Flags (drawn last). */
  flags: MapFlag[];
}

type Ctx = CanvasRenderingContext2D;

/** Black or white text, whichever reads better on `colour` (#rgb / #rrggbb; else white). */
function inkOn(colour: string): string {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(colour.trim());
  if (!m) return "#fff";
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  return 0.299 * r + 0.587 * g + 0.114 * b > 150 ? "#000" : "#fff";
}

/** A route polyline in screen px (a dark casing under a gold line). */
export function drawRoute(ctx: Ctx, pts: [number, number][], width: number): void {
  if (pts.length < 2) return;
  ctx.save();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(pts[0][0], pts[0][1]);
  for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i][0], pts[i][1]);
  ctx.strokeStyle = "rgba(0,0,0,0.75)";
  ctx.lineWidth = width + Math.max(2, width * 0.8);
  ctx.stroke();
  ctx.strokeStyle = ROUTE_COLOUR;
  ctx.lineWidth = width;
  ctx.stroke();
  ctx.restore();
}

/** A marker dot of radius `s` centred on (x, y) in screen px. */
export function drawMarker(ctx: Ctx, mk: Pick<MapMarker, "colour" | "label" | "hot">, x: number, y: number, s: number): void {
  ctx.save();
  if (mk.hot) {
    ctx.beginPath();
    ctx.arc(x, y, s * 1.5, 0, Math.PI * 2);
    ctx.strokeStyle = "#fff";
    ctx.lineWidth = Math.max(1.5, s * 0.25);
    ctx.stroke();
  }
  ctx.beginPath();
  ctx.arc(x, y, s, 0, Math.PI * 2);
  ctx.fillStyle = mk.colour;
  ctx.fill();
  ctx.strokeStyle = "#000";
  ctx.lineWidth = Math.max(1.2, s * 0.18);
  ctx.stroke();
  if (mk.label && s >= 4) {
    ctx.fillStyle = inkOn(mk.colour);
    ctx.font = `bold ${Math.max(7, Math.round(s * 1.2))}px ui-monospace, Menlo, Consolas, monospace`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(mk.label, x, y + s * 0.06);
  }
  ctx.restore();
}

/** A flag whose pole foot is at (x, y), `s` px tall (finish = chequered pennant). */
export function drawFlag(ctx: Ctx, fl: MapFlag, x: number, y: number, s: number): void {
  const pw = s * 0.7;
  const ph = s * 0.45;
  const top = y - s;
  ctx.save();
  // pole
  ctx.strokeStyle = "#000";
  ctx.lineWidth = Math.max(2.5, s * 0.14);
  ctx.beginPath();
  ctx.moveTo(x, y);
  ctx.lineTo(x, top);
  ctx.stroke();
  ctx.strokeStyle = "#fff";
  ctx.lineWidth = Math.max(1.2, s * 0.07);
  ctx.stroke();
  // pennant
  if (fl.finish) {
    const n = 4;
    const cw = pw / n;
    const ch = ph / 2;
    for (let i = 0; i < n; i++) {
      for (let j = 0; j < 2; j++) {
        ctx.fillStyle = (i + j) % 2 === 0 ? "#fff" : "#000";
        ctx.fillRect(x + i * cw, top + j * ch, cw, ch);
      }
    }
    ctx.strokeStyle = "#000";
    ctx.lineWidth = 1;
    ctx.strokeRect(x, top, pw, ph);
  } else {
    ctx.fillStyle = ROUTE_COLOUR;
    ctx.strokeStyle = "#000";
    ctx.lineWidth = Math.max(1, s * 0.06);
    ctx.fillRect(x, top, pw, ph);
    ctx.strokeRect(x, top, pw, ph);
  }
  // letter beside the pennant (always readable over the chequer)
  ctx.font = `bold ${Math.max(9, Math.round(s * 0.5))}px ui-monospace, Menlo, Consolas, monospace`;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.lineWidth = 3;
  ctx.strokeStyle = "#000";
  ctx.strokeText(fl.label, x + pw + 3, top + ph / 2);
  ctx.fillStyle = "#fff";
  ctx.fillText(fl.label, x + pw + 3, top + ph / 2);
  ctx.restore();
}
