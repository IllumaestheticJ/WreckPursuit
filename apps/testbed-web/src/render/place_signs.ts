// Landmark place details (lead, 2026-10-29; owner: "the dealership icons look nothing like
// dealerships"). The buildings themselves (showrooms, workshops, police stations, display cars,
// flags, sign panels) are greybox geometry with colliders (crates/wp_world/src/places.rs, drawn
// by tools/city-import). Two things are not geometry and are drawn here from
// `place_layouts_json()`: the text on each sign panel (made-up place names, or the generic word
// "POLICE") and the string flags of the used-car lots. Sim space (+Z up, metres); the caller
// adds `root` to the scene's sim-space world group.
import * as THREE from "three";

/** `wp_world::places::SignText`. */
export interface PlaceSign {
  text: string;
  centre: [number, number, number];
  normal: [number, number];
  size: [number, number];
  both_sides: boolean;
  depth_m: number;
}

/** `wp_world::places::PlaceLayout` (the fields drawn here). */
export interface PlaceLayout {
  id: string;
  name: string;
  style: "Dealership" | "UsedLot" | "Junkyard" | "RepairShop" | "PoliceStation" | "Safehouse" | "LockUp" | "TowerGarage" | "Barn";
  ring: [number, number, number];
  origin: [number, number];
  along: [number, number];
  out: [number, number];
  size: [number, number];
  floor_z: number;
  main: [number, number][];
  signs: PlaceSign[];
  bunting: [[number, number, number], [number, number, number]][];
}

/** Sign look per place style: panel background and text colour (CSS). */
const SIGN_LOOK: Record<PlaceLayout["style"], { bg: string; ink: string }> = {
  Dealership: { bg: "#10263f", ink: "#e9f4ff" },
  UsedLot: { bg: "#f2c21b", ink: "#14110a" },
  Junkyard: { bg: "#3a2416", ink: "#f2b53c" },
  RepairShop: { bg: "#1b1d22", ink: "#ff9a2e" },
  PoliceStation: { bg: "#0f2c7a", ink: "#ffffff" },
  Safehouse: { bg: "#111111", ink: "#cccccc" },
  LockUp: { bg: "#1d1f24", ink: "#f0e6c8" },
  TowerGarage: { bg: "#0d3b2c", ink: "#e8fff4" },
  Barn: { bg: "#e9e2d0", ink: "#6a1f18" },
};

/** Sign faces glow a little above white so they bloom at night (post.ts threshold ~1.1). */
const SIGN_GLOW = 1.35;
/** Pennant colours of the string flags. */
const PENNANTS = [0xd8312a, 0xf2f2f2, 0x2a6ad8, 0xf2c21b];

/** A canvas texture with `text` centred on `bg`, sized for a `w`×`h` m face. */
function signTexture(text: string, w: number, h: number, bg: string, ink: string): THREE.CanvasTexture {
  const px = 64; // pixels per metre of sign height
  const ch = Math.max(32, Math.round(h * px));
  const cw = Math.max(32, Math.min(2048, Math.round((w / h) * ch)));
  const c = document.createElement("canvas");
  c.width = cw;
  c.height = ch;
  const g = c.getContext("2d")!;
  g.fillStyle = bg;
  g.fillRect(0, 0, cw, ch);
  g.strokeStyle = ink;
  g.globalAlpha = 0.6;
  g.lineWidth = Math.max(2, ch * 0.05);
  g.strokeRect(g.lineWidth, g.lineWidth, cw - 2 * g.lineWidth, ch - 2 * g.lineWidth);
  g.globalAlpha = 1;
  // Fit the text: as tall as the face allows, narrower when long.
  let size = ch * 0.62;
  g.font = `bold ${size}px ui-sans-serif, "Helvetica Neue", Arial, sans-serif`;
  const room = cw * 0.9;
  const wide = g.measureText(text).width;
  if (wide > room) {
    size *= room / wide;
    g.font = `bold ${size}px ui-sans-serif, "Helvetica Neue", Arial, sans-serif`;
  }
  g.fillStyle = ink;
  g.textAlign = "center";
  g.textBaseline = "middle";
  g.fillText(text, cw / 2, ch / 2 + size * 0.04);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  return t;
}

/** Orientation of a face whose outward normal is `n` (horizontal), text upright. */
function faceQuaternion(n: [number, number]): THREE.Quaternion {
  const z = new THREE.Vector3(n[0], n[1], 0).normalize();
  const x = new THREE.Vector3(-z.y, z.x, 0);
  const y = new THREE.Vector3(0, 0, 1);
  return new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().makeBasis(x, y, z));
}

/** String flags: a sagging line of small triangular pennants between two pole tops. */
function buntingGeometry(strings: PlaceLayout["bunting"]): THREE.BufferGeometry | null {
  const pos: number[] = [];
  const col: number[] = [];
  const c = new THREE.Color();
  for (const [a, b] of strings) {
    const len = Math.hypot(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    const n = Math.max(2, Math.floor(len / 0.7));
    const sag = Math.min(0.6, len * 0.03);
    const at = (t: number) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t - sag * 4 * t * (1 - t)];
    for (let i = 0; i < n; i++) {
      const p0 = at(i / n);
      const p1 = at((i + 0.8) / n);
      const pm = at((i + 0.4) / n);
      pos.push(...p0, ...p1, pm[0], pm[1], pm[2] - 0.38);
      c.setHex(PENNANTS[i % PENNANTS.length]);
      for (let k = 0; k < 3; k++) col.push(c.r, c.g, c.b);
    }
  }
  if (!pos.length) return null;
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute("color", new THREE.Float32BufferAttribute(col, 3));
  return g;
}

/** Sign text and string flags of every landmark place of the session's city. */
export class PlaceSigns {
  readonly root = new THREE.Group();
  /** Places drawn (automation). */
  layouts: PlaceLayout[] = [];

  constructor() {
    this.root.name = "place-signs";
  }

  /** New session: draw `layouts` (empty clears). */
  set(layouts: PlaceLayout[]): void {
    this.clear();
    this.layouts = layouts;
    const strings: PlaceLayout["bunting"] = [];
    for (const l of layouts) {
      const look = SIGN_LOOK[l.style] ?? SIGN_LOOK.LockUp;
      for (const s of l.signs) {
        const tex = signTexture(s.text, s.size[0], s.size[1], look.bg, look.ink);
        const mat = new THREE.MeshBasicMaterial({ map: tex });
        mat.color.setScalar(SIGN_GLOW);
        const geo = new THREE.PlaneGeometry(s.size[0], s.size[1]);
        const faces: { n: [number, number]; off: number }[] = [{ n: s.normal, off: 0.03 }];
        if (s.both_sides) faces.push({ n: [-s.normal[0], -s.normal[1]], off: s.depth_m + 0.03 });
        for (const f of faces) {
          const m = new THREE.Mesh(geo, mat);
          const sign = f === faces[0] ? 1 : -1;
          m.position.set(s.centre[0] + s.normal[0] * f.off * sign, s.centre[1] + s.normal[1] * f.off * sign, s.centre[2]);
          m.quaternion.copy(faceQuaternion(f.n));
          m.name = `place-sign:${l.id}`;
          this.root.add(m);
        }
      }
      strings.push(...l.bunting);
    }
    const g = buntingGeometry(strings);
    if (g) {
      const m = new THREE.Mesh(g, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide }));
      m.name = "place-bunting";
      this.root.add(m);
    }
  }

  /** Remove everything (disposes GPU resources). */
  clear(): void {
    const seen = new Set<THREE.Material | THREE.BufferGeometry>();
    this.root.traverse((o) => {
      const m = o as THREE.Mesh;
      if (!m.isMesh) return;
      for (const r of [m.geometry, m.material as THREE.Material]) {
        if (seen.has(r)) continue;
        seen.add(r);
        if (r instanceof THREE.MeshBasicMaterial) r.map?.dispose();
        r.dispose();
      }
    });
    this.root.clear();
    this.layouts = [];
  }
}
