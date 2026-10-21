// Mod parts on the car: what a build's mods bolt on (wp_economy::visuals, via
// `entity_visuals_json`) drawn as procedural low-poly pieces on the car's panel mesh.
//
// Every body piece (wing, splitter, cage, bars, skirts, scoops, …) is fitted to the car's real
// surfaces by ray-probing the rest-pose panel mesh, then handed to PanelBody as an ordinary part
// skinned to the nearest lattice nodes of the region it is mounted on (PartVisual.regions). So a
// piece dents with the panel under it, swings with a hinged trunk lid, and leaves the car as
// debris when its region detaches: a rear hit that tears the trunk off takes the wing with it.
// Pieces join PanelBody's per-material batches (one extra draw per mod material) and its
// allocation-free re-skin; all the fitting below runs once, when the car spawns.
//
// Wheel pieces (rims, tread, brake discs) are hub-space parts that spin with the wheel; brake
// calipers are hub parts that steer but do not spin. Material-only parts (paint finish, carbon
// panels, tint, interior trim) change PanelBody's materials (`setModLook`: kept over art style
// changes; shared materials are copied for this car first); smoked lamps are returned for
// VehicleView to apply to its lamp materials; vinyls are an overlay drawing the paint batch's
// (skinned) geometry again.
//
// Coordinates: chassis space (+X right, +Y forward, +Z up), metres.
import * as THREE from "three";
import type { CarMeshJson, MeshPartJson, PanelBody, PartMaterial } from "./panels";
import type { WheelInfo } from "../wasm/sim";

/** `wp_economy::PartVisual`. */
export interface PartVisual {
  part: string;
  style: string;
  mod_id: string;
  regions: string[];
}

/** `wp_economy::WheelVisual` (FL, FR, RL, RR). */
export interface WheelVisual {
  base_radius_m: number;
  radius_m: number;
  base_width_m: number;
  width_m: number;
  base_tire_id: string;
  tire_id: string;
}

/** `wp_economy::BuildVisuals` (`entity_visuals_json` / `build_visuals_json`). */
export interface BuildVisuals {
  car_id: string;
  mods: string[];
  parts: PartVisual[];
  wheels: WheelVisual[];
  ride_height_m: number;
}

/** What `fitBuild` put on the car (tests / HUD). */
export interface FittedBuild {
  /** Mesh names of the body pieces added, per part kind. */
  pieces: Record<string, string[]>;
  /** Part kinds drawn (body pieces, wheel pieces and material changes). */
  drawn: string[];
  /** Smoked lamp covers: head / tail lamp colours VehicleView keeps over art style changes. */
  lamps: { head: number; tail: number } | null;
}

// ---------------------------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------------------------

const V = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);
const UP = V(0, 1, 0);

/** Triangle soup for one piece (positions in metres + indices). */
class Geo {
  readonly pos: number[] = [];
  readonly idx: number[] = [];

  /** Append a Three geometry, transformed by `m` (disposed afterwards). */
  add(g: THREE.BufferGeometry, m?: THREE.Matrix4): this {
    if (m) g.applyMatrix4(m);
    const p = g.getAttribute("position");
    const base = this.pos.length / 3;
    for (let i = 0; i < p.count; i++) this.pos.push(p.getX(i), p.getY(i), p.getZ(i));
    if (g.index) for (let i = 0; i < g.index.count; i++) this.idx.push(base + g.index.getX(i));
    else for (let i = 0; i < p.count; i++) this.idx.push(base + i);
    g.dispose();
    return this;
  }

  /** Axis-aligned box centred at `c`, optionally rotated by Euler `rot` (about its centre). */
  box(c: THREE.Vector3, size: [number, number, number], rot?: THREE.Euler): this {
    const m = new THREE.Matrix4().compose(c, new THREE.Quaternion().setFromEuler(rot ?? new THREE.Euler()), V(1, 1, 1));
    return this.add(new THREE.BoxGeometry(...size), m);
  }

  /** Round tube from `a` to `b`. */
  tube(a: THREE.Vector3, b: THREE.Vector3, r: number, seg = 8): this {
    const d = b.clone().sub(a);
    const len = d.length();
    if (len < 1e-4) return this;
    const q = new THREE.Quaternion().setFromUnitVectors(UP, d.normalize());
    const m = new THREE.Matrix4().compose(a.clone().add(b).multiplyScalar(0.5), q, V(1, 1, 1));
    return this.add(new THREE.CylinderGeometry(r, r, len, seg, 1, false), m);
  }

  /** Polyline of tubes. */
  path(pts: THREE.Vector3[], r: number): this {
    for (let i = 0; i + 1 < pts.length; i++) this.tube(pts[i], pts[i + 1], r);
    return this;
  }

  /** Side profile (y, z points, counter-clockwise) extruded across x from x0 to x1. */
  profileX(pts: [number, number][], x0: number, x1: number): this {
    const shape = new THREE.Shape(pts.map(([y, z]) => new THREE.Vector2(y, z)));
    const g = new THREE.ExtrudeGeometry(shape, { depth: x1 - x0, bevelEnabled: false });
    // Shape x → chassis y, shape y → chassis z, extrusion (+z) → chassis x.
    const m = new THREE.Matrix4().makeBasis(V(0, 1, 0), V(0, 0, 1), V(1, 0, 0)).setPosition(x0, 0, 0);
    return this.add(g, m);
  }

  get empty(): boolean {
    return this.idx.length === 0;
  }
}

/** One body piece before skinning. */
interface Piece {
  name: string;
  material: PartMaterial;
  geo: Geo;
  /** Regions it may bind to (the part's regions); the nearest one is used. */
  regions: string[];
}

// ---------------------------------------------------------------------------------------------
// Body probe: the rest-pose panel mesh, ray-cast to place pieces on real surfaces
// ---------------------------------------------------------------------------------------------

interface Box {
  min: THREE.Vector3;
  max: THREE.Vector3;
}

class BodyProbe {
  private readonly shell: THREE.Mesh;
  private readonly ray = new THREE.Raycaster();
  /** Bounds of the body (paint + trim parts), metres. */
  readonly all: Box;
  private readonly regionBoxes = new Map<string, Box>();

  constructor(mesh: CarMeshJson) {
    const pos: number[] = [];
    const idx: number[] = [];
    const all = new THREE.Box3();
    for (const p of mesh.parts) {
      if (p.wheel != null) continue;
      const solid = p.kind === "panel" || p.kind === "body" || p.kind === "light" || p.kind === "glass";
      if (!solid) continue;
      const base = pos.length / 3;
      const box = new THREE.Box3();
      for (let i = 0; i < p.positions_mm.length; i += 3) {
        const v = V(p.positions_mm[i] * 1e-3, p.positions_mm[i + 1] * 1e-3, p.positions_mm[i + 2] * 1e-3);
        pos.push(v.x, v.y, v.z);
        box.expandByPoint(v);
      }
      for (const i of p.indices) idx.push(base + i);
      if (p.kind !== "glass") all.union(box);
      if (p.region && (p.kind === "panel" || p.kind === "body")) {
        const b = this.regionBoxes.get(p.region);
        if (b) {
          b.min.min(box.min);
          b.max.max(box.max);
        } else this.regionBoxes.set(p.region, { min: box.min.clone(), max: box.max.clone() });
      }
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
    g.setIndex(idx);
    this.shell = new THREE.Mesh(g, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));
    this.shell.updateMatrixWorld(true);
    this.all = { min: all.min.clone(), max: all.max.clone() };
  }

  /** Bounds of a region's panels, or `fallback` (a fraction box of the whole body). */
  region(name: string, fallback: [number, number, number, number, number, number]): Box {
    const b = this.regionBoxes.get(name);
    if (b) return b;
    const { min, max } = this.all;
    const at = (a: number, lo: number, hi: number) => lo + (hi - lo) * a;
    return {
      min: V(at(fallback[0], min.x, max.x), at(fallback[1], min.y, max.y), at(fallback[2], min.z, max.z)),
      max: V(at(fallback[3], min.x, max.x), at(fallback[4], min.y, max.y), at(fallback[5], min.z, max.z)),
    };
  }

  private cast(origin: THREE.Vector3, dir: THREE.Vector3): number | null {
    this.ray.set(origin, dir);
    const hit = this.ray.intersectObject(this.shell, false)[0];
    return hit ? hit.distance : null;
  }

  /** Height of the top surface at (x, y), or `fallback`. */
  top(x: number, y: number, fallback = this.all.max.z): number {
    const d = this.cast(V(x, y, this.all.max.z + 2), V(0, 0, -1));
    return d === null ? fallback : this.all.max.z + 2 - d;
  }

  /** Outer side surface |x| at (y, z) on side `sign` (−1 left, +1 right). */
  side(sign: number, y: number, z: number): number {
    const x0 = this.all.max.x + 2;
    const d = this.cast(V(sign * x0, y, z), V(-sign, 0, 0));
    return d === null ? this.all.max.x : x0 - d;
  }

  /** Front surface y at (x, z). */
  front(x: number, z: number): number {
    const y0 = this.all.max.y + 2;
    const d = this.cast(V(x, y0, z), V(0, -1, 0));
    return d === null ? this.all.max.y : y0 - d;
  }

  /** Rear surface y at (x, z). */
  rear(x: number, z: number): number {
    const y0 = this.all.min.y - 2;
    const d = this.cast(V(x, y0, z), V(0, 1, 0));
    return d === null ? this.all.min.y : y0 + d;
  }

  /** Surface slope dz/dy of the top at (x, y). */
  slope(x: number, y: number): number {
    return (this.top(x, y + 0.05) - this.top(x, y - 0.05)) / 0.1;
  }

  dispose(): void {
    this.shell.geometry.dispose();
  }
}

// ---------------------------------------------------------------------------------------------
// Body pieces, one builder per part kind
// ---------------------------------------------------------------------------------------------

interface Ctx {
  b: BodyProbe;
  wheels: WheelInfo[];
  /** Body half width, m. */
  hw: number;
  /** Lowest body point (sill / underbody), m. */
  floor: number;
  front: Box;
  rear: Box;
  hood: Box;
  trunk: Box;
  roof: Box;
}

type Builder = (c: Ctx, style: string) => { name: string; material: PartMaterial; geo: Geo; side?: number }[];

const SIDES = [-1, 1] as const;

const BUILDERS: Record<string, Builder> = {
  FrontSplitter: ({ b, hw, front }, style) => {
    const yF = b.front(0, front.min.z + 0.05);
    const z = front.min.z - 0.012;
    if (style === "lowdrag") {
      // Smooth paint-matched lip that rounds off the bumper's lower edge.
      const lip = new Geo().profileX([[yF - 0.12, z], [yF + 0.035, z], [yF + 0.01, z + 0.06], [yF - 0.12, z + 0.06]], -hw * 0.9, hw * 0.9);
      return [{ name: "splitter_lowdrag", material: "paint", geo: lip }];
    }
    const g = new Geo().box(V(0, yF - 0.1, z), [hw * 1.84, 0.34, 0.018]);
    for (const s of SIDES) g.box(V(s * hw * 0.9, yF - 0.06, z + 0.045), [0.012, 0.24, 0.09]);
    return [{ name: "splitter", material: "carbon", geo: g }];
  },

  RearWing: ({ b, hw, trunk }, style) => {
    const gt = style === "gt";
    const y = trunk.min.y + Math.min(0.16, (trunk.max.y - trunk.min.y) * 0.25);
    const span = Math.min(hw * 2 * (gt ? 0.96 : 0.84), 2.2);
    const chord = gt ? 0.3 : 0.2;
    const rise = gt ? 0.3 : 0.11;
    const ux = hw * (gt ? 0.42 : 0.55);
    const base = Math.max(b.top(-ux, y), b.top(ux, y));
    const zb = base + rise;
    // Cambered blade, trailing edge up (downforce).
    const t = chord * 0.12;
    const blade = new Geo().profileX(
      [
        [y + chord * 0.5, zb],
        [y + chord * 0.2, zb - t * 0.6],
        [y - chord * 0.2, zb - t * 0.2],
        [y - chord * 0.5, zb + chord * 0.16],
        [y - chord * 0.45, zb + chord * 0.16 + t * 0.4],
        [y - chord * 0.1, zb + t * 0.9],
        [y + chord * 0.3, zb + t * 0.9],
      ],
      -span / 2,
      span / 2,
    );
    const mounts = new Geo();
    for (const s of SIDES) {
      const zs = b.top(s * ux, y);
      mounts.box(V(s * ux, y + 0.02, (zs + zb) / 2), [0.02, gt ? 0.12 : 0.09, zb - zs + 0.02]);
      if (gt) blade.box(V((s * span) / 2, y - 0.02, zb + 0.03), [0.012, chord + 0.06, 0.15]);
    }
    return [
      { name: `wing_${style}`, material: gt ? "carbon" : "paint", geo: blade },
      { name: `wing_${style}_mounts`, material: gt ? "carbon" : "steel", geo: mounts },
    ];
  },

  Diffuser: ({ b, hw, rear }) => {
    const yR = b.rear(0, rear.min.z + 0.05);
    const z = rear.min.z - 0.004;
    const len = 0.36;
    // Ramp rising toward the back, with vertical strakes.
    const g = new Geo().profileX([[yR + len, z], [yR - 0.03, z + 0.07], [yR - 0.03, z + 0.085], [yR + len, z + 0.015]], -hw * 0.75, hw * 0.75);
    for (const x of [-0.5, -0.17, 0.17, 0.5]) g.profileX([[yR + len, z], [yR - 0.03, z + 0.07], [yR - 0.03, z - 0.02]], x * hw - 0.006, x * hw + 0.006);
    return [{ name: "diffuser", material: "carbon", geo: g }];
  },

  FrontBumperKit: ({ b, hw, front }, style) => {
    const z0 = front.min.z;
    const yF = b.front(0, z0 + 0.08);
    const out: ReturnType<Builder> = [];
    if (style === "lip_a" || style === "aggro_a" || style === "wide_a") {
      const lip = new Geo().profileX([[yF - 0.08, z0 - 0.03], [yF + 0.03, z0 - 0.03], [yF + 0.01, z0], [yF - 0.08, z0]], -hw * 0.88, hw * 0.88);
      out.push({ name: `front_${style}_lip`, material: style === "lip_a" ? "trim" : "paint", geo: lip });
    }
    if (style === "wide_a") {
      for (const s of SIDES) {
        const x = s * (hw + 0.012);
        const g = new Geo().box(V(x, front.min.y + (yF - front.min.y) * 0.45, z0 + 0.12), [0.045, (yF - front.min.y) * 0.9, 0.22]);
        out.push({ name: `front_wide_${s < 0 ? "l" : "r"}`, material: "paint", geo: g, side: s });
      }
    }
    if (style === "aggro_a") {
      const g = new Geo().box(V(0, b.front(0, z0 + 0.12) + 0.006, z0 + 0.12), [hw * 0.9, 0.02, 0.12]);
      for (const s of SIDES) {
        g.box(V(s * hw * 0.72, b.front(s * hw * 0.72, z0 + 0.12) + 0.006, z0 + 0.12), [hw * 0.28, 0.02, 0.1]);
        // Canards on the bumper corners.
        for (const dz of [0.2, 0.28]) {
          const x = s * hw * 0.93;
          g.box(V(x + s * 0.03, b.front(x, z0 + dz) - 0.03, z0 + dz), [0.08, 0.1, 0.008], new THREE.Euler(0.25, 0, -s * 0.15));
        }
      }
      out.push({ name: "front_aggro_vents", material: "trim", geo: g });
    }
    return out;
  },

  SideKit: ({ b, wheels, floor, hw }, style) => {
    const out: ReturnType<Builder> = [];
    const fy = Math.max(wheels[0].position_m[1], wheels[1].position_m[1]);
    const ry = Math.min(wheels[2].position_m[1], wheels[3].position_m[1]);
    const r = Math.max(...wheels.map((w) => w.radius_m));
    const y0 = ry + r + 0.06;
    const y1 = fy - r - 0.06;
    for (const s of SIDES) {
      const tag = s < 0 ? "l" : "r";
      if (y1 > y0) {
        const x = b.side(s, (y0 + y1) / 2, floor + 0.08);
        const skirt = new Geo().box(V(s * (x + 0.012), (y0 + y1) / 2, floor + 0.035), [0.045, y1 - y0, 0.11]);
        out.push({ name: `skirt_${tag}`, material: style === "wide_a" ? "paint" : "trim", geo: skirt, side: s });
      }
      if (style !== "wide_a") continue;
      // Bolt-on arch flares round each wheel on this side.
      for (const w of wheels.filter((w) => Math.sign(w.position_m[0]) === s)) {
        const [, wy, wz] = w.position_m;
        const x = Math.max(b.side(s, wy, wz + w.radius_m * 0.7), hw - 0.06);
        const flare = new THREE.TorusGeometry(w.radius_m + 0.07, 0.04, 4, 14, Math.PI * 1.1);
        flare.rotateZ(Math.PI / 2 - Math.PI * 0.05);
        flare.rotateY(Math.PI / 2);
        flare.scale(1.4, 1, 1);
        const g = new Geo().add(flare, new THREE.Matrix4().makeTranslation(s * (x + 0.02), wy, wz));
        out.push({ name: `flare_${w.position_m[1] > 0 ? "f" : "r"}${tag}`, material: "paint", geo: g, side: s });
      }
    }
    return out;
  },

  RearBumperKit: ({ b, hw, rear }, style) => {
    const z0 = rear.min.z;
    const yR = b.rear(0, z0 + 0.08);
    const g = new Geo().profileX([[yR + 0.08, z0 - 0.025], [yR - 0.025, z0 - 0.025], [yR - 0.01, z0], [yR + 0.08, z0]], -hw * 0.86, hw * 0.86);
    for (const x of [-0.3, 0, 0.3]) g.box(V(x * hw, yR - 0.012, z0 - 0.005), [0.012, 0.05, 0.06]);
    const out: ReturnType<Builder> = [{ name: `rear_${style}_valance`, material: style === "wide_a" ? "paint" : "trim", geo: g }];
    if (style === "wide_a") {
      for (const s of SIDES) {
        const len = rear.max.y - yR;
        out.push({ name: `rear_wide_${s < 0 ? "l" : "r"}`, material: "paint", geo: new Geo().box(V(s * (hw + 0.012), yR + len * 0.55, z0 + 0.12), [0.045, len * 0.9, 0.22]), side: s });
      }
    }
    return out;
  },

  HoodStyle: ({ b, hw, hood }, style) => {
    const len = hood.max.y - hood.min.y;
    if (style === "scoop") {
      const y = hood.min.y + len * 0.45;
      const z = b.top(0, y);
      const g = new Geo().profileX([[y + 0.25, z - 0.01], [y - 0.22, z - 0.01], [y - 0.22, z + 0.085], [y - 0.16, z + 0.09]], -0.22, 0.22);
      const mouth = new Geo().box(V(0, y - 0.16, z + 0.05), [0.36, 0.02, 0.06]);
      return [
        { name: "hood_scoop", material: "paint", geo: g },
        { name: "hood_scoop_mouth", material: "trim", geo: mouth },
      ];
    }
    // Two banks of louvres near the front of the hood.
    const g = new Geo();
    for (const s of SIDES) {
      const x = s * hw * 0.38;
      for (let i = 0; i < 5; i++) {
        const y = hood.min.y + len * (0.5 + i * 0.055);
        const pitch = Math.atan(b.slope(x, y));
        g.box(V(x, y, b.top(x, y) + 0.006), [0.34, 0.025, 0.012], new THREE.Euler(-pitch + 0.35, 0, 0));
      }
    }
    return [{ name: "hood_vents", material: "trim", geo: g }];
  },

  RoofPiece: ({ b, roof }, style) => {
    const len = roof.max.y - roof.min.y;
    const half = Math.max((roof.max.x - roof.min.x) / 2, 0.3);
    if (style === "scoop") {
      const y = roof.max.y - len * 0.25;
      const z = b.top(0, y);
      const g = new Geo().profileX([[y + 0.2, z - 0.01], [y - 0.12, z - 0.01], [y - 0.12, z + 0.08], [y - 0.06, z + 0.085]], -0.14, 0.14);
      return [
        { name: "roof_scoop", material: "paint", geo: g },
        { name: "roof_scoop_mouth", material: "trim", geo: new Geo().box(V(0, y - 0.06, z + 0.045), [0.22, 0.02, 0.05]) },
      ];
    }
    const g = new Geo();
    const zr = (x: number, y: number) => b.top(x, y) + 0.07;
    const ys = [roof.min.y + len * 0.2, roof.max.y - len * 0.2];
    for (const s of SIDES) {
      const x = s * half * 0.78;
      g.tube(V(x, ys[0] - 0.08, zr(x, ys[0])), V(x, ys[1] + 0.08, zr(x, ys[1])), 0.014);
      for (const y of ys) g.box(V(x, y, (b.top(x, y) + zr(x, y)) / 2), [0.04, 0.06, zr(x, y) - b.top(x, y)]);
    }
    for (const y of ys) g.tube(V(-half * 0.85, y, zr(0, y)), V(half * 0.85, y, zr(0, y)), 0.016);
    return [{ name: "roof_rack", material: "steel", geo: g }];
  },

  RollCage: ({ b, roof, floor }, style) => {
    const welded = style === "welded";
    const r = welded ? 0.024 : 0.02;
    const ws = b.region("Windshield", [0.1, 0.55, 0.55, 0.9, 0.75, 0.95]);
    const half = Math.max((roof.max.x - roof.min.x) / 2, 0.35);
    const xc = half * 0.82;
    const zf = floor + 0.12;
    const zt = (x: number, y: number) => b.top(x, y, roof.max.z) - 0.07;
    const ym = (roof.min.y + roof.max.y) / 2 - (roof.max.y - roof.min.y) * 0.1;
    const yA = roof.max.y - 0.04;
    const yDash = ws.max.y - 0.12;
    const yRear = roof.min.y - 0.25;
    const g = new Geo();
    for (const s of SIDES) {
      const x = s * xc;
      // Main hoop leg, A-pillar bar to the dash and down to the floor, roof bar, rear stay.
      g.path([V(x, ym, zf), V(x, ym, zt(x, ym))], r);
      g.path([V(x, yDash, zf), V(x, yDash, ws.min.z + 0.02), V(x, yA, zt(x, yA))], r);
      g.tube(V(x, yA, zt(x, yA)), V(x, ym, zt(x, ym)), r);
      g.tube(V(x, ym, zt(x, ym)), V(x * 0.9, yRear, zf + 0.15), r);
      if (welded) {
        // Door bars (X) and a dash bar.
        const zm = zf + 0.22;
        g.tube(V(x * 1.03, yDash, zf + 0.05), V(x * 1.03, ym, zm + 0.12), r * 0.9);
        g.tube(V(x * 1.03, yDash, zm + 0.12), V(x * 1.03, ym, zf + 0.05), r * 0.9);
      }
    }
    g.tube(V(-xc, ym, zt(-xc, ym)), V(xc, ym, zt(xc, ym)), r);
    g.tube(V(-xc, yA, zt(-xc, yA)), V(xc, yA, zt(xc, yA)), r);
    if (welded) {
      g.tube(V(-xc, ym, zt(-xc, ym)), V(xc, ym, zf), r);
      g.tube(V(-xc, yDash, ws.min.z + 0.02), V(xc, yDash, ws.min.z + 0.02), r);
      g.tube(V(-xc, yA, zt(-xc, yA)), V(xc, ym, zt(xc, ym)), r * 0.9);
    }
    return [{ name: `cage_${style}`, material: "metal", geo: g }];
  },

  BullBar: ({ b, hw, front }) => {
    const z0 = front.min.z + 0.1;
    const z1 = Math.min(front.min.z + 0.32, front.max.z - 0.05);
    const x = hw * 0.72;
    const y = Math.max(b.front(0, z0), b.front(x, z0)) + 0.06;
    const g = new Geo().tube(V(-x, y, z0), V(x, y, z0), 0.03).tube(V(-x * 0.9, y, z1), V(x * 0.9, y, z1), 0.026);
    for (const s of SIDES) {
      g.tube(V(s * x * 0.55, y, z0), V(s * x * 0.55, y, z1), 0.024);
      g.tube(V(s * x * 0.55, y, z0), V(s * x * 0.55, b.front(s * x * 0.55, z0) - 0.03, z0), 0.026);
    }
    return [{ name: "bull_bar", material: "steel", geo: g }];
  },

  RearBar: ({ b, hw, rear }) => {
    const z = rear.min.z + 0.16;
    const x = hw * 0.7;
    const y = Math.min(b.rear(0, z), b.rear(x, z)) - 0.05;
    const g = new Geo().tube(V(-x, y, z), V(x, y, z), 0.028);
    for (const s of SIDES) g.tube(V(s * x * 0.6, y, z), V(s * x * 0.6, b.rear(s * x * 0.6, z) + 0.03, z), 0.024);
    return [{ name: "rear_bar", material: "steel", geo: g }];
  },

  PushBar: ({ b, hw, front }) => {
    const z0 = front.min.z + 0.08;
    const z1 = front.max.z + 0.12;
    const xr = hw * 0.32;
    const y = b.front(0, (z0 + z1) / 2) + 0.12;
    const g = new Geo();
    for (const s of SIDES) {
      g.box(V(s * xr, y, (z0 + z1) / 2), [0.05, 0.05, z1 - z0]);
      g.box(V(s * xr, y + 0.035, (z0 + z1) / 2), [0.06, 0.025, (z1 - z0) * 0.8]);
      g.box(V(s * xr, (y + b.front(s * xr, z0 + 0.1)) / 2, z0 + 0.1), [0.04, y - b.front(s * xr, z0 + 0.1) + 0.04, 0.05]);
    }
    for (const t of [0.2, 0.55, 0.95]) g.box(V(0, y, z0 + (z1 - z0) * t), [xr * 2 + 0.05, 0.04, 0.035]);
    return [{ name: "push_bar", material: "steel", geo: g }];
  },

  SkidPlate: ({ b, hw, floor, wheels, front }) => {
    const y0 = Math.max(wheels[0].position_m[1], wheels[1].position_m[1]) - 0.3;
    const y1 = b.front(0, front.min.z + 0.05) - 0.12;
    const g = new Geo().box(V(0, (y0 + y1) / 2, floor - 0.012), [hw * 1.2, Math.max(y1 - y0, 0.2), 0.012]);
    return [{ name: "skid_plate", material: "metal", geo: g }];
  },

  RadiatorGuard: ({ b, hw, front }) => {
    const za = front.min.z + 0.08;
    const zb = Math.min(front.min.z + 0.3, front.max.z - 0.05);
    const xa = hw * 0.36;
    const y = b.front(0, (za + zb) / 2) + 0.012;
    const g = new Geo();
    for (let i = 0; i <= 8; i++) g.box(V(-xa + (2 * xa * i) / 8, y, (za + zb) / 2), [0.008, 0.008, zb - za]);
    for (let i = 0; i <= 3; i++) g.box(V(0, y + 0.004, za + ((zb - za) * i) / 3), [2 * xa, 0.008, 0.008]);
    return [{ name: "radiator_guard", material: "steel", geo: g }];
  },

  SideArmor: ({ b }) => {
    const out: ReturnType<Builder> = [];
    for (const [region, s] of [["LeftDoorF", -1], ["LeftDoorR", -1], ["RightDoorF", 1], ["RightDoorR", 1]] as const) {
      const d = b.region(region, s < 0 ? [0, 0.35, 0.25, 0.05, 0.6, 0.6] : [0.95, 0.35, 0.25, 1, 0.6, 0.6]);
      const y = (d.min.y + d.max.y) / 2;
      const ly = (d.max.y - d.min.y) * 0.84;
      const z0 = d.min.z + 0.04;
      const z1 = z0 + (d.max.z - d.min.z) * 0.62;
      const x = b.side(s, y, (z0 + z1) / 2) + 0.012;
      const g = new Geo().box(V(s * x, y, (z0 + z1) / 2), [0.012, ly, z1 - z0]);
      for (const fy of [-0.42, 0, 0.42])
        for (const fz of [0.12, 0.88]) {
          const bolt = new THREE.CylinderGeometry(0.012, 0.012, 0.012, 6).rotateZ(Math.PI / 2);
          g.add(bolt, new THREE.Matrix4().makeTranslation(s * (x + 0.01), y + fy * ly, z0 + (z1 - z0) * fz));
        }
      out.push({ name: `armor_${region}`, material: "steel", geo: g, side: s });
    }
    return out;
  },

  Exhaust: ({ b, hw, rear }, style) => {
    const race = style === "race";
    const r = race ? 0.055 : 0.04;
    const z = rear.min.z + 0.06;
    const xs = race ? [-hw * 0.55, hw * 0.55] : [hw * 0.5, hw * 0.5 + 0.11];
    const g = new Geo();
    const caps = new Geo();
    for (const x of xs) {
      const y1 = b.rear(x, z + 0.06) - 0.04;
      const tip = new THREE.CylinderGeometry(r, r * 0.92, 0.16, 12, 1, true);
      g.add(tip, new THREE.Matrix4().compose(V(x, y1 + 0.08, z), new THREE.Quaternion().setFromUnitVectors(UP, V(0, -1, 0)), V(1, 1, 1)));
      caps.add(new THREE.CircleGeometry(r * 0.85, 12).rotateX(Math.PI / 2), new THREE.Matrix4().makeTranslation(x, y1 + 0.03, z));
    }
    return [
      { name: `exhaust_${style}`, material: "chrome", geo: g },
      { name: `exhaust_${style}_soot`, material: "trim", geo: caps },
    ];
  },

  Intercooler: ({ b, hw, front }) => {
    const za = front.min.z + 0.06;
    const zb = Math.min(front.min.z + 0.24, front.max.z - 0.08);
    const xa = hw * 0.55;
    const y = b.front(0, (za + zb) / 2) + 0.006;
    const core = new Geo().box(V(0, y, (za + zb) / 2), [2 * xa, 0.03, zb - za]);
    const fins = new Geo();
    for (let i = 1; i < 6; i++) fins.box(V(0, y + 0.017, za + ((zb - za) * i) / 6), [2 * xa - 0.02, 0.006, 0.008]);
    return [
      { name: "intercooler", material: "metal", geo: core },
      { name: "intercooler_fins", material: "trim", geo: fins },
    ];
  },

  Supercharger: ({ b, hood }, style) => {
    const tall = style === "blower_2";
    const y = hood.min.y + (hood.max.y - hood.min.y) * 0.42;
    const z = b.top(0, y);
    const h = tall ? 0.2 : 0.13;
    const body = new Geo().box(V(0, y, z + h / 2 - 0.02), [0.3, 0.4, h + 0.04]);
    const ribs = new Geo();
    for (let i = 0; i < 4; i++) ribs.box(V(0, y - 0.15 + i * 0.1, z + h + 0.005), [0.32, 0.02, 0.02]);
    const hat = new Geo();
    if (tall)
      for (const x of [-0.08, 0.08]) hat.tube(V(x, y + 0.05, z + h), V(x, y + 0.05, z + h + 0.1), 0.06, 12);
    else hat.profileX([[y + 0.16, z + h], [y - 0.16, z + h], [y - 0.16, z + h + 0.08], [y + 0.04, z + h + 0.08]], -0.13, 0.13);
    return [
      { name: `blower_${style}`, material: "metal", geo: body },
      { name: `blower_${style}_ribs`, material: "chrome", geo: ribs },
      { name: `blower_${style}_hat`, material: tall ? "chrome" : "trim", geo: hat },
    ];
  },
};

// ---------------------------------------------------------------------------------------------
// Wheel pieces (hub space: axle along X, wheel centre at the origin)
// ---------------------------------------------------------------------------------------------

/** Outer radius of the stock rim part (where the tire's sidewall opening is), m. */
function stockRimRadius(mesh: CarMeshJson, wheel: number): number {
  const rim = mesh.parts.find((p) => p.wheel === wheel && p.kind === "rim");
  if (!rim) return 0;
  let r = 0;
  for (let i = 0; i < rim.positions_mm.length; i += 3) r = Math.max(r, Math.hypot(rim.positions_mm[i + 1], rim.positions_mm[i + 2]));
  return r * 1e-3;
}

/** A disc of `n` radial spokes / a full face, with a barrel lip, facing side `s` at x. */
function rimGeo(style: string, rimR: number, s: number, width: number): { geo: Geo; material: PartMaterial }[] {
  const face = s * (width * 0.5 - 0.035);
  const facing = new THREE.Matrix4().makeRotationZ((s * Math.PI) / 2); // cylinder axis Y → ±X
  const at = (x: number) => new THREE.Matrix4().makeTranslation(x, 0, 0).multiply(facing);
  const lip = new Geo().add(new THREE.CylinderGeometry(rimR, rimR, 0.03, 24, 1, true), at(face));
  lip.add(new THREE.TorusGeometry(rimR - 0.008, 0.01, 4, 24).rotateY(Math.PI / 2), new THREE.Matrix4().makeTranslation(face + s * 0.012, 0, 0));
  const hub = new Geo().add(new THREE.CylinderGeometry(rimR * 0.22, rimR * 0.26, 0.04, 10), at(face));
  const spokes = new Geo();
  if (style === "steel") {
    // Pressed dish: a solid face with a ring of vent holes suggested by a darker inner ring.
    spokes.add(new THREE.CylinderGeometry(rimR * 0.97, rimR * 0.97, 0.012, 24), at(face - s * 0.01));
    return [
      { geo: spokes, material: "steel" },
      { geo: lip, material: "steel" },
      { geo: hub.add(new THREE.TorusGeometry(rimR * 0.6, 0.02, 4, 12).rotateY(Math.PI / 2), new THREE.Matrix4().makeTranslation(face + s * 0.004, 0, 0)), material: "trim" },
    ];
  }
  const n = style === "mesh" ? 16 : 5;
  const spokeW = style === "mesh" ? 0.012 : rimR * 0.3;
  for (let i = 0; i < n; i++) {
    const a = (i / n) * Math.PI * 2;
    const g = new THREE.BoxGeometry(0.02, rimR * 0.78, spokeW);
    g.translate(0, rimR * 0.58, 0);
    if (style === "mesh") g.rotateX(0.5); // crossing pattern: alternate the lean
    const m = new THREE.Matrix4().makeTranslation(face - s * 0.004, 0, 0).multiply(new THREE.Matrix4().makeRotationX(a + (style === "mesh" && i % 2 ? -1 : 0)));
    spokes.add(g, m);
  }
  if (style === "mesh") spokes.add(new THREE.TorusGeometry(rimR * 0.62, 0.008, 3, 16).rotateY(Math.PI / 2), new THREE.Matrix4().makeTranslation(face, 0, 0));
  const mat: PartMaterial = style === "mesh" ? "chrome" : "steel";
  return [
    { geo: spokes, material: mat },
    { geo: lip, material: "chrome" },
    { geo: hub, material: mat },
  ];
}

/** Tread / sidewall details for a compound. */
function treadGeo(tire: string, r: number, width: number, s: number): { geo: Geo; material: PartMaterial }[] {
  const ring = (rad: number, mat: PartMaterial) => {
    const g = new THREE.RingGeometry(rad - 0.018, rad, 28).rotateY((s * Math.PI) / 2);
    return { geo: new Geo().add(g, new THREE.Matrix4().makeTranslation(s * (width / 2 + 0.003), 0, 0)), material: mat };
  };
  switch (tire) {
    case "all_terrain": {
      // Chunky staggered lugs standing proud of the tread.
      const g = new Geo();
      const n = 22;
      for (let row = 0; row < 2; row++)
        for (let i = 0; i < n; i++) {
          const a = ((i + row * 0.5) / n) * Math.PI * 2;
          const lug = new THREE.BoxGeometry(width * 0.42, 0.075, 0.03);
          lug.translate((row ? 1 : -1) * width * 0.24, 0, r + 0.008);
          g.add(lug, new THREE.Matrix4().makeRotationX(a));
        }
      return [{ geo: g, material: "rubber" }];
    }
    case "semi_slick":
      return [ring(r * 0.86, "caliperRace")];
    case "drag_radial":
      return [ring(r * 0.84, "stripe"), ring(r * 0.74, "stripe")];
    case "sport":
      return [ring(r * 0.86, "caliper")];
    default:
      return [];
  }
}

// ---------------------------------------------------------------------------------------------
// Skinning: bind each vertex to the nearest lattice nodes of the piece's region
// ---------------------------------------------------------------------------------------------

function nodesOf(panels: PanelBody, region: string): number[] {
  const code = panels.regions.indexOf(region);
  const out: number[] = [];
  if (code < 0) return out;
  for (let i = 0; i < panels.nodeRegions.length; i++) if (panels.nodeRegions[i] === code) out.push(i);
  return out;
}

function nearest(rest: Float32Array, ids: number[], p: THREE.Vector3): number {
  let best = Infinity;
  for (const i of ids) best = Math.min(best, (rest[i * 3] - p.x) ** 2 + (rest[i * 3 + 1] - p.y) ** 2 + (rest[i * 3 + 2] - p.z) ** 2);
  return best;
}

/** Region (of `candidates`) closest to the piece, and its nodes; any region if none exists. */
function pickRegion(panels: PanelBody, candidates: string[], centre: THREE.Vector3): { region: string | null; nodes: number[] } {
  let best: { region: string | null; nodes: number[]; d: number } = { region: null, nodes: [], d: Infinity };
  for (const r of candidates) {
    const nodes = nodesOf(panels, r);
    if (nodes.length === 0) continue;
    const d = nearest(panels.restNodes, nodes, centre);
    if (d < best.d) best = { region: r, nodes, d };
  }
  if (best.region) return best;
  // The lattice lacks every named region: bind to whatever region is nearest.
  const all = Array.from({ length: panels.restNodes.length / 3 }, (_, i) => i);
  const rest = panels.restNodes;
  let id = 0;
  let d = Infinity;
  for (const i of all) {
    const di = (rest[i * 3] - centre.x) ** 2 + (rest[i * 3 + 1] - centre.y) ** 2 + (rest[i * 3 + 2] - centre.z) ** 2;
    if (di < d) [id, d] = [i, di];
  }
  const region = panels.regions[panels.nodeRegions[id]] ?? null;
  return { region, nodes: region ? nodesOf(panels, region) : all };
}

const K = 4;

/** Mesh-file shaped part for a piece: positions in mm, inverse-distance weights to the K
 * nearest nodes of its region (bytes summing to 255). */
function skinPiece(panels: PanelBody, piece: Piece): MeshPartJson {
  const { pos, idx } = piece.geo;
  const n = pos.length / 3;
  const centre = new THREE.Vector3();
  for (let i = 0; i < n; i++) centre.add(V(pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]));
  centre.multiplyScalar(1 / Math.max(n, 1));
  const { region, nodes } = pickRegion(panels, piece.regions, centre);
  const rest = panels.restNodes;
  const skinNodes: number[] = [];
  const skinWeights: number[] = [];
  for (let v = 0; v < n; v++) {
    const px = pos[v * 3];
    const py = pos[v * 3 + 1];
    const pz = pos[v * 3 + 2];
    const near: [number, number][] = [];
    for (const i of nodes) {
      const d = Math.hypot(rest[i * 3] - px, rest[i * 3 + 1] - py, rest[i * 3 + 2] - pz);
      if (near.length < K) near.push([d, i]);
      else if (d < near[K - 1][0]) near[K - 1] = [d, i];
      else continue;
      near.sort((a, b) => a[0] - b[0]);
    }
    const w = near.map(([d]) => 1 / (d + 0.05));
    const sum = w.reduce((a, b) => a + b, 0) || 1;
    const bytes = w.map((x) => Math.round((x / sum) * 255));
    bytes[0] += 255 - bytes.reduce((a, b) => a + b, 0);
    for (let k = 0; k < K; k++) {
      skinNodes.push(near[k]?.[1] ?? 0);
      skinWeights.push(near[k] ? bytes[k] : 0);
    }
  }
  return {
    name: `mod_${piece.name}`,
    region,
    kind: "mod",
    material: piece.material,
    positions_mm: pos.map((x) => x * 1000),
    uvs: [],
    indices: idx,
    skin_nodes: skinNodes,
    skin_weights: skinWeights,
  };
}

// ---------------------------------------------------------------------------------------------
// Materials: paint finish, carbon panels, tint, interior, lamps, vinyl overlay
// ---------------------------------------------------------------------------------------------

const PAINT_FINISH: Record<string, { metalness: number; roughness: number }> = {
  gloss: { metalness: 0.15, roughness: 0.18 },
  metallic: { metalness: 0.8, roughness: 0.28 },
  matte: { metalness: 0, roughness: 0.95 },
  pearl: { metalness: 0.55, roughness: 0.14 },
};

/** Vinyl pattern ids for the overlay shader. */
const VINYL_ID: Record<string, number> = { stripes: 1, flames: 2, tribal: 3, sponsor_generic: 4 };

/** Transparent overlay drawing a vinyl from chassis-space position (so it dents with the paint). */
function vinylMaterial(style: string, all: Box): THREE.MeshStandardMaterial {
  const mat = new THREE.MeshStandardMaterial({
    color: 0xffffff,
    roughness: 0.5,
    metalness: 0.1,
    transparent: true,
    depthWrite: false,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
    side: THREE.DoubleSide,
    flatShading: true,
  });
  const u = {
    uVinyl: { value: VINYL_ID[style] ?? 0 },
    uFront: { value: all.max.y },
    uLen: { value: all.max.y - all.min.y },
    uLow: { value: all.min.z },
    uHigh: { value: all.max.z },
  };
  mat.onBeforeCompile = (sh) => {
    Object.assign(sh.uniforms, u);
    sh.vertexShader = sh.vertexShader
      .replace("#include <common>", "#include <common>\nvarying vec3 vObj;")
      .replace("#include <begin_vertex>", "#include <begin_vertex>\nvObj = position;");
    sh.fragmentShader = sh.fragmentShader
      .replace(
        "#include <common>",
        `#include <common>
varying vec3 vObj;
uniform int uVinyl;
uniform float uFront, uLen, uLow, uHigh;
vec4 vinyl(vec3 p, vec3 n) {
  bool top = abs(n.z) > 0.55;
  bool side = abs(n.x) > 0.6;
  float zn = (p.z - uLow) / max(uHigh - uLow, 0.1);
  float yn = (uFront - p.y) / uLen; // 0 at the nose, 1 at the tail
  if (uVinyl == 1) {
    float s = abs(p.x);
    if (top && zn > 0.4 && s > 0.06 && s < 0.17) return vec4(0.95, 0.95, 0.95, 1.0);
  } else if (uVinyl == 2) {
    float f = 1.0 - yn / 0.45;
    float tongue = 0.5 + 0.5 * sin(p.z * 34.0 + 1.3);
    float e = f - (1.0 - tongue) * 0.75 - abs(zn - 0.42) * 0.9;
    if (side && e > 0.0) return vec4(mix(vec3(0.85, 0.08, 0.02), vec3(1.0, 0.85, 0.1), clamp(e * 3.0, 0.0, 1.0)), 1.0);
    float h = 1.0 - yn / 0.28;
    float t2 = 0.5 + 0.5 * sin(p.x * 26.0);
    float e2 = h - (1.0 - t2) * 0.8;
    if (top && zn > 0.4 && e2 > 0.0) return vec4(mix(vec3(0.85, 0.08, 0.02), vec3(1.0, 0.85, 0.1), clamp(e2 * 3.0, 0.0, 1.0)), 1.0);
  } else if (uVinyl == 3) {
    float sp = abs(fract(p.y * 2.4) - 0.5) * 2.0;
    if (side && yn > 0.15 && yn < 0.85 && zn > 0.28 && zn < 0.28 + 0.3 * (1.0 - sp)) return vec4(0.04, 0.04, 0.05, 1.0);
  } else if (uVinyl == 4) {
    vec2 c = vec2(uFront - uLen * 0.47, uLow + (uHigh - uLow) * 0.38);
    float d = length(vec2(p.y, p.z) - c);
    if (side && d < 0.2) return vec4(0.97, 0.97, 0.97, 1.0);
    if (side && d < 0.23) return vec4(0.1, 0.25, 0.75, 1.0);
    if (side && zn > 0.16 && zn < 0.22) return vec4(0.1, 0.25, 0.75, 1.0);
    if (top && zn > 0.4 && yn < 0.22 && yn > 0.08 && abs(p.x) < 0.3) return vec4(0.97, 0.97, 0.97, 1.0);
  }
  return vec4(0.0);
}`,
      )
      .replace(
        "#include <color_fragment>",
        `#include <color_fragment>
vec3 nObj = normalize(cross(dFdx(vObj), dFdy(vObj)));
vec4 vin = vinyl(vObj, nObj);
if (vin.a < 0.5) discard;
diffuseColor = vec4(vin.rgb, 1.0);`,
      );
  };
  return mat;
}

// ---------------------------------------------------------------------------------------------
// Fitting a build onto a car
// ---------------------------------------------------------------------------------------------

/**
 * Put `build` on a loaded PanelBody: body pieces become skinned parts, wheel pieces go to
 * `wheelParts` / `hubParts` (call before VehicleView parents those), materials change.
 * `wheels` are the car's (modified) `vehicle_info` wheels; `lamps` the car's head/tail
 * lamp materials (smoked covers tint them).
 */
export function fitBuild(panels: PanelBody, build: BuildVisuals, wheels: WheelInfo[]): FittedBuild {
  const fitted: FittedBuild = { pieces: {}, drawn: [], lamps: null };
  const mesh = panels.mesh;
  if (!mesh || (build.parts.length === 0 && build.wheels.every((w) => w.width_m === w.base_width_m))) return fitted;
  const probe = new BodyProbe(mesh);
  const all = probe.all;
  const ctx: Ctx = {
    b: probe,
    wheels,
    hw: all.max.x,
    floor: all.min.z,
    front: probe.region("FrontBumper", [0.05, 0.92, 0.05, 0.95, 1, 0.45]),
    rear: probe.region("RearBumper", [0.05, 0, 0.05, 0.95, 0.08, 0.45]),
    hood: probe.region("Hood", [0.1, 0.65, 0.4, 0.9, 0.95, 0.7]),
    trunk: probe.region("Trunk", [0.1, 0.05, 0.4, 0.9, 0.3, 0.7]),
    roof: probe.region("Roof", [0.12, 0.3, 0.8, 0.88, 0.6, 1]),
  };
  const drawn = new Set<string>();
  const byStyle = (kind: string) => build.parts.filter((p) => p.part === kind).at(-1)?.style;

  // Body pieces.
  const defs: MeshPartJson[] = [];
  for (const part of build.parts) {
    const builder = BUILDERS[part.part];
    if (!builder) continue;
    for (const p of builder(ctx, part.style)) {
      if (p.geo.empty) continue;
      // Sided pieces only bind to regions on their side.
      const regions = p.side === undefined ? part.regions : part.regions.filter((r) => sideOf(r) === 0 || sideOf(r) === p.side);
      const def = skinPiece(panels, { name: p.name, material: p.material, geo: p.geo, regions });
      defs.push(def);
      (fitted.pieces[part.part] ??= []).push(def.name);
      drawn.add(part.part);
    }
  }
  panels.addModParts(defs);

  // Wheels: tire width, rims, tread, brakes.
  const rimStyle = byStyle("Rims");
  const brake = byStyle("BrakeKit");
  build.wheels.forEach((wv, i) => {
    const wi = wheels[i];
    if (!wi) return;
    const s = Math.sign(wi.position_m[0]) || 1;
    const scale = wv.base_width_m > 0 ? wv.width_m / wv.base_width_m : 1;
    for (const m of panels.wheelParts[i]) if (m.name.endsWith("_tire")) m.scale.x = scale;
    const rimR = (stockRimRadius(mesh, i) || wi.radius_m * 0.62) * (wv.radius_m / (wv.base_radius_m || wv.radius_m));
    const addWheel = (geo: Geo, material: PartMaterial, name: string, hub = false) => {
      panels.addWheelPart({ name, region: null, kind: "mod", material, wheel: i, positions_mm: geo.pos.map((x) => x * 1000), uvs: [], indices: geo.idx, skin_nodes: [], skin_weights: [] }, hub);
    };
    if (rimStyle) {
      // The stock rim goes; the new design takes its place.
      for (let k = panels.wheelParts[i].length - 1; k >= 0; k--) if (panels.wheelParts[i][k].name.endsWith("_rim")) panels.wheelParts[i].splice(k, 1)[0].removeFromParent();
      rimGeo(rimStyle, rimR, s, wv.width_m).forEach((r, k) => addWheel(r.geo, r.material, `mod_rim_${rimStyle}_${i}_${k}`));
      drawn.add("Rims");
    }
    const tread = treadGeo(wv.tire_id === wv.base_tire_id ? "" : wv.tire_id, wv.radius_m, wv.width_m, s);
    tread.forEach((t, k) => addWheel(t.geo, t.material, `mod_tread_${wv.tire_id}_${i}_${k}`));
    if (tread.length) drawn.add("Tires");
    if (brake) {
      const race = brake === "race";
      const discR = rimR * (race ? 0.86 : 0.8);
      const disc = new Geo().add(new THREE.CylinderGeometry(discR, discR, 0.028, 24).rotateZ(Math.PI / 2), new THREE.Matrix4().makeTranslation(s * 0.005, 0, 0));
      const bell = new Geo().add(new THREE.CylinderGeometry(discR * 0.45, discR * 0.45, 0.04, 12).rotateZ(Math.PI / 2), new THREE.Matrix4().makeTranslation(s * 0.012, 0, 0));
      addWheel(disc, "metal", `mod_disc_${i}`);
      addWheel(bell, "trim", `mod_disc_bell_${i}`);
      // Caliper over the rear-top of the disc (behind the axle, as on most cars).
      const a = (Math.PI * 3) / 4;
      const cal = new THREE.BoxGeometry(0.055, race ? 0.21 : 0.16, 0.06);
      const m = new THREE.Matrix4()
        .makeTranslation(s * 0.028, Math.cos(a) * discR * 0.86, Math.sin(a) * discR * 0.86)
        .multiply(new THREE.Matrix4().makeRotationX(a - Math.PI / 2));
      addWheel(new Geo().add(cal, m), race ? "caliperRace" : "caliper", `mod_caliper_${i}`, true);
      drawn.add("BrakeKit");
    }
  });

  // Materials.
  const finish = PAINT_FINISH[byStyle("Paint") ?? ""];
  if (finish) {
    panels.setModLook("paint", finish);
    drawn.add("Paint");
  }
  for (const [kind, region] of [["HoodPanel", "Hood"], ["TrunkPanel", "Trunk"]] as const) {
    if (!byStyle(kind)) continue;
    panels.setPartMaterial((def) => def.region === region && def.material === "paint" && def.kind === "panel", "carbon");
    drawn.add(kind);
  }
  if (byStyle("WindowTint")) {
    panels.setModLook("glass", { color: 0x0d1013, opacity: 0.78 });
    drawn.add("WindowTint");
  }
  if (byStyle("InteriorTrim")) {
    panels.setModLook("interior", { color: 0x6b4a32 });
    drawn.add("InteriorTrim");
  }
  if (byStyle("Lights")) {
    fitted.lamps = { head: 0x55575c, tail: 0x1c0000 };
    drawn.add("Lights");
  }
  // Mod pieces and re-materialed panels into the batches (once), then the vinyl over the
  // paint batch (mod pieces are batched apart, so they carry no vinyl).
  panels.rebatch();
  const vinyl = byStyle("Vinyl");
  const paintBatch = panels.batchMesh("paint");
  if (vinyl && VINYL_ID[vinyl] && paintBatch) {
    const overlay = new THREE.Mesh(paintBatch.geometry, vinylMaterial(vinyl, all));
    overlay.name = `${paintBatch.name}_vinyl`;
    overlay.frustumCulled = false;
    overlay.renderOrder = 1;
    paintBatch.add(overlay);
    drawn.add("Vinyl");
  }
  probe.dispose();
  fitted.drawn = [...drawn].sort();
  return fitted;
}

/** −1 for left-side regions, +1 right, 0 centre. */
function sideOf(region: string): number {
  if (region.includes("Left")) return -1;
  if (region.includes("Right")) return 1;
  return 0;
}
