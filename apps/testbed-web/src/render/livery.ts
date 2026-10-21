// Police liveries (wave-2 style-police; spec: docs/design/vehicles/research_police.md §1, §3).
//
// One CanvasTexture per livery scheme (= per car model, cached), painted onto the livery atlas
// of the car's mesh (layout: data/panels/README.md "Livery atlas"; helpers liveryPx /
// liveryBandPx in render/panels.ts). The canvas covers only U 0–0.5 (the paint half of the
// atlas) and is stretched over it with `repeat.x = 2`, which doubles the horizontal
// resolution. Two-tone regions are measured from the mesh's own panel parts (door, roof,
// hood, trunk boxes), so the paint edges sit on the real shut lines.
//
// Text is anchored with plain fillText (never mirrored): the atlas already reads left → right
// from outside on both sides. Glyph widths are corrected for the band's px/m anisotropy.
// Unit numbers vary per car, so they are not in the shared canvas: they go on the mesh's
// `decal_*` carrier parts with a small per-unit texture (`applyUnitDecals`).
//
// Fictional Port Vesper Police Department (PVPD) only; generic words otherwise.
import * as THREE from "three";
import type { CarMeshJson, LiverySide, LiveryAtlas, PanelBody } from "./panels";
import { DEFAULT_ATLAS, liveryPx } from "./panels";

/** Canvas size of a livery (covers atlas U 0–0.5). */
const W = 2048;
const H = 1536;
/** Bold condensed sans (system stack; no font download). */
const FONT = `"Arial Narrow", "Roboto Condensed", "Liberation Sans Narrow", "Helvetica Neue", Arial, sans-serif`;
/** Fonts that are not condensed get this extra horizontal squeeze. */
const NARROW_FALLBACK = 0.86;

type Pt = [number, number];

/** Axis-aligned boxes of the mesh's body parts (chassis space, m), by part name. */
export type PartBoxes = Map<string, THREE.Box3>;

/** Rest-pose bounding boxes of every non-wheel part of a mesh file. */
export function partBoxes(mesh: CarMeshJson): PartBoxes {
  const out: PartBoxes = new Map();
  for (const p of mesh.parts) {
    if (p.wheel != null) continue;
    const b = new THREE.Box3();
    const v = new THREE.Vector3();
    for (let i = 0; i < p.positions_mm.length; i += 3)
      b.expandByPoint(v.set(p.positions_mm[i] * 1e-3, p.positions_mm[i + 1] * 1e-3, p.positions_mm[i + 2] * 1e-3));
    out.set(p.name, b);
  }
  return out;
}

const hex = (c: [number, number, number] | undefined, fallback: string): string =>
  c ? `rgb(${c[0]},${c[1]},${c[2]})` : fallback;

/** Text options. */
interface TextOpts {
  fill: string;
  /** Outline colour and width (m). */
  stroke?: string;
  strokeM?: number;
  /** Horizontal scale of the glyphs (condensed 0.8, extended 1.25). */
  cond?: number;
  /** Italic shear, glyph tops toward the car's front (harrier). */
  lean?: number;
  /** Letter spacing, em. */
  track?: number;
  alpha?: number;
}

/** Car-space painter over one livery canvas. */
class Painter {
  readonly g: CanvasRenderingContext2D;
  constructor(readonly canvas: HTMLCanvasElement, readonly atlas: LiveryAtlas) {
    this.g = canvas.getContext("2d")!;
  }

  /** Car space → canvas pixel (the canvas maps U 0–0.5, so it is half of a 2W-wide atlas). */
  px(side: LiverySide, a: number, b: number): { x: number; y: number } {
    return liveryPx(this.atlas, side, a, b, 2 * W, H);
  }

  /** Pixels per metre along a and b in a band. */
  scale(side: LiverySide, a: number, b: number): { pa: number; pb: number } {
    const p0 = this.px(side, a, b);
    const p1 = this.px(side, a + 0.1, b);
    const p2 = this.px(side, a, b + 0.1);
    return { pa: Math.hypot(p1.x - p0.x, p1.y - p0.y) * 10, pb: Math.hypot(p2.x - p0.x, p2.y - p0.y) * 10 };
  }

  /** Fill a whole band (with 8 px bleed into the padding). */
  band(side: LiverySide, fill: string): void {
    const a = this.atlas;
    const [lo, hi] = side === "left" || side === "right" || side === "top" ? [a.y[0], a.y[1]] : [-a.x_half, a.x_half];
    const [blo, bhi] = side === "top" ? [-a.x_half, a.x_half] : [a.z[0], a.z[1]];
    this.poly(side, [[lo, blo], [hi, blo], [hi, bhi], [lo, bhi]], fill, 8);
  }

  /** Polygon in band coordinates (a, b), optional bleed (px). */
  poly(side: LiverySide, pts: Pt[], fill: string, bleed = 2): void {
    const g = this.g;
    g.beginPath();
    pts.forEach(([a, b], i) => {
      const p = this.px(side, a, b);
      if (i === 0) g.moveTo(p.x, p.y);
      else g.lineTo(p.x, p.y);
    });
    g.closePath();
    g.fillStyle = fill;
    g.fill();
    if (bleed > 0) {
      g.strokeStyle = fill;
      g.lineWidth = bleed;
      g.stroke();
    }
  }

  rect(side: LiverySide, a0: number, a1: number, b0: number, b1: number, fill: string): void {
    this.poly(side, [[a0, b0], [a1, b0], [a1, b1], [a0, b1]], fill);
  }

  /** Same polygon on both side bands. */
  sides(pts: Pt[], fill: string): void {
    this.poly("left", pts, fill);
    this.poly("right", pts, fill);
  }

  /** Polyline stroke of `widthM` metres (measured along b). */
  line(side: LiverySide, pts: Pt[], widthM: number, stroke: string): void {
    const g = this.g;
    const { pb } = this.scale(side, pts[0][0], pts[0][1]);
    g.beginPath();
    pts.forEach(([a, b], i) => {
      const p = this.px(side, a, b);
      if (i === 0) g.moveTo(p.x, p.y);
      else g.lineTo(p.x, p.y);
    });
    g.strokeStyle = stroke;
    g.lineWidth = Math.max(1, widthM * pb);
    g.lineJoin = "round";
    g.stroke();
  }

  /**
   * Text with its baseline centre at (a, b) and cap height `capM`. Sides / front / rear: upright
   * as seen from outside. Top band: glyph tops toward the car's front, reading from behind.
   */
  text(side: LiverySide, str: string, a: number, b: number, capM: number, o: TextOpts): void {
    const g = this.g;
    const p = this.px(side, a, b);
    const { pa, pb } = this.scale(side, a, b);
    const top = side === "top";
    // Up direction in px/m: sides → b (z); top → a (y, toward the nose).
    const upPx = top ? pa : pb;
    const alongPx = top ? pb : pa;
    const fontPx = (capM * upPx) / 0.72;
    g.save();
    g.translate(p.x, p.y);
    if (top) g.rotate(Math.PI / 2);
    // Lean: tops toward the car's front. Left band: front = reader's left (c > 0 moves tops
    // left); right band: front = reader's right.
    const lean = o.lean ?? 0;
    const c = side === "left" ? lean : side === "right" ? -lean : 0;
    if (c !== 0) g.transform(1, 0, c, 1, 0, 0);
    g.scale((o.cond ?? 0.82) * NARROW_FALLBACK * (alongPx / upPx), 1);
    g.font = `800 ${fontPx.toFixed(1)}px ${FONT}`;
    g.textAlign = "center";
    g.textBaseline = "alphabetic";
    try {
      (g as unknown as { letterSpacing: string }).letterSpacing = `${((o.track ?? 0.04) * fontPx).toFixed(1)}px`;
    } catch {
      /* older canvas */
    }
    g.globalAlpha = o.alpha ?? 1;
    if (o.stroke && o.strokeM) {
      g.lineJoin = "round";
      g.lineWidth = 2 * o.strokeM * upPx;
      g.strokeStyle = o.stroke;
      g.strokeText(str, 0, 0);
    }
    g.fillStyle = o.fill;
    g.fillText(str, 0, 0);
    g.restore();
  }

  /** Text on both side bands. */
  sideText(str: string, y: number, z: number, capM: number, o: TextOpts): void {
    this.text("left", str, y, z, capM, o);
    this.text("right", str, y, z, capM, o);
  }

  /** PVPD emblem: a lighthouse (tapered tower, lamp room, rays) inside a ring; never a seal. */
  emblem(side: LiverySide, y: number, z: number, diaM: number, colour: string, opts: { alpha?: number; solid?: string } = {}): void {
    const g = this.g;
    const p = this.px(side, y, z);
    const { pa, pb } = this.scale(side, y, z);
    const r = (diaM / 2) * pb;
    g.save();
    g.translate(p.x, p.y);
    g.scale(pa / pb, 1); // round on the car
    g.globalAlpha = opts.alpha ?? 1;
    if (opts.solid) {
      g.fillStyle = opts.solid;
      g.beginPath();
      g.arc(0, 0, r, 0, Math.PI * 2);
      g.fill();
    }
    g.strokeStyle = colour;
    g.fillStyle = colour;
    g.lineWidth = Math.max(1.5, r * 0.08);
    g.beginPath();
    g.arc(0, 0, r * 0.92, 0, Math.PI * 2);
    g.stroke();
    // Tower (3 bands), lamp room, roof, rays.
    const u = r / 10;
    g.beginPath();
    g.moveTo(-3.2 * u, 7 * u);
    g.lineTo(3.2 * u, 7 * u);
    g.lineTo(1.9 * u, -2.5 * u);
    g.lineTo(-1.9 * u, -2.5 * u);
    g.closePath();
    g.fill();
    g.clearRect(-4 * u, 1.2 * u, 8 * u, 0.9 * u);
    g.clearRect(-4 * u, 4.2 * u, 8 * u, 0.9 * u);
    if (opts.solid) {
      g.fillStyle = opts.solid;
      g.fillRect(-4 * u, 1.2 * u, 8 * u, 0.9 * u);
      g.fillRect(-4 * u, 4.2 * u, 8 * u, 0.9 * u);
      g.fillStyle = colour;
    }
    g.fillRect(-1.5 * u, -4.4 * u, 3 * u, 1.6 * u);
    g.beginPath();
    g.moveTo(-2 * u, -4.4 * u);
    g.lineTo(0, -6 * u);
    g.lineTo(2 * u, -4.4 * u);
    g.closePath();
    g.fill();
    g.lineWidth = Math.max(1, u * 0.6);
    for (const s of [-1, 1])
      for (const k of [-1, 0, 1]) {
        g.beginPath();
        g.moveTo(s * 2.2 * u, -3.6 * u);
        g.lineTo(s * 6.5 * u, (-3.6 + k * 1.6) * u);
        g.stroke();
      }
    g.restore();
  }
}

/** What a scheme painter can measure on the car. */
interface CarGeo {
  atlas: LiveryAtlas;
  box: (name: string) => THREE.Box3 | null;
  /** Front door (y0 rear edge, y1 front edge), rear door (or the quarter's front part). */
  fd: [number, number];
  rd: [number, number];
  /** Door bottom / belt (door top) heights. */
  zDoor: [number, number];
  roof: THREE.Box3;
  hood: THREE.Box3 | null;
  trunk: THREE.Box3 | null;
  /** %L → y. */
  y: (pct: number) => number;
  colors: [number, number, number][];
}

function geo(mesh: CarMeshJson, boxes: PartBoxes): CarGeo {
  const atlas = mesh.atlas ?? DEFAULT_ATLAS;
  const box = (n: string) => boxes.get(n) ?? null;
  const fdB = box("door_fl");
  const rdB = box("door_rl");
  const fd: [number, number] = fdB ? [fdB.min.y, fdB.max.y] : [-0.4, 0.5];
  const rd: [number, number] = rdB ? [rdB.min.y, rdB.max.y] : [fd[0] - 0.7, fd[0]];
  const zDoor: [number, number] = fdB ? [fdB.min.z, fdB.max.z] : [0.3, 0.9];
  const roof = box("roof") ?? new THREE.Box3(new THREE.Vector3(-0.7, -0.8, 1.4), new THREE.Vector3(0.7, 0.3, 1.5));
  const L = atlas.y[1] - atlas.y[0];
  return {
    atlas,
    box,
    fd,
    rd,
    zDoor,
    roof,
    hood: box("hood"),
    trunk: box("trunk"),
    y: (pct) => atlas.y[1] - pct * L,
    colors: mesh.livery?.colors ?? [],
  };
}

type SchemePainter = (p: Painter, c: CarGeo) => void;

/** §1.1 / §1.2 classic black-and-white (`pvpd_bw`, `pvpd_bw_classic`). */
function blackWhite(classic: boolean): SchemePainter {
  return (p, c) => {
    const black = hex(c.colors[0], classic ? "#16171A" : "#111317");
    const white = hex(c.colors[1], classic ? "#EEEEE8" : "#F3F4F6");
    const a = c.atlas;
    for (const s of ["left", "right", "top", "front", "rear"] as const) p.band(s, black);
    // White doors (rocker → belt), the pillars between them and the roof.
    const [zr, zb] = [0.30, c.zDoor[1] + 0.01];
    const y0 = c.rd[0];
    const y1 = c.fd[1];
    p.sides([[y0, zr], [y1, zr], [y1, zb], [y0, zb]], white);
    const deck = c.trunk ? c.trunk.max.y : c.roof.min.y - 0.4;
    p.sides([[deck, zb - 0.02], [c.roof.max.y - 0.05, zb - 0.02], [c.roof.max.y - 0.05, a.z[1]], [deck, a.z[1]]], white);
    p.rect("top", deck, c.roof.max.y + 0.02, -a.x_half, a.x_half, white);
    if (!classic) {
      // Silver pinline on the tone edges (the modern car's doors follow the shut lines).
      for (const y of [y0, y1]) {
        p.line("left", [[y, zr], [y, zb]], 0.01, hex(c.colors[2], "#B8BCC2"));
        p.line("right", [[y, zr], [y, zb]], 0.01, hex(c.colors[2], "#B8BCC2"));
      }
    }
    const fdMid = (c.fd[0] + c.fd[1]) / 2;
    const rdMid = (c.rd[0] + c.rd[1]) / 2;
    if (classic) {
      // 80s: extended upright sans; emblem disc above POLICE; PORT VESPER on the rear door.
      for (const s of ["left", "right"] as const) p.emblem(s, fdMid, zb - 0.11, 0.17, white, { solid: black });
      p.sideText("POLICE", fdMid, 0.60, 0.13, { fill: black, cond: 1.25, track: 0.06 });
      p.sideText("PORT VESPER", rdMid, 0.66, 0.06, { fill: black, cond: 1.2 });
    } else {
      for (const s of ["left", "right"] as const) p.emblem(s, fdMid, 0.63, 0.30, black, { alpha: 0.22 });
      p.sideText("POLICE", fdMid, 0.56, 0.16, { fill: black, track: 0.05 });
      p.sideText("PORT VESPER", fdMid, 0.76, 0.06, { fill: black });
      p.sideText("DIAL 911", rdMid, 0.48, 0.07, { fill: black });
    }
    // Trunk lid (black): POLICE reading from behind; rear bumper DIAL 911.
    if (c.trunk) p.text("top", "POLICE", c.trunk.min.y + 0.62, 0, classic ? 0.09 : 0.10, { fill: white, cond: classic ? 1.2 : 0.85 });
    p.text("rear", "DIAL 911", 0, 0.42, 0.05, { fill: white });
    p.text("front", "POLICE", 0, 0.42, 0.06, { fill: white });
  };
}

/** §1.3 highway patrol tan / umber with gold pinstripes (`pvpd_highway`). */
const highway: SchemePainter = (p, c) => {
  const tan = hex(c.colors[0], "#CDBB8E");
  const umber = hex(c.colors[1], "#2B241C");
  const gold = hex(c.colors[2], "#C99A2E");
  const goldText = hex(c.colors[3], "#D9AA3A");
  const a = c.atlas;
  for (const s of ["left", "right", "front", "rear"] as const) p.band(s, umber);
  p.band("top", tan);
  // Tone split at the shoulder (door top): tan above.
  const zs = c.zDoor[1] - 0.035;
  p.sides([[a.y[0], zs], [a.y[1], zs], [a.y[1], a.z[1]], [a.y[0], a.z[1]]], tan);
  // Gold pinstripe pair on the split, hockey-stick drop toward the headlamp.
  const nose = a.y[1];
  const stick = (dz: number): Pt[] => [
    [a.y[0] + 0.05, zs + dz],
    [nose - 0.55, zs + dz],
    [nose - 0.30, zs + dz - 0.025],
    [nose - 0.12, zs + dz - 0.06],
  ];
  for (const s of ["left", "right"] as const) {
    p.line(s, stick(-0.009), 0.018, gold);
    p.line(s, stick(-0.030), 0.006, gold);
  }
  const t = { fill: goldText, stroke: "#000", strokeM: 0.006, cond: 0.80 };
  p.sideText("HIGHWAY PATROL", c.fd[0], 0.60, 0.12, t);
  p.sideText("PORT VESPER", (c.rd[0] + c.rd[1]) / 2, 0.46, 0.055, { fill: goldText });
  const yEmb = c.fd[1] + 0.25;
  for (const s of ["left", "right"] as const) p.emblem(s, yEmb, 0.70, 0.20, goldText);
  if (c.trunk) p.text("top", "HIGHWAY PATROL", c.trunk.min.y + 0.55, 0, 0.08, { fill: umber, cond: 0.8 });
  p.text("rear", "HIGHWAY PATROL", 0, 0.45, 0.05, { fill: goldText });
};

/** §1.4 SUV white + navy lower band + cyan stripe with chevrons (`pvpd_suv`). */
const suv: SchemePainter = (p, c) => {
  const white = hex(c.colors[0], "#F2F4F5");
  const navy = hex(c.colors[1], "#14223F");
  const cyan = hex(c.colors[2], "#1E7FD0");
  const green = hex(c.colors[3], "#2FA15A");
  const a = c.atlas;
  for (const s of ["left", "right", "top", "front", "rear"] as const) p.band(s, white);
  // Navy lower body, bumpers and hood top.
  p.sides([[a.y[0], a.z[0]], [a.y[1], a.z[0]], [a.y[1], 0.55], [a.y[0], 0.55]], navy);
  for (const s of ["front", "rear"] as const) p.rect(s, -a.x_half, a.x_half, a.z[0], 0.62, navy);
  if (c.hood) p.rect("top", c.hood.min.y + 0.02, a.y[1], -a.x_half, a.x_half, navy);
  // Cyan band z 0.60–0.70 + green pinline, breaking into forward-raked chevrons at the rear.
  const yChev = a.y[0] + 0.85;
  p.sides([[yChev, 0.60], [a.y[1], 0.60], [a.y[1], 0.70], [yChev, 0.70]], cyan);
  p.sides([[a.y[0] + 0.1, 0.585], [a.y[1], 0.585], [a.y[1], 0.597], [a.y[0] + 0.1, 0.597]], green);
  for (let i = 0; i < 4; i++) {
    const y = yChev - 0.06 - i * 0.18;
    // Parallelogram leaning forward (top ahead of the bottom).
    p.sides([[y - 0.10, 0.60], [y, 0.60], [y + 0.10, 0.70], [y, 0.70]], cyan);
  }
  p.sideText("POLICE", c.fd[0], 0.80, 0.22, { fill: navy, track: 0.06 });
  p.sideText("PORT VESPER", c.y(0.18), 1.02, 0.07, { fill: navy });
  for (const s of ["left", "right"] as const) p.emblem(s, c.y(0.58), 1.05, 0.26, navy);
  p.text("rear", "POLICE", 0, 0.92, 0.11, { fill: navy, cond: 0.85 });
  p.text("rear", "DIAL 911", 0, 0.42, 0.06, { fill: white });
  p.text("front", "POLICE", 0, 0.45, 0.07, { fill: white });
  if (c.hood) p.text("top", "POLICE", (c.hood.min.y + a.y[1]) / 2, 0, 0.16, { fill: white, cond: 0.85 });
};

/** §1.5 diagonal black / white split + blue accent, italic lettering (`pvpd_pursuit`). */
const pursuit: SchemePainter = (p, c) => {
  const black = hex(c.colors[0], "#0E0F12");
  const white = hex(c.colors[1], "#F4F5F7");
  const blue = hex(c.colors[2], "#1C6CFF");
  const a = c.atlas;
  for (const s of ["left", "right", "top", "front"] as const) p.band(s, black);
  p.band("rear", white);
  // White below / behind a line from the front door's leading edge at the rocker, rising
  // rearward at 18° to the belt, then along the belt to the tail.
  const yF = c.fd[1];
  const zr = 0.30;
  const zb = c.zDoor[1] - 0.02;
  const yMeet = yF - (zb - zr) / Math.tan((18 * Math.PI) / 180);
  p.sides([[yF, a.z[0]], [yF, zr], [yMeet, zb], [a.y[0], zb], [a.y[0], a.z[0]]], white);
  // Blue accent 0.02 above the split, on the black side.
  const off = 0.03;
  for (const s of ["left", "right"] as const)
    p.line(s, [[yF + 0.02, zr + off], [yMeet + 0.02, zb + off], [a.y[0], zb + off]], 0.035, blue);
  // Twin white stripes over hood and deck; the roof stays black for the aerial number.
  for (const sx of [-1, 1]) {
    p.rect("top", c.roof.max.y + 0.02, a.y[1], sx * 0.02, sx * 0.15, white);
    p.rect("top", a.y[0], c.roof.min.y - 0.02, sx * 0.02, sx * 0.15, white);
  }
  // POLICE where the white panel is tall enough (behind the door, under the rising split).
  const it = { fill: black, cond: 0.80, lean: 0.17 };
  p.sideText("POLICE", c.y(0.58), 0.31, 0.14, it);
  p.sideText("PORT VESPER", c.fd[1] + 0.24, 0.66, 0.05, { ...it, fill: white });
  p.text("rear", "POLICE", 0, 0.60, 0.09, { fill: black, cond: 0.85 });
  p.text("rear", "PURSUIT", 0, 0.50, 0.05, { fill: black, cond: 0.85 });
  p.text("front", "DIAL 911", 0, 0.30, 0.05, { fill: white });
};

const SCHEMES: Record<string, SchemePainter> = {
  pvpd_bw: blackWhite(false),
  pvpd_bw_classic: blackWhite(true),
  pvpd_highway: highway,
  pvpd_suv: suv,
  pvpd_pursuit: pursuit,
};

/** True if the mesh carries a livery scheme this painter knows. */
export function hasPoliceLivery(mesh: CarMeshJson | null): boolean {
  return !!mesh?.livery && mesh.livery.scheme in SCHEMES;
}

const liveryCache = new Map<string, THREE.CanvasTexture>();

/** The shared livery texture of a car model (null if its mesh has no known scheme). */
export function liveryTexture(mesh: CarMeshJson, anisotropy = 8): THREE.CanvasTexture | null {
  const scheme = mesh.livery?.scheme;
  if (!scheme || !(scheme in SCHEMES)) return null;
  const key = `${mesh.id}:${scheme}`;
  const hit = liveryCache.get(key);
  if (hit) return hit;
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const painter = new Painter(canvas, mesh.atlas ?? DEFAULT_ATLAS);
  SCHEMES[scheme](painter, geo(mesh, partBoxes(mesh)));
  const tex = new THREE.CanvasTexture(canvas);
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.repeat.set(2, 1); // canvas = atlas U 0–0.5
  tex.wrapS = THREE.ClampToEdgeWrapping;
  tex.anisotropy = anisotropy;
  tex.minFilter = THREE.LinearMipmapLinearFilter;
  tex.generateMipmaps = true;
  liveryCache.set(key, tex);
  return tex;
}

/** Base (first) colour of a scheme, for paint parts that do not take the livery (mirrors). */
export function liveryBaseColour(mesh: CarMeshJson): THREE.Color | null {
  const c = mesh.livery?.colors?.[0];
  return c ? new THREE.Color().setRGB(c[0] / 255, c[1] / 255, c[2] / 255, THREE.SRGBColorSpace) : null;
}

// ------------------------------------------------------------------------------------------
// Unit numbers (decal carriers)
// ------------------------------------------------------------------------------------------

/** 32-bit integer hash (stable phase / unit numbers from an id). */
export function hash32(n: number): number {
  let x = (Math.floor(n) ^ 0x9e3779b9) >>> 0;
  x = Math.imul(x ^ (x >>> 16), 0x85ebca6b) >>> 0;
  x = Math.imul(x ^ (x >>> 13), 0xc2b2ae35) >>> 0;
  return (x ^ (x >>> 16)) >>> 0;
}

/** Body unit string from a car model and a stable id: `C-12`, `C-44`, `L-L1`, `S-B3`, `C-H1`. */
export function unitNumber(carId: string, id: number): string {
  const h = hash32(id);
  const precinct = "CHNSWC"[h % 6];
  const n = (k: number) => 1 + ((h >>> 8) % k);
  switch (carId) {
    case "sentinel_warden":
      return `${precinct}-${40 + ((h >>> 8) % 20)}`;
    case "sentinel_wolfhound":
      return `L-L${n(9)}`;
    case "sentinel_bastion":
      return `${precinct}-B${n(9)}`;
    case "sentinel_harrier":
      return `${precinct}-H${n(9)}`;
    default:
      return `${precinct}-${String(n(19)).padStart(2, "0")}`;
  }
}

/** Decal text colours per scheme: side, roof, trunk (fill, optional outline). */
const DECAL_STYLE: Record<string, Record<"side" | "roof" | "trunk", { fill: string; stroke?: string }>> = {
  pvpd_bw: { side: { fill: "#F3F4F6" }, roof: { fill: "#111317" }, trunk: { fill: "#F3F4F6" } },
  pvpd_bw_classic: { side: { fill: "#EEEEE8" }, roof: { fill: "#16171A" }, trunk: { fill: "#EEEEE8" } },
  pvpd_highway: { side: { fill: "#D9AA3A", stroke: "#000" }, roof: { fill: "#2B241C" }, trunk: { fill: "#2B241C" } },
  pvpd_suv: { side: { fill: "#14223F" }, roof: { fill: "#14223F" }, trunk: { fill: "#14223F" } },
  pvpd_pursuit: { side: { fill: "#0E0F12" }, roof: { fill: "#F4F5F7" }, trunk: { fill: "#0E0F12" } },
};

const decalCache = new Map<string, THREE.MeshStandardMaterial>();

/** A transparent decal material showing `text` on a canvas with aspect `aspect` (w / h). */
function decalMaterial(text: string, aspect: number, fill: string, stroke: string | undefined, lean: number): THREE.MeshStandardMaterial {
  const key = `${text}|${aspect.toFixed(2)}|${fill}|${stroke ?? ""}|${lean}`;
  const hit = decalCache.get(key);
  if (hit) return hit;
  const h = 128;
  const w = Math.min(1024, Math.max(64, Math.round(h * aspect)));
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const g = c.getContext("2d")!;
  const font = (px: number) => `800 ${px}px ${FONT}`;
  let px = h * 0.92;
  g.font = font(px);
  const fit = (w * 0.92) / Math.max(1, g.measureText(text).width);
  // Keep glyphs condensed-looking: shrink width first, height only if needed.
  const sx = Math.min(1, fit) * NARROW_FALLBACK;
  if (fit < 0.55) px *= fit / 0.55;
  g.translate(w / 2, h * 0.86);
  if (lean) g.transform(1, 0, lean, 1, 0, 0);
  g.scale(Math.min(sx, fit), 1);
  g.font = font(px);
  g.textAlign = "center";
  g.textBaseline = "alphabetic";
  if (stroke) {
    g.lineJoin = "round";
    g.lineWidth = px * 0.08;
    g.strokeStyle = stroke;
    g.strokeText(text, 0, 0);
  }
  g.fillStyle = fill;
  g.fillText(text, 0, 0);
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.anisotropy = 8;
  const m = new THREE.MeshStandardMaterial({
    map: tex,
    transparent: true,
    alphaTest: 0.35,
    roughness: 0.5,
    metalness: 0.1,
    flatShading: true,
    side: THREE.DoubleSide,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
  });
  decalCache.set(key, m);
  return m;
}

/**
 * Re-map the `decal_*` carrier parts under `root` (meshes named after their part) onto
 * per-unit number textures: rear quarters / fenders get `C-12`, roof and trunk `C12`-style
 * strings. Side decals read left → right from outside; roof / trunk read from behind with
 * glyph tops toward the front. Call once the meshes exist (rest pose).
 */
export function applyUnitDecals(root: THREE.Object3D, mesh: CarMeshJson, unitId: number): void {
  const scheme = mesh.livery?.scheme ?? "";
  const style = DECAL_STYLE[scheme];
  if (!style) return;
  const body = unitNumber(mesh.id, unitId);
  const roofStr = body.replace("-", "");
  const lean = scheme === "pvpd_pursuit" ? 0.17 : 0;
  root.traverse((o) => {
    const m = o as THREE.Mesh;
    if (!m.isMesh || !m.name.startsWith("decal_")) return;
    const pos = m.geometry.getAttribute("position") as THREE.BufferAttribute;
    const uv = m.geometry.getAttribute("uv") as THREE.BufferAttribute;
    if (!pos || !uv) return;
    m.geometry.computeBoundingBox();
    const b = m.geometry.boundingBox!;
    const sz = b.getSize(new THREE.Vector3());
    const top = m.name === "decal_roof" || m.name === "decal_trunk";
    const left = m.name.endsWith("_l");
    for (let i = 0; i < pos.count; i++) {
      const x = pos.getX(i);
      const y = pos.getY(i);
      const z = pos.getZ(i);
      let u: number;
      let v: number;
      if (top) {
        u = (x - b.min.x) / Math.max(sz.x, 1e-3);
        v = (y - b.min.y) / Math.max(sz.y, 1e-3);
      } else {
        const t = (y - b.min.y) / Math.max(sz.y, 1e-3);
        u = left ? 1 - t : t;
        v = (z - b.min.z) / Math.max(sz.z, 1e-3);
      }
      uv.setXY(i, u, v);
    }
    uv.needsUpdate = true;
    const aspect = top ? sz.x / Math.max(sz.y, 1e-3) : sz.y / Math.max(sz.z, 1e-3);
    const st = m.name === "decal_roof" ? style.roof : m.name === "decal_trunk" ? style.trunk : style.side;
    const text = top ? roofStr : body;
    // Side italic: tops toward the car's front (left side: reader's left).
    const l = top ? 0 : left ? lean : -lean;
    m.material = decalMaterial(text, aspect, st.fill, st.stroke, l);
  });
}

/**
 * Give a sim police car's panel mesh its scheme: shared livery texture on the livery parts,
 * the scheme's base colour on plain paint parts (mirrors), per-unit decals from `unitId`.
 * Skipped when the URL forces a livery (`?livery=grid` debug view) or the mesh has no scheme.
 */
export function setupPoliceLook(panels: PanelBody, mesh: CarMeshJson, unitId: number): void {
  let flag: string | null = null;
  try {
    flag = new URLSearchParams(globalThis.location?.search ?? "").get("livery");
  } catch {
    flag = null;
  }
  if (flag) return;
  const tex = liveryTexture(mesh);
  if (!tex) return;
  panels.setLivery(tex);
  const base = liveryBaseColour(mesh);
  if (base) panels.paint.color.copy(base);
  applyUnitDecals(panels.root, mesh, unitId);
}
