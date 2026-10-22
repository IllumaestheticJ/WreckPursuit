// The vinyl library and the car's livery canvas: base paint, bare carbon panels and one vinyl
// design, drawn into the livery atlas of the car's panel mesh (layout: data/panels/README.md
// "Livery atlas"). Designs draw in car metres through `Painter`, so one design fits every
// model. All designs are our own: shapes, made-up words, no real brands or logos.
import * as THREE from "three";
import { liveryBandPx, liveryPx, type LiveryAtlas, type LiverySide } from "../panels";
import type { CarFrame } from "./frame";

/** Canvas size of the livery texture (u 0..1 × v 0..1; paint uses the left half). */
const W = 2048;
const H = 1024;

/** Draws in car metres on the atlas bands of one canvas. */
export class Painter {
  constructor(
    readonly g: CanvasRenderingContext2D,
    readonly atlas: LiveryAtlas,
    /** Body side from the sill (`z0`) to the belt line (`z1`), m. */
    readonly z0: number,
    readonly z1: number,
  ) {}

  /** Nose and tail y, m. */
  get y0(): number {
    return this.atlas.y[0];
  }
  get y1(): number {
    return this.atlas.y[1];
  }

  /** Draw on both sides in (y forward, z up) metres; `side` is −1 left, +1 right. */
  sides(draw: (g: CanvasRenderingContext2D, side: -1 | 1) => void): void {
    const { y: [y0, y1], z: [za, zb] } = this.atlas;
    const dy = y1 - y0;
    const dz = zb - za;
    for (const side of [-1, 1] as const) {
      const b = liveryBandPx(side < 0 ? "left" : "right", W, H);
      const a = (side < 0 ? -1 : 1) * (b.w / dy);
      const e = side < 0 ? b.x + b.w + (b.w * y0) / dy : b.x - (b.w * y0) / dy;
      this.clipped(b, () => {
        this.g.setTransform(a, 0, 0, -b.h / dz, e, b.y + b.h + (b.h * za) / dz);
        draw(this.g, side);
      });
    }
  }

  /** Draw on the top band in (y forward, x right) metres. */
  top(draw: (g: CanvasRenderingContext2D) => void): void {
    const { y: [y0, y1], x_half: xh } = this.atlas;
    const b = liveryBandPx("top", W, H);
    const a = b.w / (y1 - y0);
    this.clipped(b, () => {
      this.g.setTransform(a, 0, 0, b.h / (2 * xh), b.x - a * y0, b.y + b.h / 2);
      draw(this.g);
    });
  }

  /** Draw on the front (`end` 1) or rear (−1) face in (x right, z up) metres. */
  end(end: 1 | -1, draw: (g: CanvasRenderingContext2D) => void): void {
    const { z: [za, zb], x_half: xh } = this.atlas;
    const b = liveryBandPx(end > 0 ? "front" : "rear", W, H);
    const dz = zb - za;
    const a = ((end > 0 ? -1 : 1) * b.w) / (2 * xh);
    this.clipped(b, () => {
      this.g.setTransform(a, 0, 0, -b.h / dz, b.x + b.w / 2, b.y + b.h + (b.h * za) / dz);
      draw(this.g);
    });
  }

  /** Text on a side, centred at (y, z), `size` metres tall, reading correctly from outside. */
  sideText(side: -1 | 1, text: string, y: number, z: number, size: number, fill: string, stroke?: string): void {
    const p = liveryPx(this.atlas, side < 0 ? "left" : "right", y, z, W, H);
    // The side band has fewer pixels per metre along the car than up it: squeeze the glyphs
    // so they come out in proportion on the car.
    const b = liveryBandPx("left", W, H);
    const up = b.h / (this.atlas.z[1] - this.atlas.z[0]);
    const along = b.w / (this.atlas.y[1] - this.atlas.y[0]);
    const g = this.g;
    g.save();
    g.setTransform(along / up, 0, 0, 1, p.x, p.y);
    this.text(0, 0, text, size * up, fill, stroke);
    g.restore();
  }

  /** Text on the top band centred at (y, x), upright for someone behind the car looking forward. */
  topText(text: string, y: number, x: number, size: number, fill: string): void {
    const p = liveryPx(this.atlas, "top", y, x, W, H);
    const b = liveryBandPx("top", W, H);
    const across = b.h / (2 * this.atlas.x_half);
    const along = b.w / (this.atlas.y[1] - this.atlas.y[0]);
    const g = this.g;
    g.save();
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.translate(p.x, p.y);
    // Plan view has the nose to the right and the car's left at the top: a quarter turn puts
    // the text's up toward the nose and its reading direction from left to right of the car;
    // then squeeze the glyphs to the band's pixels per metre in each direction.
    g.rotate(Math.PI / 2);
    g.scale(across / along, 1);
    this.text(0, 0, text, size * along, fill);
    g.restore();
  }

  /** Centred text at canvas point (x, y) in the current transform, `px` pixels tall. */
  private text(x: number, y: number, text: string, px: number, fill: string, stroke?: string): void {
    const g = this.g;
    g.save();
    g.font = `900 ${Math.round(px)}px "Arial Black", Impact, sans-serif`;
    g.textAlign = "center";
    g.textBaseline = "middle";
    if (stroke) {
      g.lineWidth = px * 0.12;
      g.strokeStyle = stroke;
      g.strokeText(text, x, y);
    }
    g.fillStyle = fill;
    g.fillText(text, x, y);
    g.restore();
  }

  /** Fill a car-space rectangle of one band with a pixel-space pattern (carbon weave). */
  fillPx(side: LiverySide, a0: number, b0: number, a1: number, b1: number, fill: string | CanvasPattern): void {
    const p = liveryPx(this.atlas, side, a0, b0, W, H);
    const q = liveryPx(this.atlas, side, a1, b1, W, H);
    this.g.setTransform(1, 0, 0, 1, 0, 0);
    this.g.fillStyle = fill;
    this.g.fillRect(Math.min(p.x, q.x), Math.min(p.y, q.y), Math.abs(q.x - p.x), Math.abs(q.y - p.y));
  }

  private clipped(b: { x: number; y: number; w: number; h: number }, fn: () => void): void {
    const g = this.g;
    g.save();
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.beginPath();
    g.rect(b.x, b.y, b.w, b.h);
    g.clip();
    fn();
    g.restore();
  }
}

/** Deterministic random numbers for a design (same car, same splats). */
function rng(seed: number): () => number {
  let s = seed >>> 0 || 1;
  return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
}

/** Colours a design picks from the paint: a contrasting ink and an accent. */
export interface Inks {
  /** Light on dark paint, dark on light paint. */
  ink: string;
  /** The other one. */
  base: string;
  /** Hot accent. */
  accent: string;
}

/** A vinyl design: draws on the painter. */
type Design = (p: Painter, k: Inks) => void;

/**
 * Flame tongues licking back from `yStart`: a solid base at the front, `tongues` points of
 * varying length trailing toward −y, spread over `height` around `zMid` (a, b = y, z or y, x).
 */
const flamePath = (g: CanvasRenderingContext2D, yStart: number, zMid: number, len: number, height: number, tongues: number) => {
  const zb = zMid - height / 2;
  const step = height / tongues;
  g.beginPath();
  g.moveTo(yStart, zb);
  for (let i = 0; i < tongues; i++) {
    const reach = len * (0.45 + 0.55 * Math.abs(Math.sin(i * 1.7 + 0.6)));
    const zTip = zb + step * (i + 0.5);
    const zEnd = zb + step * (i + 1);
    const valley = yStart - len * 0.22;
    // Out along the lower edge of the tongue to its tip, back along the upper edge with a curl.
    g.quadraticCurveTo(yStart - reach * 0.55, zb + step * i + step * 0.05, yStart - reach, zTip - step * 0.15);
    g.quadraticCurveTo(yStart - reach * 0.7, zTip + step * 0.35, valley, zEnd);
  }
  g.lineTo(yStart + 0.05, zb + height);
  g.lineTo(yStart + 0.05, zb);
  g.closePath();
};

const fireGradient = (g: CanvasRenderingContext2D, y0: number, y1: number) => {
  const grad = g.createLinearGradient(y0, 0, y1, 0);
  grad.addColorStop(0, "#ffe14a");
  grad.addColorStop(0.45, "#ff8a1a");
  grad.addColorStop(1, "#d31f12");
  return grad;
};

const DESIGNS: Record<string, Design> = {
  stripes: (p, k) => {
    p.top((g) => {
      g.fillStyle = k.ink;
      for (const x of [-0.17, 0.05]) g.fillRect(p.y0, x, p.y1 - p.y0, 0.12);
    });
    for (const e of [1, -1] as const)
      p.end(e, (g) => {
        g.fillStyle = k.ink;
        for (const x of [-0.17, 0.05]) g.fillRect(x, 0, 0.12, 3);
      });
  },
  flames: (p) => {
    const front = p.y1 - 0.15;
    p.sides((g) => {
      flamePath(g, front, (p.z0 + p.z1) / 2, 2.4, (p.z1 - p.z0) * 0.9, 5);
      g.fillStyle = fireGradient(g, front, front - 2.4);
      g.fill();
      g.lineWidth = 0.025;
      g.strokeStyle = "#5a0904";
      g.stroke();
    });
    p.top((g) => {
      g.save();
      // Hood flames licking back from the nose, in plan view (y forward, x across).
      flamePath(g, front, 0, 1.1, 1.3, 6);
      g.fillStyle = fireGradient(g, front, front - 1.1);
      g.fill();
      g.restore();
    });
  },
  tribal: (p, k) => {
    p.sides((g) => {
      g.fillStyle = k.ink;
      const mid = (p.z0 + p.z1) / 2;
      for (let i = 0; i < 4; i++) {
        const y = p.y1 - 0.6 - i * 0.85;
        g.beginPath();
        g.moveTo(y, mid);
        g.bezierCurveTo(y - 0.3, mid + 0.25, y - 0.6, mid + 0.2, y - 0.9, mid + 0.3);
        g.bezierCurveTo(y - 0.55, mid + 0.05, y - 0.35, mid - 0.05, y - 0.7, mid - 0.25);
        g.bezierCurveTo(y - 0.35, mid - 0.12, y - 0.15, mid - 0.1, y, mid);
        g.fill();
      }
    });
  },
  sponsor_generic: (p, k) => {
    p.sides((_g, side) => {
      p.sideText(side, "WRECK FUEL", (p.y0 + p.y1) / 2 + 0.2, (p.z0 + p.z1) / 2 + 0.05, 0.13, k.ink);
      p.sideText(side, "STREETSIDE", (p.y0 + p.y1) / 2 - 0.55, p.z0 + 0.14, 0.07, k.accent);
      p.sideText(side, "TORQUE SHOP", (p.y0 + p.y1) / 2 + 0.5, p.z0 + 0.14, 0.07, k.accent);
    });
    p.topText("WRECK FUEL", p.y1 - 0.65, 0, 0.16, k.ink);
  },
  side_slash: (p, k) => {
    p.sides((g) => {
      const y = (p.y0 + p.y1) / 2;
      g.fillStyle = k.ink;
      g.beginPath();
      g.moveTo(y + 0.9, p.z1 + 0.05);
      g.lineTo(y + 1.3, p.z1 + 0.05);
      g.lineTo(y - 0.6, p.z0 - 0.05);
      g.lineTo(y - 1.0, p.z0 - 0.05);
      g.fill();
      g.fillStyle = k.accent;
      g.beginPath();
      g.moveTo(y + 1.4, p.z1 + 0.05);
      g.lineTo(y + 1.5, p.z1 + 0.05);
      g.lineTo(y - 0.4, p.z0 - 0.05);
      g.lineTo(y - 0.5, p.z0 - 0.05);
      g.fill();
    });
  },
  pinstripe: (p, k) => {
    p.sides((g) => {
      g.strokeStyle = k.accent;
      g.lineWidth = 0.012;
      for (const dz of [0, 0.035]) {
        g.beginPath();
        g.moveTo(p.y1 - 0.3, p.z1 - 0.08 - dz);
        g.lineTo(p.y0 + 0.6, p.z1 - 0.08 - dz);
        g.quadraticCurveTo(p.y0 + 0.25, p.z1 - 0.08 - dz, p.y0 + 0.35, p.z1 - 0.3 - dz);
        g.stroke();
      }
    });
    p.top((g) => {
      g.strokeStyle = k.accent;
      g.lineWidth = 0.012;
      const y = p.y1 - 0.5;
      for (const s of [-1, 1]) {
        g.beginPath();
        g.moveTo(y, 0);
        g.bezierCurveTo(y - 0.2, s * 0.25, y - 0.45, s * 0.05, y - 0.6, s * 0.3);
        g.stroke();
      }
    });
  },
  race_number: (p, k) => {
    const n = "27";
    p.sides((g, side) => {
      const y = (p.y0 + p.y1) / 2 + 0.25;
      const z = (p.z0 + p.z1) / 2 + 0.02;
      g.fillStyle = k.base;
      g.strokeStyle = k.ink;
      g.lineWidth = 0.03;
      g.beginPath();
      g.arc(y, z, 0.26, 0, Math.PI * 2);
      g.fill();
      g.stroke();
      p.sideText(side, n, y, z, 0.3, k.ink);
    });
    p.top((g) => {
      const y = p.y1 - 0.75;
      g.fillStyle = k.base;
      g.beginPath();
      g.arc(y, 0, 0.3, 0, Math.PI * 2);
      g.fill();
    });
    p.topText(n, p.y1 - 0.75, 0, 0.32, k.ink);
  },
  checker: (p, k) => {
    p.sides((g) => {
      const s0 = 0.12;
      let y = p.y0;
      let col = 0;
      while (y < p.y0 + 2.2) {
        const s = s0 * (1 - (y - p.y0) / 2.6);
        for (let z = p.z0, row = 0; z < p.z1; z += s0, row++) {
          if ((row + col) % 2) continue;
          g.fillStyle = k.ink;
          g.fillRect(y, z + (s0 - s) / 2, s, s);
        }
        y += s0;
        col++;
      }
    });
  },
  lightning: (p, k) => {
    p.sides((g) => {
      const mid = (p.z0 + p.z1) / 2;
      const h = (p.z1 - p.z0) * 0.35;
      const pts: [number, number][] = [];
      const n = 7;
      for (let i = 0; i <= n; i++) pts.push([p.y1 - 0.3 - (i * (p.y1 - p.y0 - 0.6)) / n, mid + (i % 2 ? h : -h) * (1 - i / (n + 2))]);
      g.beginPath();
      g.moveTo(pts[0][0], pts[0][1] + 0.06);
      for (const [y, z] of pts) g.lineTo(y, z + 0.06);
      for (const [y, z] of pts.slice().reverse()) g.lineTo(y + 0.08, z - 0.06);
      g.closePath();
      g.fillStyle = "#ffe14a";
      g.fill();
      g.strokeStyle = k.ink;
      g.lineWidth = 0.02;
      g.stroke();
    });
  },
  splatter: (p) => {
    const colours = ["#ff2d6f", "#2ad1ff", "#ffe14a", "#7cff4a"];
    const blot = (g: CanvasRenderingContext2D, r: () => number, y: number, z: number, s: number, col: string) => {
      g.fillStyle = col;
      g.beginPath();
      g.arc(y, z, s, 0, Math.PI * 2);
      g.fill();
      for (let i = 0; i < 7; i++) {
        const a = r() * Math.PI * 2;
        const d = s * (1.2 + r() * 1.6);
        g.beginPath();
        g.arc(y + Math.cos(a) * d, z + Math.sin(a) * d, s * (0.12 + r() * 0.25), 0, Math.PI * 2);
        g.fill();
      }
    };
    p.sides((g) => {
      const r = rng(11);
      for (let i = 0; i < 14; i++) blot(g, r, p.y0 + r() * (p.y1 - p.y0), p.z0 + r() * (p.z1 - p.z0), 0.05 + r() * 0.1, colours[i % colours.length]);
    });
    p.top((g) => {
      const r = rng(23);
      for (let i = 0; i < 12; i++) blot(g, r, p.y0 + r() * (p.y1 - p.y0), (r() - 0.5) * 1.6, 0.05 + r() * 0.1, colours[i % colours.length]);
    });
  },
  hazard: (p) => {
    const stripes = (g: CanvasRenderingContext2D, a0: number, a1: number, b0: number, b1: number) => {
      g.fillStyle = "#f2c418";
      g.fillRect(a0, b0, a1 - a0, b1 - b0);
      g.fillStyle = "#141414";
      for (let a = a0 - (b1 - b0); a < a1; a += 0.24) {
        g.beginPath();
        g.moveTo(a, b0);
        g.lineTo(a + 0.12, b0);
        g.lineTo(a + 0.12 + (b1 - b0), b1);
        g.lineTo(a + (b1 - b0), b1);
        g.fill();
      }
    };
    p.sides((g) => stripes(g, p.y0, p.y1, p.z0, p.z0 + 0.16));
    for (const e of [1, -1] as const) p.end(e, (g) => stripes(g, -2, 2, p.z0, p.z0 + 0.22));
  },
  shark_mouth: (p) => {
    p.end(1, (g) => {
      const z = p.z0 + 0.22;
      g.fillStyle = "#b5121b";
      g.beginPath();
      g.ellipse(0, z, 0.7, 0.16, 0, 0, Math.PI * 2);
      g.fill();
      g.fillStyle = "#f4f4f0";
      for (let i = -6; i < 6; i++) {
        for (const s of [1, -1]) {
          g.beginPath();
          g.moveTo(i * 0.11, z + s * 0.15);
          g.lineTo(i * 0.11 + 0.11, z + s * 0.15);
          g.lineTo(i * 0.11 + 0.055, z + s * 0.02);
          g.fill();
        }
      }
    });
    p.sides((g) => {
      const y = p.y1 - 0.45;
      const z = (p.z0 + p.z1) / 2 + 0.08;
      g.fillStyle = "#f4f4f0";
      g.beginPath();
      g.ellipse(y, z, 0.11, 0.08, 0, 0, Math.PI * 2);
      g.fill();
      g.fillStyle = "#141414";
      g.beginPath();
      g.arc(y + 0.02, z, 0.05, 0, Math.PI * 2);
      g.fill();
      g.fillStyle = "#b5121b";
      g.beginPath();
      g.moveTo(y + 0.2, p.z0 + 0.12);
      g.quadraticCurveTo(y - 0.2, p.z0 + 0.05, y - 0.5, p.z0 + 0.22);
      g.quadraticCurveTo(y - 0.2, p.z0 + 0.14, y + 0.2, p.z0 + 0.2);
      g.fill();
    });
  },
  retro_bands: (p) => {
    const cols = ["#ffb000", "#ff6a00", "#d8261c"];
    p.sides((g) => {
      const z = (p.z0 + p.z1) / 2 - 0.05;
      cols.forEach((c, i) => {
        g.fillStyle = c;
        const zi = z + i * 0.07;
        g.beginPath();
        g.moveTo(p.y1, zi);
        g.lineTo(p.y0 + 1.1, zi);
        g.quadraticCurveTo(p.y0 + 0.8, zi, p.y0 + 0.6, zi + 0.35);
        g.lineTo(p.y0 + 0.66, zi + 0.35);
        g.quadraticCurveTo(p.y0 + 0.86, zi + 0.05, p.y0 + 1.1, zi + 0.05);
        g.lineTo(p.y1, zi + 0.05);
        g.fill();
      });
    });
  },
  camo_urban: (p) => {
    const cols = ["#5b5f63", "#8b8f93", "#2f3236", "#b9bcbf"];
    const blobs = (g: CanvasRenderingContext2D, seed: number, a0: number, a1: number, b0: number, b1: number) => {
      const r = rng(seed);
      for (let i = 0; i < 60; i++) {
        g.fillStyle = cols[i % cols.length];
        g.beginPath();
        const y = a0 + r() * (a1 - a0);
        const z = b0 + r() * (b1 - b0);
        g.moveTo(y, z);
        for (let k = 0; k < 6; k++) g.lineTo(y + (r() - 0.5) * 0.6, z + (r() - 0.5) * 0.35);
        g.fill();
      }
    };
    p.sides((g) => blobs(g, 5, p.y0, p.y1, p.z0 - 0.2, p.z1 + 0.6));
    p.top((g) => blobs(g, 9, p.y0, p.y1, -1.1, 1.1));
    for (const e of [1, -1] as const) p.end(e, (g) => blobs(g, e > 0 ? 13 : 17, -1.1, 1.1, 0, 1.8));
  },
  camo_digital: (p) => {
    const cols = ["#4a5a3a", "#6d7a52", "#2f3a26", "#8a8f6a"];
    const pix = (g: CanvasRenderingContext2D, seed: number, a0: number, a1: number, b0: number, b1: number) => {
      const r = rng(seed);
      const s = 0.06;
      for (let a = a0; a < a1; a += s)
        for (let b = b0; b < b1; b += s) {
          if (r() < 0.45) continue;
          g.fillStyle = cols[Math.floor(r() * cols.length)];
          g.fillRect(a, b, s * (1 + Math.floor(r() * 2)), s);
        }
    };
    p.sides((g) => pix(g, 3, p.y0, p.y1, p.z0 - 0.2, p.z1 + 0.6));
    p.top((g) => pix(g, 4, p.y0, p.y1, -1.1, 1.1));
  },
  sunset_fade: (p) => {
    p.sides((g) => {
      const grad = g.createLinearGradient(0, p.z0, 0, p.z1 + 0.4);
      grad.addColorStop(0, "#ff7a1a");
      grad.addColorStop(0.5, "#e8336b");
      grad.addColorStop(1, "#5a2a8a");
      g.fillStyle = grad;
      g.fillRect(p.y0, p.z0 - 0.3, p.y1 - p.y0, p.z1 - p.z0 + 1);
    });
    p.top((g) => {
      const grad = g.createLinearGradient(p.y0, 0, p.y1, 0);
      grad.addColorStop(0, "#5a2a8a");
      grad.addColorStop(1, "#e8336b");
      g.fillStyle = grad;
      g.fillRect(p.y0, -1.2, p.y1 - p.y0, 2.4);
    });
    for (const e of [1, -1] as const)
      p.end(e, (g) => {
        g.fillStyle = e > 0 ? "#e8336b" : "#ff7a1a";
        g.fillRect(-1.2, 0, 2.4, 2);
      });
  },
  circuit: (p) => {
    const trace = (g: CanvasRenderingContext2D, seed: number, a0: number, a1: number, b0: number, b1: number) => {
      const r = rng(seed);
      g.strokeStyle = "#2ad1ff";
      g.fillStyle = "#2ad1ff";
      g.lineWidth = 0.012;
      for (let i = 0; i < 18; i++) {
        let a = a0 + r() * (a1 - a0);
        let b = b0 + r() * (b1 - b0);
        g.beginPath();
        g.moveTo(a, b);
        for (let k = 0; k < 4; k++) {
          if (k % 2) b += (r() - 0.5) * 0.3;
          else a += (r() - 0.5) * 0.8;
          g.lineTo(a, b);
        }
        g.stroke();
        g.beginPath();
        g.arc(a, b, 0.022, 0, Math.PI * 2);
        g.fill();
      }
    };
    p.sides((g) => trace(g, 7, p.y0, p.y1, p.z0, p.z1));
    p.top((g) => trace(g, 8, p.y0, p.y1, -0.8, 0.8));
  },
  skull_trail: (p, k) => {
    const skull = (g: CanvasRenderingContext2D, y: number, z: number, s: number) => {
      g.fillStyle = k.ink;
      g.beginPath();
      g.arc(y, z, s, 0, Math.PI * 2);
      g.fill();
      g.fillRect(y - s * 0.55, z - s * 1.25, s * 1.1, s * 0.6);
      g.fillStyle = k.base;
      for (const d of [-0.4, 0.4]) {
        g.beginPath();
        g.arc(y + d * s, z - s * 0.05, s * 0.28, 0, Math.PI * 2);
        g.fill();
      }
      g.fillRect(y - s * 0.08, z - s * 0.6, s * 0.16, s * 0.25);
    };
    p.sides((g) => {
      for (let i = 0; i < 5; i++) skull(g, p.y1 - 0.8 - i * 0.62, p.z0 + 0.25 + i * 0.03, 0.14 - i * 0.018);
    });
  },
};

/** Ids of the vinyl designs. */
export const VINYL_IDS = Object.keys(DESIGNS);

/** Carbon weave tile (pixel space). */
function carbonPattern(g: CanvasRenderingContext2D): CanvasPattern | string {
  const c = document.createElement("canvas");
  c.width = c.height = 8;
  const t = c.getContext("2d");
  if (!t) return "#202326";
  t.fillStyle = "#1b1d20";
  t.fillRect(0, 0, 8, 8);
  t.fillStyle = "#34383d";
  t.fillRect(0, 0, 4, 4);
  t.fillRect(4, 4, 4, 4);
  t.fillStyle = "#2a2d31";
  t.fillRect(0, 1, 4, 1);
  t.fillRect(4, 5, 4, 1);
  return g.createPattern(c, "repeat") ?? "#202326";
}

/** Contrast inks for a paint colour. */
export function inksFor(paint: THREE.Color): Inks {
  const lum = 0.2126 * paint.r + 0.7152 * paint.g + 0.0722 * paint.b;
  const dark = "#151619";
  const light = "#f2f2ee";
  return { ink: lum > 0.45 ? dark : light, base: lum > 0.45 ? light : dark, accent: "#ff3b2f" };
}

/**
 * The livery canvas of a look: base paint, carbon panels, the vinyl. Null when there is nothing
 * to draw (plain paint needs no texture).
 */
export function liveryCanvas(frame: CarFrame, paint: THREE.Color, vinyl: string | undefined, carbon: Set<"hood" | "trunk">, old: HTMLCanvasElement | null): HTMLCanvasElement | null {
  const design = vinyl ? DESIGNS[vinyl] : undefined;
  if (!design && carbon.size === 0) return null;
  const cv = old ?? document.createElement("canvas");
  cv.width = W;
  cv.height = H;
  const g = cv.getContext("2d");
  if (!g) return null;
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.fillStyle = `#${paint.getHexString()}`;
  g.fillRect(0, 0, W, H);
  const doors = frame.region("LeftDoorF") ?? frame.region("RightDoorF");
  const p = new Painter(g, frame.atlas, frame.body.min.z + 0.03, doors?.max.z ?? frame.body.min.z + 0.7);
  for (const panel of carbon) {
    const b = panel === "hood" ? frame.hood : frame.trunk;
    p.fillPx("top", b.min.y, b.min.x, b.max.y, b.max.x, carbonPattern(g));
  }
  design?.(p, inksFor(paint));
  g.setTransform(1, 0, 0, 1, 0, 0);
  return cv;
}
