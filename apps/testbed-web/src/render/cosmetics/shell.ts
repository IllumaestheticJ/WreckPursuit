// Replacement panels: aftermarket bumpers, lips and carbon lids that take the stock panel's place.
//
// A bolt-on next to a stock bumper reads as a stuck-on box; a real aftermarket bumper is a new
// cover with its own shape. These builders copy the stock panel mesh of the car (so the new part
// fits every model), reshape it (deeper chin, pushed-out lower lip, wider corners), cut intakes
// into it (dark recess, honeycomb mesh, trim frame) and hide the stock cover and its trim. Carbon
// hoods and trunk lids are the stock lid's shape in a clear-coated twill weave. The copies ride
// with their region like every bolt-on piece (look.ts) and come off with the panel.
import * as THREE from "three";
import type { MeshPartJson } from "../panels";
import type { Builder, LookCtx } from "./context";
import type { CarFrame } from "./frame";
import { lampMat } from "./kit";

const V = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);
const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** Stock panel parts of a body region (the cover itself, not its trim). */
const panelOf = (region: string) => (p: MeshPartJson) => p.wheel == null && p.kind === "panel" && p.region === region;

/**
 * One geometry from the parts `pick` selects (chassis metres), each vertex passed through `warp`.
 * Null when the model has none.
 */
export function shellOf(frame: CarFrame, pick: (p: MeshPartJson) => boolean, warp?: (v: THREE.Vector3) => void): THREE.BufferGeometry | null {
  const pos: number[] = [];
  const idx: number[] = [];
  const v = new THREE.Vector3();
  for (const p of frame.mesh.parts) {
    if (!pick(p) || p.positions_mm.length === 0) continue;
    const base = pos.length / 3;
    const a = p.positions_mm;
    for (let i = 0; i < a.length; i += 3) {
      v.set(a[i] * 1e-3, a[i + 1] * 1e-3, a[i + 2] * 1e-3);
      warp?.(v);
      pos.push(v.x, v.y, v.z);
    }
    for (const i of p.indices) idx.push(base + i);
  }
  if (!idx.length) return null;
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  g.setIndex(idx);
  g.computeVertexNormals();
  return g;
}

/** Outermost face position of `geo` along y near (x, z): the front (max y) or rear (min y). */
function faceY(geo: THREE.BufferGeometry, x: number, z: number, front: boolean, fallback: number): number {
  const p = geo.getAttribute("position");
  let best = front ? -Infinity : Infinity;
  for (let i = 0; i < p.count; i++) {
    if (Math.abs(p.getX(i) - x) > 0.12 || Math.abs(p.getZ(i) - z) > 0.08) continue;
    best = front ? Math.max(best, p.getY(i)) : Math.min(best, p.getY(i));
  }
  return Number.isFinite(best) ? best : fallback;
}

// ----- materials ------------------------------------------------------------------------------

let weave: THREE.CanvasTexture | null = null;

/**
 * 2x2 twill carbon weave tile (light and dark tows on the diagonal). The contrast and the tow
 * size are exaggerated well past real carbon (a few mm per tow) so the weave still reads at
 * game distance and stays distinct from a satin-black painted hood.
 */
function weaveTexture(): THREE.CanvasTexture {
  if (weave) return weave;
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const g = c.getContext("2d")!;
  const tow = 16;
  for (let i = 0; i < 4; i++)
    for (let j = 0; j < 4; j++) {
      // Twill: the over/under pattern steps one tow per row.
      const over = (i + j) % 4 < 2;
      const grad = over ? g.createLinearGradient(i * tow, 0, i * tow + tow, 0) : g.createLinearGradient(0, j * tow, 0, j * tow + tow);
      grad.addColorStop(0, over ? "#14171b" : "#08090b");
      grad.addColorStop(0.5, over ? "#6d7682" : "#323840");
      grad.addColorStop(1, over ? "#14171b" : "#08090b");
      g.fillStyle = grad;
      g.fillRect(i * tow, j * tow, tow, tow);
    }
  weave = new THREE.CanvasTexture(c);
  weave.wrapS = weave.wrapT = THREE.RepeatWrapping;
  weave.colorSpace = THREE.SRGBColorSpace;
  weave.anisotropy = 8;
  return weave;
}

let carbonMat: THREE.MeshPhysicalMaterial | null = null;

/** Clear-coated carbon fibre (shared). */
export function carbonFibre(): THREE.MeshPhysicalMaterial {
  return (carbonMat ??= new THREE.MeshPhysicalMaterial({
    color: 0xffffff,
    map: weaveTexture(),
    roughness: 0.35,
    metalness: 0.3,
    clearcoat: 1,
    clearcoatRoughness: 0.03,
    side: THREE.DoubleSide,
    flatShading: true,
  }));
}

/** Planar UVs: x and y in car metres, one weave tile every `tile` metres (top-down panels). */
function topUV(geo: THREE.BufferGeometry, tile = 0.14): void {
  const p = geo.getAttribute("position");
  const uv = new Float32Array(p.count * 2);
  for (let i = 0; i < p.count; i++) {
    uv[i * 2] = p.getX(i) / tile;
    uv[i * 2 + 1] = p.getY(i) / tile;
  }
  geo.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
}

let honeycomb: THREE.MeshStandardMaterial | null = null;

/** Hexagonal intake mesh on a see-through background. */
function honeycombMat(): THREE.MeshStandardMaterial {
  if (honeycomb) return honeycomb;
  const c = document.createElement("canvas");
  c.width = c.height = 64;
  const g = c.getContext("2d")!;
  g.strokeStyle = "#2c2f34";
  g.lineWidth = 3;
  const r = 8;
  const h = Math.sqrt(3) * r;
  for (let row = -1; row < 6; row++)
    for (let col = -1; col < 6; col++) {
      const cx = col * 1.5 * r * 2 * 0.75 + (row % 2 ? 1.5 * r : 0);
      const cy = row * h * 0.5 * 1.15;
      g.beginPath();
      for (let k = 0; k <= 6; k++) {
        const a = (Math.PI / 3) * k;
        g.lineTo(cx + r * Math.cos(a), cy + r * Math.sin(a));
      }
      g.stroke();
    }
  const t = new THREE.CanvasTexture(c);
  t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = THREE.SRGBColorSpace;
  honeycomb = new THREE.MeshStandardMaterial({ map: t, transparent: true, alphaTest: 0.35, metalness: 0.4, roughness: 0.5, side: THREE.DoubleSide, forceSinglePass: true });
  return honeycomb;
}

// ----- intakes and lips -------------------------------------------------------------------------

/**
 * An opening in a bumper face at (x, z), `w` × `h` m: dark recess, honeycomb mesh, trim frame.
 * `front` faces forward (+y), else backward.
 */
function intake(c: LookCtx, geo: THREE.BufferGeometry, x: number, z: number, w: number, h: number, front: boolean, fallbackY: number): THREE.Group {
  const s = front ? 1 : -1;
  const y = faceY(geo, x, z, front, fallbackY);
  const g = new THREE.Group();
  const recess = new THREE.Mesh(new THREE.BoxGeometry(w, 0.05, h), c.kit.hole);
  recess.position.set(x, y - s * 0.02, z);
  const mat = honeycombMat().clone();
  mat.map = mat.map!.clone();
  mat.map.needsUpdate = true;
  mat.map.repeat.set(w / 0.05, h / 0.05);
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(w * 0.96, h * 0.92).rotateX(Math.PI / 2), mat);
  mesh.position.set(x, y + s * 0.006, z);
  g.add(recess, mesh);
  // Trim frame: four thin bars proud of the face.
  const t = 0.014;
  for (const dz of [-1, 1]) g.add(new THREE.Mesh(new THREE.BoxGeometry(w + t * 2, 0.02, t), c.kit.black).translateX(x).translateY(y + s * 0.006).translateZ(z + (dz * (h + t)) / 2));
  for (const dx of [-1, 1]) g.add(new THREE.Mesh(new THREE.BoxGeometry(t, 0.02, h), c.kit.black).translateX(x + (dx * (w + t)) / 2).translateY(y + s * 0.006).translateZ(z));
  return g;
}

/**
 * A lip blade following the bumper's lower edge in plan view: from the edge `reach` m outward and
 * slightly down. `front` picks the end of the car.
 */
function lipBlade(geo: THREE.BufferGeometry, m: THREE.Material, front: boolean, reach: number, drop = 0.012): THREE.Mesh | null {
  const p = geo.getAttribute("position");
  geo.computeBoundingBox();
  const b = geo.boundingBox!;
  const s = front ? 1 : -1;
  const zCut = b.min.z + 0.07;
  const bins = 24;
  const edge: { x: number; y: number; z: number }[] = [];
  for (let k = 0; k <= bins; k++) {
    const x = b.min.x + ((b.max.x - b.min.x) * k) / bins;
    let y = front ? -Infinity : Infinity;
    let z = Infinity;
    for (let i = 0; i < p.count; i++) {
      if (Math.abs(p.getX(i) - x) > (b.max.x - b.min.x) / bins || p.getZ(i) > zCut) continue;
      y = front ? Math.max(y, p.getY(i)) : Math.min(y, p.getY(i));
      z = Math.min(z, p.getZ(i));
    }
    if (Number.isFinite(y)) edge.push({ x: x * 0.97, y, z });
  }
  if (edge.length < 2) return null;
  const pos: number[] = [];
  const idx: number[] = [];
  for (const e of edge) {
    // Inner edge tucked under the bumper, outer edge out and down, plus a thin lower face.
    pos.push(e.x, e.y - s * 0.04, e.z + 0.004, e.x, e.y + s * reach, e.z - drop, e.x, e.y + s * reach, e.z - drop - 0.012, e.x, e.y - s * 0.04, e.z - 0.012);
  }
  for (let k = 0; k + 1 < edge.length; k++) {
    const a = k * 4;
    const n = a + 4;
    for (let f = 0; f < 3; f++) idx.push(a + f, n + f, a + f + 1, a + f + 1, n + f, n + f + 1);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  g.setIndex(idx);
  g.computeVertexNormals();
  return new THREE.Mesh(g, m);
}

/** A swept canard fin on a front bumper corner at (x, y, z), sticking out to `side`. */
function canard(m: THREE.Material, x: number, y: number, z: number, side: number): THREE.Mesh {
  const s = new THREE.Shape();
  s.moveTo(0, 0);
  s.lineTo(0.18, 0);
  s.lineTo(0.04, 0.07);
  s.closePath();
  const fin = new THREE.Mesh(new THREE.ExtrudeGeometry(s, { depth: 0.008, bevelEnabled: false }), m);
  // Shape x → back along the car, shape y → outward, extrusion → up; leading edge tilted up.
  const basis = new THREE.Matrix4().makeBasis(V(0, -1, 0), V(side, 0, 0), V(0, 0, 1));
  fin.setRotationFromMatrix(new THREE.Matrix4().makeRotationX(-0.22).multiply(basis));
  fin.position.set(x, y, z);
  return fin;
}

// ----- builders ---------------------------------------------------------------------------------

/**
 * Warp for a reshaped bumper: the lower part of the face pushed out by `push` m and dropped by
 * `drop` m (more toward the bottom edge), the corners widened by `widen` m.
 */
function bumperWarp(b: THREE.Box3, front: boolean, push: number, drop: number, widen: number, halfW: number) {
  const s = front ? 1 : -1;
  const faceAt = front ? b.max.y : b.min.y;
  return (v: THREE.Vector3) => {
    const t = clamp01((b.max.z - v.z) / Math.max(0.05, b.max.z - b.min.z));
    const onFace = clamp01(1 - Math.abs(faceAt - v.y) / 0.22);
    v.y += s * push * onFace * t * t;
    v.z -= drop * t * t * t;
    const out = Math.abs(v.x) / Math.max(0.3, halfW);
    v.x += Math.sign(v.x) * widen * out * out * out * clamp01(t * 1.5);
  };
}

/**
 * Aftermarket bumper covers (`bumper` piece; `end`: front or rear; `style`: aggressive, wide
 * for the front; sport, wide for the rear). Hides the stock cover and its trim.
 */
const bumper: Builder = (c, p) => {
  const front = p.end !== "rear";
  const region = front ? "FrontBumper" : "RearBumper";
  const f = c.frame;
  const b = front ? f.bumperFront : f.bumperRear;
  const wide = p.style === "wide";
  const aggressive = p.style === "aggressive";
  const warp = front
    ? bumperWarp(b, true, aggressive ? 0.06 : 0.035, aggressive ? 0.035 : 0.025, wide ? 0.06 : 0.015, f.halfW)
    : bumperWarp(b, false, 0.03, 0.03, wide ? 0.06 : 0.01, f.halfW);
  const geo = shellOf(f, panelOf(region), warp);
  if (!geo) return;
  // The cover and its trim, lamps and grilles go; the number plate moves over to the new cover.
  c.hide((d) => d.wheel == null && d.region === region && !d.name.startsWith("plate"));
  const g = new THREE.Group();
  g.add(new THREE.Mesh(geo, c.paint));
  geo.computeBoundingBox();
  const nb = geo.boundingBox!;
  const w = nb.max.x - nb.min.x;
  const h = nb.max.z - nb.min.z;
  const y0 = front ? nb.max.y : nb.min.y;
  if (front) {
    // Centre intake, side intakes, LED strips over them, lip blade, canards, tow strap.
    g.add(intake(c, geo, 0, nb.min.z + h * 0.33, w * (wide ? 0.56 : 0.5), h * (aggressive ? 0.4 : 0.32), true, y0));
    const sx = w * (aggressive ? 0.37 : 0.4);
    for (const s of [-1, 1]) {
      g.add(intake(c, geo, s * sx, nb.min.z + h * 0.3, w * 0.13, h * (aggressive ? 0.34 : 0.22), true, y0));
      const led = lampMat(0xfff1d6);
      c.lamp(led, 1.6);
      const ly = faceY(geo, s * sx, nb.min.z + h * 0.56, true, y0);
      g.add(new THREE.Mesh(new THREE.BoxGeometry(w * 0.13, 0.012, 0.014), led).translateX(s * sx).translateY(ly + 0.004).translateZ(nb.min.z + h * 0.56));
    }
    const blade = lipBlade(geo, c.kit.carbon, true, aggressive ? 0.08 : 0.05);
    if (blade) g.add(blade);
    if (aggressive) {
      for (const s of [-1, 1]) {
        const cx = s * (w / 2 - 0.05);
        const cy = faceY(geo, cx, nb.min.z + h * 0.35, true, y0) - 0.06;
        g.add(canard(c.kit.carbon, cx, cy, nb.min.z + h * 0.42, s));
        g.add(canard(c.kit.carbon, cx, cy - 0.02, nb.min.z + h * 0.25, s));
      }
      g.add(new THREE.Mesh(new THREE.BoxGeometry(0.035, 0.03, 0.08), c.kit.red).translateX(w * 0.3).translateY(faceY(geo, w * 0.3, nb.min.z + h * 0.12, true, y0) + 0.01).translateZ(nb.min.z + h * 0.12));
    }
  } else {
    // Diffuser-style lower section with fins, reflector strips, a lip.
    const lowZ = nb.min.z + h * 0.2;
    g.add(intake(c, geo, 0, lowZ, w * 0.62, h * 0.24, false, y0));
    for (let k = -2; k <= 2; k++) {
      const x = k * w * 0.11;
      const y = faceY(geo, x, lowZ, false, y0);
      g.add(new THREE.Mesh(new THREE.BoxGeometry(0.012, 0.12, h * 0.26), c.kit.carbon).translateX(x).translateY(y - 0.03).translateZ(lowZ - 0.01));
    }
    for (const s of [-1, 1]) {
      const x = s * w * 0.4;
      const z = nb.min.z + h * 0.42;
      const y = faceY(geo, x, z, false, y0);
      const refl = lampMat(0x8a0b0b);
      g.add(new THREE.Mesh(new THREE.BoxGeometry(w * 0.12, 0.01, 0.025), refl).translateX(x).translateY(y - 0.004).translateZ(z));
      g.add(intake(c, geo, x, nb.min.z + h * 0.22, w * 0.1, h * 0.18, false, y0));
    }
    const blade = lipBlade(geo, c.kit.carbon, false, 0.04);
    if (blade) g.add(blade);
  }
  c.body(g, region, front ? "front_bumper" : "rear_bumper");
};

/** Front lip: a carbon blade following the stock bumper's lower edge, with end plates. */
const lip: Builder = (c, p) => {
  const geo = shellOf(c.frame, panelOf("FrontBumper"));
  if (!geo) return;
  const aggro = p.style === "aggressive";
  const blade = lipBlade(geo, c.kit.carbon, true, aggro ? 0.08 : 0.055, aggro ? 0.02 : 0.012);
  geo.dispose();
  if (!blade) return;
  const g = new THREE.Group();
  g.add(blade);
  c.body(g, "FrontBumper", "front_bumper");
};

/**
 * Carbon hood / trunk lid (`carbon` piece, `panel`: hood or trunk): the stock lid's shape in
 * clear-coated weave, replacing the painted lid, held down by a pair of chrome lid pins at the
 * leading edge (the race-car tell, so it never reads as a black painted lid).
 */
const carbon: Builder = (c, p) => {
  const region = p.panel === "trunk" ? "Trunk" : "Hood";
  const geo = shellOf(c.frame, panelOf(region));
  if (!geo) return;
  topUV(geo);
  c.hide(panelOf(region));
  const g = new THREE.Group();
  g.add(new THREE.Mesh(geo, carbonFibre()));
  // Stock plain-black carbon trim on the lid (the Shiro Turbo's hood cover, its wing) would sit
  // over the new weave and look stock: redraw it in the weave too.
  const trimOf = (m: MeshPartJson) => m.wheel == null && m.region === region && m.material === "carbon";
  const trim = shellOf(c.frame, trimOf);
  if (trim) {
    topUV(trim);
    c.hide(trimOf);
    g.add(new THREE.Mesh(trim, carbonFibre()));
  }
  geo.computeBoundingBox();
  const b = geo.boundingBox!;
  const w = b.max.x - b.min.x;
  // Pins sit near the lid's outer edge: the front of a hood, the back of a trunk lid.
  const edgeY = region === "Hood" ? b.max.y - 0.1 : b.min.y + 0.1;
  for (const sx of [-1, 1]) {
    const x = sx * w * 0.32;
    const z = topZ(geo, x, edgeY, b.max.z) + 0.006;
    const pin = new THREE.Mesh(new THREE.CylinderGeometry(0.022, 0.022, 0.012, 12), c.kit.chrome);
    pin.rotation.x = Math.PI / 2;
    pin.position.set(x, edgeY, z);
    g.add(pin);
    const clip = new THREE.Mesh(new THREE.TorusGeometry(0.02, 0.004, 6, 12, Math.PI), c.kit.chrome);
    clip.position.set(x, edgeY - 0.012, z + 0.004);
    g.add(clip);
  }
  c.body(g, region, region === "Hood" ? "hood" : "trunk_lid");
};

/** Highest point of `geo` within 6 cm of (x, y), else `fallback`. */
function topZ(geo: THREE.BufferGeometry, x: number, y: number, fallback: number): number {
  const p = geo.getAttribute("position");
  let best = -Infinity;
  for (let i = 0; i < p.count; i++) {
    if (Math.abs(p.getX(i) - x) < 0.06 && Math.abs(p.getY(i) - y) < 0.06) best = Math.max(best, p.getZ(i));
  }
  return Number.isFinite(best) ? best : fallback;
}

/** Replacement-panel builders by piece name. */
export const SHELL_BUILDERS: Record<string, Builder> = { bumper, lip, carbon };
