// RPM arc gauge + shift light for the player speed cluster (owner-approved feature F46).
//
// A shallow tachometer arc across the cluster (0 … rev limit, red band from the redline) with
// the current rpm filled in, and a row of five shift LEDs above it: they light green → amber
// from 80 % of the redline, all red at 97 %, and the whole row flashes at the redline (with
// automatic gears the box shifts for you: the row never flashes).

/** Shift light state: 0 off, 1–5 LEDs lit, 6 = shift now (flashing). */
export type ShiftLight = number;

/** LEDs start at this fraction of the redline. */
const LED_START = 0.8;
/** All LEDs lit (red) from this fraction of the redline. */
const LED_FULL = 0.97;

/** Shift light for `rpm` (auto gears: never the flashing "shift now"). */
export function shiftLight(rpm: number, redline: number, autoGears: boolean): ShiftLight {
  if (redline <= 0) return 0;
  const f = rpm / redline;
  if (f >= 1) return autoGears ? 5 : 6;
  if (f < LED_START) return 0;
  return Math.min(5, 1 + Math.floor(((f - LED_START) / (LED_FULL - LED_START)) * 4));
}

const LED_COLOURS = ["#2ee59d", "#2ee59d", "#ffd23f", "#ffd23f", "#ff4040"];

/** Draw the gauge on `c` (220 × 54 canvas). `nowMs` drives the flash. */
export function drawRpmGauge(c: HTMLCanvasElement, rpm: number, redline: number, limit: number, light: ShiftLight, nowMs: number): void {
  const ctx = c.getContext("2d");
  if (!ctx) return;
  const w = c.width;
  const h = c.height;
  ctx.clearRect(0, 0, w, h);
  // Shift LEDs.
  const flashOn = light === 6 && Math.floor(nowMs / 90) % 2 === 0;
  for (let i = 0; i < 5; i++) {
    const lit = light === 6 ? flashOn : i < light;
    ctx.beginPath();
    ctx.arc(w / 2 + (i - 2) * 18, 7, 5, 0, Math.PI * 2);
    ctx.fillStyle = lit ? (light === 6 ? "#ff3b30" : LED_COLOURS[i]) : "rgba(255,255,255,0.12)";
    ctx.fill();
  }
  // Arc: a wide, shallow segment of a big circle (centre below the canvas).
  const r = w * 1.05;
  const cx = w / 2;
  const cy = 18 + r + 10;
  const half = Math.asin((w / 2 - 6) / r);
  const a0 = -Math.PI / 2 - half;
  const a1 = -Math.PI / 2 + half;
  const at = (frac: number) => a0 + (a1 - a0) * Math.max(0, Math.min(1, frac));
  const max = Math.max(limit, redline, 1);
  ctx.lineCap = "butt";
  ctx.lineWidth = 9;
  ctx.strokeStyle = "rgba(255,255,255,0.12)";
  ctx.beginPath();
  ctx.arc(cx, cy, r, a0, a1);
  ctx.stroke();
  ctx.strokeStyle = "rgba(255,64,64,0.45)";
  ctx.beginPath();
  ctx.arc(cx, cy, r, at(redline / max), a1);
  ctx.stroke();
  const f = rpm / max;
  ctx.strokeStyle = rpm >= redline ? "#ff4040" : light >= 3 ? "#ffd23f" : "#40c0ff";
  ctx.beginPath();
  ctx.arc(cx, cy, r, a0, at(f));
  ctx.stroke();
  // Ticks every 1000 rpm.
  ctx.strokeStyle = "rgba(255,255,255,0.45)";
  ctx.lineWidth = 1;
  for (let k = 1000; k < max; k += 1000) {
    const a = at(k / max);
    ctx.beginPath();
    ctx.moveTo(cx + Math.cos(a) * (r - 7), cy + Math.sin(a) * (r - 7));
    ctx.lineTo(cx + Math.cos(a) * (r + 6), cy + Math.sin(a) * (r + 6));
    ctx.stroke();
  }
  ctx.fillStyle = "rgba(255,255,255,0.6)";
  ctx.font = "10px system-ui, sans-serif";
  ctx.textAlign = "left";
  ctx.fillText(`${(rpm / 1000).toFixed(1)}k rpm`, 2, 11);
}
