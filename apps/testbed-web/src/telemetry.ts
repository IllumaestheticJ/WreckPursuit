// Rolling 10 s telemetry plots drawn on a plain canvas (no chart library).
import { T, TW } from "./wasm/layout";

interface Channel {
  label: string;
  colour: string;
  get: (t: Float32Array) => number;
}

interface Plot {
  title: string;
  min: number;
  max: number;
  channels: Channel[];
}

const wheel = (i: number, f: number) => (t: Float32Array) => t[T.wheelBase + i * T.wheelStride + f];
const WHEEL_COLOURS = ["#ff6b6b", "#ffd93d", "#6bcBff", "#9dff6b"];
const WHEEL_NAMES = ["FL", "FR", "RL", "RR"];

const PLOTS: Plot[] = [
  { title: "speed km/h", min: 0, max: 200, channels: [{ label: "speed", colour: "#7fdbff", get: (t) => t[T.speed] * 3.6 }] },
  {
    title: "slip angle °",
    min: -15,
    max: 15,
    channels: WHEEL_NAMES.map((n, i) => ({ label: n, colour: WHEEL_COLOURS[i], get: (t: Float32Array) => (wheel(i, TW.slipAngle)(t) * 180) / Math.PI })),
  },
  {
    title: "load kN",
    min: 0,
    max: 10,
    channels: WHEEL_NAMES.map((n, i) => ({ label: n, colour: WHEEL_COLOURS[i], get: (t: Float32Array) => wheel(i, TW.load)(t) / 1000 })),
  },
  { title: "yaw rate °/s", min: -90, max: 90, channels: [{ label: "yaw", colour: "#ff9f43", get: (t) => (t[T.yawRate] * 180) / Math.PI }] },
];

/** Seconds of history shown. */
const WINDOW_S = 10;
/** Ring capacity (samples): 10 s at up to ~200 fps. */
const CAP = 2048;
/** Redraw at most this often (ms): the strip is 340 px wide, 20 Hz reads as smooth. */
const DRAW_EVERY_MS = 50;
const CHANNELS = PLOTS.reduce((n, p) => n + p.channels.length, 0);
const TITLES = PLOTS.map((p) => `${p.title} [${p.min}, ${p.max}]`);

export class TelemetryPlots {
  readonly canvas = document.createElement("canvas");
  private readonly ctx: CanvasRenderingContext2D;
  // Ring buffer (no allocation per sample): times and CHANNELS values per slot.
  private readonly times = new Float64Array(CAP);
  private readonly values = new Float32Array(CAP * CHANNELS);
  private head = 0;
  private count = 0;
  private lastDraw = -Infinity;
  private dirty = true;

  constructor(parent: HTMLElement) {
    // Short strip: at 4 × 70 px it covered the lower half of the car in the near chase camera.
    this.canvas.width = 340;
    this.canvas.height = 4 * 48;
    this.canvas.className = "panel telemetry";
    parent.appendChild(this.canvas);
    this.ctx = this.canvas.getContext("2d")!;
  }

  clear(): void {
    this.count = 0;
    this.dirty = true;
  }

  /** Samples currently held (tests). */
  get size(): number {
    return this.count;
  }

  private slot(i: number): number {
    return (this.head - this.count + i + CAP) % CAP;
  }

  /** Add one sample at sim time `t` seconds. */
  push(t: number, tel: Float32Array): void {
    if (this.count && t < this.times[this.slot(this.count - 1)]) this.count = 0; // seek back
    const s = this.head;
    this.times[s] = t;
    let c = 0;
    for (const p of PLOTS) for (const ch of p.channels) this.values[s * CHANNELS + c++] = ch.get(tel);
    this.head = (this.head + 1) % CAP;
    this.count = Math.min(this.count + 1, CAP);
    while (this.count && this.times[this.slot(0)] < t - WINDOW_S) this.count--;
    this.dirty = true;
  }

  /** Redraw (throttled to DRAW_EVERY_MS; `force` draws now). */
  draw(force = false): void {
    const now = performance.now();
    if (!force && (!this.dirty || now - this.lastDraw < DRAW_EVERY_MS)) return;
    this.lastDraw = now;
    this.dirty = false;
    const { ctx, canvas } = this;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const h = canvas.height / PLOTS.length;
    const w = canvas.width;
    const tEnd = this.count ? this.times[this.slot(this.count - 1)] : 0;
    let col = 0;
    for (let pi = 0; pi < PLOTS.length; pi++) {
      const p = PLOTS[pi];
      const y0 = pi * h;
      ctx.fillStyle = "rgba(0,0,0,0.55)";
      ctx.fillRect(0, y0, w, h - 2);
      ctx.strokeStyle = "rgba(255,255,255,0.15)";
      const zeroY = y0 + (h - 4) * (1 - (0 - p.min) / (p.max - p.min)) + 2;
      ctx.beginPath();
      ctx.moveTo(0, zeroY);
      ctx.lineTo(w, zeroY);
      ctx.stroke();
      ctx.fillStyle = "#ccc";
      ctx.font = "10px monospace";
      ctx.fillText(TITLES[pi], 4, y0 + 11);
      for (let ci = 0; ci < p.channels.length; ci++) {
        const c = p.channels[ci];
        ctx.strokeStyle = c.colour;
        ctx.beginPath();
        for (let i = 0; i < this.count; i++) {
          const s = this.slot(i);
          const x = w * (1 - (tEnd - this.times[s]) / WINDOW_S);
          const frac = (this.values[s * CHANNELS + col] - p.min) / (p.max - p.min);
          const y = y0 + (h - 4) * (1 - Math.min(Math.max(frac, 0), 1)) + 2;
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        }
        ctx.stroke();
        if (p.channels.length > 1) {
          ctx.fillStyle = c.colour;
          ctx.fillText(c.label, w - 26 * (p.channels.length - ci), y0 + 11);
        }
        col++;
      }
    }
  }
}
