// Wide-tyre stance: how a car fitted with wider tyres ("Wider Tires", `tires_wide`) looks.
// Render only: the sim's tyre width (+2 cm per grade, data/mods/tires.json) drives grip; this
// file only exaggerates the look so the mod reads at a glance (owner request 2026-10-31).
//
// Per width grade (one grade = one `wheels.width_m` step of WIDTH_STEP_M, or one `tyres: wide`
// visual piece):
// - a wider tread: the wheel's spinning group is scaled across the axle (TREAD_SCALE);
// - the wheel pushed out to fill the arch: the inner face stays put, the outer face moves out
//   (a wider track / lower offset look), capped so the tyre stays under the arch lip;
// - a deeper dish: the rim, disc and calipers sit back inside the tyre by DISH_M, with a bright
//   lip ring and barrel at the bead;
// - a body-colour arch lip (fender flare) traced along each wheel arch's real edge.
//
// Every car with the mod gets it the same way: VehicleView applies it from the build's wheel
// widths (player, AI opponents, lot cars) or from a cosmetic look's mod list (career car,
// workshop preview, touge opponents), in every art style (the lip uses the body paint, the
// ring the mesh's chrome slot, both restyled by PanelBody).
//
// Coordinates: chassis space (+X right, +Y forward, +Z up), metres; wheel space has x along the
// axle (outward = the side the wheel is on).
import * as THREE from "three";
import type { CarMeshJson, MeshPartJson, PanelBody } from "../panels";
import type { VehicleInfo } from "../../wasm/sim";

/** Physics width added by one grade of wider tyres (data/mods/tires.json `tires_wide`), m. */
export const WIDTH_STEP_M = 0.02;
/** Highest grade drawn (more width steps look like this one). */
export const MAX_GRADE = 3;
/** Visual tread width per grade, × the stock tyre mesh (grade 0 = stock). */
export const TREAD_SCALE = [1, 1.2, 1.36, 1.5] as const;
/** How far the arch lip (fender flare) juts out of the body per grade, at least / at most, m.
 * A roomy arch gets the small lip; a tight one a flare big enough to cover the wider tread. */
export const LIP_MIN_M = [0, 0.018, 0.024, 0.03] as const;
export const LIP_MAX_M = [0, 0.045, 0.055, 0.065] as const;
/** How far the rim face sits back inside the tyre per grade, m (deep dish). */
export const DISH_M = [0, 0.016, 0.022, 0.028] as const;
/** Most the wheel is pushed out past keeping its inner face still, per grade, m. */
export const FILL_M = [0, 0.04, 0.05, 0.06] as const;
/** Gap kept between the tyre's outer face and the outside of the arch lip, m (front wheels
 * add FRONT_EXTRA_M: they swing out at steering lock). */
export const MARGIN_M = 0.008;
export const FRONT_EXTRA_M = 0.006;
/** The lip's inner edge sits this far inside the panel surface, m (no seam shows). */
const LIP_SINK_M = 0.004;
/** Radial height of the lip (from the arch edge up the panel), m. */
const LIP_TALL_M = 0.07;

/** Width grade of a wheel from its stock and modified physics widths (0 = stock). */
export function widthGrade(baseWidth: number, width: number): number {
  if (!(baseWidth > 0) || !(width > baseWidth)) return 0;
  return Math.min(MAX_GRADE, Math.max(0, Math.round((width - baseWidth) / WIDTH_STEP_M)));
}

/** Width grade of a cosmetic look's pieces (data/cosmetics/visuals.json `tyres: wide`). */
export function gradeOfPieces(pieces: readonly { piece: string; style?: string; level?: number }[]): number {
  let g = 0;
  for (const p of pieces) if (p.piece === "tyres" && p.style === "wide") g += p.level ?? 1;
  return Math.min(MAX_GRADE, g);
}

/** What one corner's fit is solved from (wheel space / chassis |x|, m). */
export interface CornerFit {
  /** |x| of the hub. */
  hubX: number;
  /** Half the stock tyre mesh width. */
  tyreHalf: number;
  /** |x| of the outside of the body at the top of the arch (null: not found; no lip). */
  archX: number | null;
  /** Steered wheel (more margin). */
  front: boolean;
}

/** How one corner is drawn. */
export interface CornerStance {
  /** Spin group scale across the axle. */
  scale: number;
  /** Outward offset of the wheel (hub space), m. */
  push: number;
  /** Rim face set back by this much (wheel space, before the scale), m. */
  dish: number;
  /** How far the arch lip juts out of the body, m (0: none). */
  lip: number;
}

/** Stock: nothing changes. */
export const STOCK: CornerStance = { scale: 1, push: 0, dish: 0, lip: 0 };

/**
 * Solve one corner. The tread grows by TREAD_SCALE; the wheel moves out so its inner face stays
 * where it was (nothing new to clip on the suspension side), plus up to FILL_M to fill the
 * arch. The outer face stays MARGIN_M inside the outside of the arch lip, which juts out
 * LIP_MIN_M, or more on a tight arch (up to LIP_MAX_M: a bolt-on flare covering the tread).
 * Past that the wheel stops moving out (the inner face moves in instead), and as a last resort
 * the tread is trimmed to fit.
 */
export function solveCorner(fit: CornerFit, grade: number): CornerStance {
  const g = Math.min(MAX_GRADE, Math.max(0, Math.round(grade)));
  if (g === 0) return STOCK;
  let scale: number = TREAD_SCALE[g];
  const dish = DISH_M[g];
  const grow = fit.tyreHalf * (scale - 1);
  if (fit.archX === null) return { scale, push: grow, dish, lip: 0 };
  const margin = MARGIN_M + (fit.front ? FRONT_EXTRA_M : 0);
  // Room for the outer face to move out without a lip, and the lip that covers the rest.
  // (A stock tyre already standing out of the arch edge, as on the open-fendered prototype,
  // counts as flush: the body never covered it.)
  const gap = Math.max(fit.archX - margin - (fit.hubX + fit.tyreHalf), 0);
  const lip = Math.min(Math.max(2 * grow - gap, LIP_MIN_M[g]), LIP_MAX_M[g]);
  const room = gap + lip;
  if (room < grow) {
    // Even growing evenly the tread would stand out of the lip: trim it.
    scale = 1 + Math.max(room, 0) / fit.tyreHalf;
    return { scale, push: 0, dish, lip };
  }
  const push = Math.min(room, 2 * grow + FILL_M[g]) - grow;
  return { scale, push, dish, lip };
}

/** One sample of an arch edge (chassis space). */
export interface ArchPoint {
  /** Angle around the hub in the car's YZ plane: 0 forward, π/2 up, π back. */
  a: number;
  /** Distance from the hub to the edge of the body, m. */
  r: number;
  /** |x| of the outside of the body there, m. */
  x: number;
}

/** Part kinds that make up the outside of a wheel arch. */
const SKIN = new Set(["panel", "body"]);

/** How far above the arch edge the panel's outside is measured, m. */
const EDGE_BAND_M = 0.05;

/**
 * Trace the edge of the wheel arch over hub `hub` (chassis space, the side of `hub[0]`) on the
 * rest-pose mesh: at angles over the top of the wheel, the smallest distance from the hub where
 * a sideways ray finds the outside of the body, and how far out the panel is just above it.
 * Pure (no Three), so it runs once at spawn.
 */
export function traceArch(mesh: CarMeshJson, hub: readonly [number, number, number], radius: number): ArchPoint[] {
  const sx = Math.sign(hub[0]) || 1;
  const hubX = Math.abs(hub[0]);
  const [, hy, hz] = hub;
  const reach = radius + 0.2;
  // Triangles near this arch on this side: (y, z) corners and the outward x.
  const tris: number[] = [];
  for (const p of mesh.parts) {
    if (p.wheel != null || !SKIN.has(p.kind) || p.region === "Accessory") continue;
    nearTriangles(p, sx, hubX, hy, hz, reach, tris);
  }
  const out: ArchPoint[] = [];
  for (let deg = 15; deg <= 165; deg += 10) {
    const a = (deg * Math.PI) / 180;
    const ca = Math.cos(a);
    const sa = Math.sin(a);
    for (let r = radius + 0.005; r <= radius + 0.17; r += 0.0075) {
      const x = outerX(tris, hy + r * ca, hz + r * sa);
      if (x !== null && x > hubX) {
        // The outside of the panel just above the edge (arch edges often roll inward).
        let w = x;
        for (let r2 = r + 0.0075; r2 <= r + EDGE_BAND_M; r2 += 0.0075) w = Math.max(w, outerX(tris, hy + r2 * ca, hz + r2 * sa) ?? w);
        out.push({ a, r, x: w });
        break;
      }
    }
  }
  return out;
}

/** Append the triangles of `p` within `reach` of the hub (y, z) on side `sx` as 9 floats each
 * (y0 z0 x0 y1 z1 x1 y2 z2 x2, x outward-positive). */
function nearTriangles(p: MeshPartJson, sx: number, hubX: number, hy: number, hz: number, reach: number, out: number[]): void {
  const v = p.positions_mm;
  const idx = p.indices;
  for (let t = 0; t + 2 < idx.length; t += 3) {
    let near = false;
    let side = true;
    for (let k = 0; k < 3; k++) {
      const i = idx[t + k] * 3;
      const x = v[i] * 1e-3 * sx;
      const y = v[i + 1] * 1e-3;
      const z = v[i + 2] * 1e-3;
      if (x < hubX * 0.5) side = false;
      if (Math.abs(y - hy) < reach && z > hz - 0.1 && z < hz + reach) near = true;
    }
    if (!near || !side) continue;
    for (let k = 0; k < 3; k++) {
      const i = idx[t + k] * 3;
      out.push(v[i + 1] * 1e-3, v[i + 2] * 1e-3, v[i] * 1e-3 * sx);
    }
  }
}

/** Outermost x where a sideways ray at (y, z) crosses the triangles (null: none). */
function outerX(tris: number[], y: number, z: number): number | null {
  let best: number | null = null;
  for (let t = 0; t < tris.length; t += 9) {
    const y0 = tris[t], z0 = tris[t + 1];
    const y1 = tris[t + 3], z1 = tris[t + 4];
    const y2 = tris[t + 6], z2 = tris[t + 7];
    const d = (z1 - z2) * (y0 - y2) + (y2 - y1) * (z0 - z2);
    if (Math.abs(d) < 1e-12) continue;
    const w0 = ((z1 - z2) * (y - y2) + (y2 - y1) * (z - z2)) / d;
    const w1 = ((z2 - z0) * (y - y2) + (y0 - y2) * (z - z2)) / d;
    const w2 = 1 - w0 - w1;
    if (w0 < 0 || w1 < 0 || w2 < 0) continue;
    const x = w0 * tris[t + 2] + w1 * tris[t + 5] + w2 * tris[t + 8];
    if (best === null || x > best) best = x;
  }
  return best;
}

/** |x| of the body at the top of an arch: the narrowest sample between 50° and 130° (null: too
 * few samples to trust). */
export function archTopX(points: readonly ArchPoint[]): number | null {
  const top = points.filter((p) => p.a >= (50 * Math.PI) / 180 && p.a <= (130 * Math.PI) / 180);
  if (top.length < 2) return null;
  return Math.min(...top.map((p) => p.x));
}

/** Longest run of consecutive samples (10° apart): the lip is one unbroken piece. */
function longestRun(points: readonly ArchPoint[]): ArchPoint[] {
  let best: ArchPoint[] = [];
  let run: ArchPoint[] = [];
  for (const p of points) {
    if (run.length && p.a - run[run.length - 1].a > 0.2) run = [];
    run.push(p);
    if (run.length > best.length) best = run.slice();
  }
  return best;
}

/**
 * A fender flare along an arch edge (`run`, hub at (hy, hz) on side `sx`): a wedge that starts
 * at the arch edge, juts out `lip` at its lower edge and slopes back into the panel LIP_TALL_M
 * up. The edge is smoothed and sampled three times as densely as traced.
 */
export function flareGeometry(run: readonly ArchPoint[], sx: number, hy: number, hz: number, lip: number): THREE.BufferGeometry {
  // Smooth the traced radius / surface (the trace steps 7.5 mm), then densify.
  const sm = run.map((p, k) => {
    const a = run[Math.max(k - 1, 0)];
    const b = run[Math.min(k + 1, run.length - 1)];
    return { a: p.a, r: (a.r + 2 * p.r + b.r) / 4, x: (a.x + 2 * p.x + b.x) / 4 };
  });
  const edge: ArchPoint[] = [];
  for (let k = 0; k + 1 < sm.length; k++)
    for (let s = 0; s < 3; s++) {
      const t = s / 3;
      const p = sm[k];
      const q = sm[k + 1];
      edge.push({ a: p.a + (q.a - p.a) * t, r: p.r + (q.r - p.r) * t, x: p.x + (q.x - p.x) * t });
    }
  edge.push(sm[sm.length - 1]);
  // Cross-section: (outward, up the panel from the arch edge), m.
  const profile: [number, number][] = [
    [-LIP_SINK_M, 0.002],
    [lip, 0.004],
    [lip * 0.8, 0.02],
    [-LIP_SINK_M, LIP_TALL_M],
  ];
  const at = (p: ArchPoint, [o, d]: [number, number]): [number, number, number] => {
    const r = p.r + d;
    return [sx * (p.x + o), hy + r * Math.cos(p.a), hz + r * Math.sin(p.a)];
  };
  const pos: number[] = [];
  const quad = (a: number[], b: number[], c: number[], d: number[]) => pos.push(...a, ...b, ...c, ...a, ...c, ...d);
  for (let k = 0; k + 1 < edge.length; k++)
    for (let j = 0; j + 1 < profile.length; j++) quad(at(edge[k], profile[j]), at(edge[k + 1], profile[j]), at(edge[k + 1], profile[j + 1]), at(edge[k], profile[j + 1]));
  // End caps.
  for (const p of [edge[0], edge[edge.length - 1]]) quad(at(p, profile[0]), at(p, profile[1]), at(p, profile[2]), at(p, profile[3]));
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  geo.computeVertexNormals();
  return geo;
}

/** What the stance needs from the vehicle view (VehicleView implements it). */
export interface StanceHost {
  readonly panels: PanelBody;
  readonly info: VehicleInfo;
  /** Per wheel: the spinning group and the steering-only group (calipers), and the debug tyre. */
  readonly wheels: readonly { spin: THREE.Object3D; camber: THREE.Object3D; tire: THREE.Object3D }[];
}

/** One corner's measurements, taken once. */
interface Corner {
  fit: CornerFit;
  arch: ArchPoint[];
  rimR: number;
  rimOuter: number;
}

/** Body-colour arch lip of one corner, riding its fender's lattice. */
interface Lip {
  mesh: THREE.Mesh;
  region: string;
}

const tmp = new THREE.Vector3();

/**
 * The wide-tyre stance on one car. `apply(grade)` (re)draws it (0 = stock); call again after
 * new wheel pieces were added (a cosmetic look's rims); `update` per frame; `dispose` to go.
 */
export class WheelStance {
  /** Arch lips (child of the panel root: they follow ride height and art style). */
  readonly root = new THREE.Group();
  private corners: Corner[] | null = null;
  private lips: Lip[] = [];
  /** Wheel-space bead pieces (lip ring + barrel), per wheel. */
  private beads: THREE.Object3D[] = [];
  private shown = -1;
  /** Per wheel, what is drawn now. */
  readonly corner: CornerStance[] = [STOCK, STOCK, STOCK, STOCK];

  constructor(private readonly host: StanceHost) {
    this.root.name = "wide_stance";
    host.panels.root.add(this.root);
  }

  /** Grade drawn now. */
  get grade(): number {
    return this.shown < 0 ? 0 : this.shown;
  }

  /** Draw grade `grade` (0 = stock). Cheap when nothing changed apart from new wheel pieces. */
  apply(grade: number): void {
    const g = Math.min(MAX_GRADE, Math.max(0, Math.round(grade)));
    const host = this.host;
    if (!host.panels.mesh) return;
    if (g !== this.shown) {
      this.clearPieces();
      this.shown = g;
      if (g > 0) this.buildPieces(g);
      else this.corner.fill(STOCK);
    }
    // Wheel groups and what sits in them (looks add pieces later; re-run is cheap).
    host.wheels.forEach((w, i) => {
      const c = this.corner[i] ?? STOCK;
      const sx = Math.sign(host.info.wheels[i]?.position_m[0] ?? 1) || 1;
      w.spin.scale.x = c.scale;
      w.spin.position.x = sx * c.push;
      // Calipers and uprights stay on the (set back, widened) disc.
      w.camber.position.x = sx * (c.push - c.dish * c.scale);
      for (const o of w.spin.children) o.position.x = this.isTyre(o, w.tire) ? 0 : -sx * c.dish;
    });
  }

  /** Scale across the axle of wheel `i` (VehicleView keeps it when it scales for punctures). */
  widthScale(i: number): number {
    return this.corner[i]?.scale ?? 1;
  }

  /** Per frame, after the panel mesh re-skinned: lips ride with their fenders. */
  update(removed: ReadonlySet<string> = EMPTY): void {
    const panels = this.host.panels;
    for (const l of this.lips) {
      l.mesh.visible = !panels.regionGone(l.region) && !removed.has(l.region);
      l.mesh.position.copy(panels.regionShift(l.region, tmp));
    }
  }

  dispose(): void {
    this.apply(0);
    this.root.removeFromParent();
  }

  /** Tyre-like pieces stay with the tread; rims, discs and rim pieces sit back (deep dish). */
  private isTyre(o: THREE.Object3D, debugTire: THREE.Object3D): boolean {
    if (o === debugTire || o.userData.stanceBead) return true;
    const slot = o.userData.slot as string | undefined;
    if (slot?.startsWith("tyre_")) return true;
    if (o instanceof THREE.Mesh) {
      if (this.host.panels.wheelDef(o)?.kind === "tire") return true;
      if (o.name.startsWith("mod_tread_") || o.name.endsWith("_tire")) return true;
    }
    return false;
  }

  private measure(): Corner[] {
    const host = this.host;
    const mesh = host.panels.mesh!;
    const ride = host.panels.root.position.z;
    return host.info.wheels.slice(0, 4).map((w, i) => {
      const [x, y, z] = w.position_m;
      const sx = Math.sign(x) || 1;
      let tyreHalf = 0;
      let rimR = 0;
      let rimOuter = 0;
      for (const p of mesh.parts) {
        if (p.wheel !== i) continue;
        const v = p.positions_mm;
        for (let k = 0; k < v.length; k += 3) {
          if (p.kind === "tire") tyreHalf = Math.max(tyreHalf, Math.abs(v[k]) * 1e-3);
          if (p.kind === "rim") {
            rimR = Math.max(rimR, Math.hypot(v[k + 1], v[k + 2]) * 1e-3);
            rimOuter = Math.max(rimOuter, sx * v[k] * 1e-3);
          }
        }
      }
      if (!(tyreHalf > 0.03)) tyreHalf = w.width_m / 2;
      if (!(rimR > 0.1 && rimR < w.radius_m)) rimR = w.radius_m * 0.62;
      if (!(rimOuter > 0.01 && rimOuter <= tyreHalf)) rimOuter = tyreHalf * 0.85;
      // The panel mesh sits `ride` above the chassis frame (lowered / lifted springs).
      const arch = traceArch(mesh, [x, y, z - ride], w.radius_m);
      const fit: CornerFit = { hubX: Math.abs(x), tyreHalf, archX: archTopX(arch), front: y > 0 };
      return { fit, arch, rimR, rimOuter };
    });
  }

  private buildPieces(g: number): void {
    const host = this.host;
    const corners = (this.corners ??= this.measure());
    const paint = host.panels.paint;
    const chrome = host.panels.materialFor("chrome");
    corners.forEach((c, i) => {
      const st = solveCorner(c.fit, g);
      this.corner[i] = st;
      const w = host.info.wheels[i];
      const sx = Math.sign(w.position_m[0]) || 1;
      // Deep dish: a bright lip ring at the bead and the barrel back to the set-back rim face.
      const bead = new THREE.Group();
      bead.userData.stanceBead = true;
      const ring = new THREE.RingGeometry(c.rimR * 0.86, c.rimR * 1.01, 40).rotateY(Math.PI / 2).translate(sx * c.rimOuter, 0, 0);
      const barrel = new THREE.CylinderGeometry(c.rimR * 0.97, c.rimR * 0.97, st.dish + 0.004, 32, 1, true).rotateZ(Math.PI / 2).translate(sx * (c.rimOuter - st.dish / 2), 0, 0);
      for (const geo of [ring, barrel]) {
        const m = new THREE.Mesh(geo, chrome);
        m.name = `stance_bead_${i}`;
        m.userData.cosmetic = true;
        m.frustumCulled = false;
        bead.add(m);
      }
      bead.userData.cosmetic = true;
      host.wheels[i]?.spin.add(bead);
      this.beads.push(bead);
      // Arch lip in body colour along the traced edge.
      const run = longestRun(c.arch);
      if (st.lip > 0 && run.length >= 4) {
        const geo = flareGeometry(run, sx, w.position_m[1], w.position_m[2] - host.panels.root.position.z, st.lip);
        const mesh = new THREE.Mesh(geo, paint);
        mesh.name = `stance_lip_${i}`;
        const front = w.position_m[1] > 0;
        const region = front ? (sx < 0 ? "FrontLeftFender" : "FrontRightFender") : sx < 0 ? "RearLeftQuarter" : "RearRightQuarter";
        this.root.add(mesh);
        this.lips.push({ mesh, region });
      }
    });
    this.update();
  }

  private clearPieces(): void {
    for (const l of this.lips) {
      l.mesh.removeFromParent();
      l.mesh.geometry.dispose();
    }
    this.lips = [];
    for (const b of this.beads) {
      b.removeFromParent();
      b.traverse((o) => o instanceof THREE.Mesh && o.geometry.dispose());
    }
    this.beads = [];
  }
}

const EMPTY: ReadonlySet<string> = new Set();
