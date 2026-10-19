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

/** Samples kept: 10 s at 60 Hz. */
const WINDOW_S = 10;

export class TelemetryPlots {
  readonly canvas = document.createElement("canvas");
  private readonly ctx: CanvasRenderingContext2D;
  private samples: { t: number; v: number[] }[] = [];

  constructor(parent: HTMLElement) {
    // Short strip: at 4 × 70 px it covered the lower half of the car in the near chase camera.
    this.canvas.width = 340;
    this.canvas.height = 4 * 48;
    this.canvas.className = "panel telemetry";
    parent.appendChild(this.canvas);
    this.ctx = this.canvas.getContext("2d")!;
  }

  clear(): void {
    this.samples = [];
  }

  /** Add one sample at sim time `t` seconds. */
  push(t: number, tel: Float32Array): void {
    if (this.samples.length && t < this.samples[this.samples.length - 1].t) this.samples = []; // seek back
    this.samples.push({ t, v: PLOTS.flatMap((p) => p.channels.map((c) => c.get(tel))) });
    while (this.samples.length && this.samples[0].t < t - WINDOW_S) this.samples.shift();
  }

  draw(): void {
    const { ctx, canvas } = this;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const h = canvas.height / PLOTS.length;
    const w = canvas.width;
    const tEnd = this.samples.length ? this.samples[this.samples.length - 1].t : 0;
    let col = 0;
    PLOTS.forEach((p, pi) => {
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
      ctx.fillText(`${p.title} [${p.min}, ${p.max}]`, 4, y0 + 11);
      p.channels.forEach((c, ci) => {
        ctx.strokeStyle = c.colour;
        ctx.beginPath();
        this.samples.forEach((s, i) => {
          const x = w * (1 - (tEnd - s.t) / WINDOW_S);
          const frac = (s.v[col] - p.min) / (p.max - p.min);
          const y = y0 + (h - 4) * (1 - Math.min(Math.max(frac, 0), 1)) + 2;
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        });
        ctx.stroke();
        if (p.channels.length > 1) {
          ctx.fillStyle = c.colour;
          ctx.fillText(c.label, w - 26 * (p.channels.length - ci), y0 + 11);
        }
        col++;
      });
    });
  }
}
