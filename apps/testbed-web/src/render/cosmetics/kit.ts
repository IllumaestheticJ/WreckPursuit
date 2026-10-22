// Materials and small geometry helpers shared by the cosmetic piece builders. Everything is made
// in code (boxes, tubes, lathes, canvas textures); no art files.
import * as THREE from "three";

// Double-sided: thin plates and open barrels are seen from both sides.
const std = (o: THREE.MeshStandardMaterialParameters) => new THREE.MeshStandardMaterial({ flatShading: true, side: THREE.DoubleSide, ...o });

/** Materials the same on every car (bolt-on parts that are never recoloured per car). */
export interface KitMats {
  chrome: THREE.MeshStandardMaterial;
  alloy: THREE.MeshStandardMaterial;
  black: THREE.MeshStandardMaterial;
  gunmetal: THREE.MeshStandardMaterial;
  carbon: THREE.MeshStandardMaterial;
  rubber: THREE.MeshStandardMaterial;
  steel: THREE.MeshStandardMaterial;
  red: THREE.MeshStandardMaterial;
  gold: THREE.MeshStandardMaterial;
  white: THREE.MeshStandardMaterial;
  blue: THREE.MeshStandardMaterial;
  titanium: THREE.MeshStandardMaterial;
  hole: THREE.MeshStandardMaterial;
  core: THREE.MeshStandardMaterial;
  cage: THREE.MeshStandardMaterial;
  fabric: THREE.MeshStandardMaterial;
  nos: THREE.MeshStandardMaterial;
}

let shared: KitMats | null = null;

/** The shared bolt-on materials (made on first use). */
export function kitMats(): KitMats {
  return (shared ??= {
    chrome: std({ color: 0xe6eaee, metalness: 1, roughness: 0.12 }),
    alloy: std({ color: 0xb8bdc2, metalness: 0.8, roughness: 0.32 }),
    black: std({ color: 0x15161a, metalness: 0.3, roughness: 0.6 }),
    gunmetal: std({ color: 0x3c4046, metalness: 0.7, roughness: 0.35 }),
    carbon: std({ color: 0x202326, metalness: 0.35, roughness: 0.3 }),
    rubber: std({ color: 0x121212, roughness: 0.95 }),
    steel: std({ color: 0x7c8186, metalness: 0.6, roughness: 0.45 }),
    red: std({ color: 0xc0161e, metalness: 0.2, roughness: 0.45 }),
    gold: std({ color: 0xd4a236, metalness: 0.85, roughness: 0.3 }),
    white: std({ color: 0xf1f1ee, roughness: 0.7 }),
    blue: std({ color: 0x1f5fbf, metalness: 0.4, roughness: 0.4 }),
    titanium: std({ color: 0x6d5fa8, metalness: 0.9, roughness: 0.25, emissive: 0x1a0f2a }),
    hole: std({ color: 0x050506, roughness: 1 }),
    core: std({ color: 0x9aa0a6, metalness: 0.7, roughness: 0.5, map: finTexture() }),
    cage: std({ color: 0x9a9fa5, metalness: 0.65, roughness: 0.4 }),
    fabric: std({ color: 0x1b1c20, roughness: 1 }),
    nos: std({ color: 0x2d7fd4, metalness: 0.5, roughness: 0.3 }),
  });
}

/** Radiator / intercooler fins: fine vertical lines. */
function finTexture(): THREE.CanvasTexture {
  const c = document.createElement("canvas");
  c.width = 64;
  c.height = 16;
  const g = c.getContext("2d")!;
  g.fillStyle = "#9aa0a6";
  g.fillRect(0, 0, 64, 16);
  g.fillStyle = "#4a4f55";
  for (let x = 0; x < 64; x += 4) g.fillRect(x, 0, 2, 16);
  const t = new THREE.CanvasTexture(c);
  t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.repeat.set(6, 1);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

/** A lamp material of one car (lit with the headlights through `emissiveIntensity`). */
export function lampMat(colour: number): THREE.MeshStandardMaterial {
  return std({ color: colour, emissive: colour, emissiveIntensity: 0, roughness: 0.3 });
}

/** A point (any object with x, y, z). */
export type P3 = { x: number; y: number; z: number };

const tmpA = new THREE.Vector3();
const tmpB = new THREE.Vector3();
const UP = new THREE.Vector3(0, 1, 0);

/** Box of size (x, y, z) centred at (cx, cy, cz). */
export function box(x: number, y: number, z: number, m: THREE.Material, cx = 0, cy = 0, cz = 0): THREE.Mesh {
  const b = new THREE.Mesh(new THREE.BoxGeometry(x, y, z), m);
  b.position.set(cx, cy, cz);
  return b;
}

/** Round tube of radius `r` from `a` to `b`. */
export function tube(a: P3, b: P3, r: number, m: THREE.Material, sides = 10): THREE.Mesh {
  const from = tmpA.set(a.x, a.y, a.z);
  const to = tmpB.set(b.x, b.y, b.z);
  const len = from.distanceTo(to);
  const t = new THREE.Mesh(new THREE.CylinderGeometry(r, r, len, sides), m);
  t.position.copy(from).add(to).multiplyScalar(0.5);
  t.quaternion.setFromUnitVectors(UP, to.sub(from).normalize());
  return t;
}

/** Tubes through a list of points (a bent bar). */
export function bentTube(points: P3[], r: number, m: THREE.Material): THREE.Group {
  const g = new THREE.Group();
  for (let i = 1; i < points.length; i++) g.add(tube(points[i - 1], points[i], r, m));
  // Ball joints hide the gaps at the bends.
  for (let i = 1; i < points.length - 1; i++) {
    const s = new THREE.Mesh(new THREE.SphereGeometry(r, 8, 6), m);
    s.position.set(points[i].x, points[i].y, points[i].z);
    g.add(s);
  }
  return g;
}

/** Cylinder of radius `r`, length `len` along the X axis (wheel axle direction), centred at `c`. */
export function cylX(r: number, len: number, m: THREE.Material, c: P3 = { x: 0, y: 0, z: 0 }, sides = 24): THREE.Mesh {
  const g = new THREE.CylinderGeometry(r, r, len, sides).rotateZ(Math.PI / 2);
  const mesh = new THREE.Mesh(g, m);
  mesh.position.set(c.x, c.y, c.z);
  return mesh;
}

/** Cylinder along the Y axis (pointing forward / back), centred at `c`. */
export function cylY(r: number, len: number, m: THREE.Material, c: P3, sides = 18): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.CylinderGeometry(r, r, len, sides), m);
  mesh.position.set(c.x, c.y, c.z);
  return mesh;
}

/** Flat ring in the wheel plane (normal along X), radii `r0` < `r1`, at x offset `x`. */
export function ringX(r0: number, r1: number, m: THREE.Material, x: number, segments = 40): THREE.Mesh {
  const g = new THREE.RingGeometry(r0, r1, segments).rotateY(Math.PI / 2);
  const mesh = new THREE.Mesh(g, m);
  mesh.position.x = x;
  return mesh;
}

/** A wedge (triangular prism along X): rises from `h0` at y = 0 to `h1` at y = `len`. */
export function wedge(width: number, len: number, h0: number, h1: number, m: THREE.Material): THREE.Mesh {
  const s = new THREE.Shape();
  s.moveTo(0, 0);
  s.lineTo(len, 0);
  s.lineTo(len, h1);
  s.lineTo(0, h0);
  s.closePath();
  const g = new THREE.ExtrudeGeometry(s, { depth: width, bevelEnabled: false });
  // Shape x → car y, shape y → car z, extrude z → car x (centred); a cyclic swap keeps the winding.
  g.translate(0, 0, -width / 2);
  g.applyMatrix4(new THREE.Matrix4().set(0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1));
  return new THREE.Mesh(g, m);
}

/** Mesh-grille texture (diamond wire) for guards and window nets. */
export function meshTexture(fg: string, cells: number, line = 3): THREE.CanvasTexture {
  const c = document.createElement("canvas");
  c.width = c.height = 128;
  const g = c.getContext("2d")!;
  g.clearRect(0, 0, 128, 128);
  g.strokeStyle = fg;
  g.lineWidth = line;
  const step = 128 / cells;
  for (let i = -cells; i <= cells * 2; i++) {
    g.beginPath();
    g.moveTo(i * step, 0);
    g.lineTo(i * step + 128, 128);
    g.stroke();
    g.beginPath();
    g.moveTo(i * step, 128);
    g.lineTo(i * step + 128, 0);
    g.stroke();
  }
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

/** Named colours of colour options (underglow, springs, seats). */
export const NAMED: Record<string, number> = {
  blue: 0x2a7bff,
  green: 0x22e06a,
  purple: 0xa63dff,
  red: 0xff2a3a,
  yellow: 0xf2c418,
  orange: 0xff7a1a,
  white: 0xf4f6ff,
  black: 0x18191c,
};
