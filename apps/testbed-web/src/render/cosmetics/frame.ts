// Where things are on a car, from its panel mesh (data/panels/meshes/<car>.json): boxes of the
// body regions and named parts, and surface heights on top of a panel. Cosmetic pieces place
// themselves with these, so one builder fits every model. Chassis space (+X right, +Y forward,
// +Z up), metres.
import * as THREE from "three";
import type { WheelInfo } from "../../wasm/sim";
import type { CarMeshJson, LiveryAtlas, MeshPartJson } from "../panels";

/** Car geometry the cosmetic builders read. */
export class CarFrame {
  /** Union of the body panels (no wheels, no aerials). */
  readonly body = new THREE.Box3();
  /** Half the body width, m. */
  readonly halfW: number;
  private readonly regionBoxes = new Map<string, THREE.Box3>();
  private readonly partBoxes = new Map<string, THREE.Box3>();
  private readonly partsByName = new Map<string, MeshPartJson>();

  constructor(
    readonly mesh: CarMeshJson,
    readonly wheels: WheelInfo[],
    readonly atlas: LiveryAtlas,
  ) {
    const v = new THREE.Vector3();
    for (const p of mesh.parts) {
      if (p.wheel != null) continue;
      this.partsByName.set(p.name, p);
      const b = new THREE.Box3();
      for (let i = 0; i < p.positions_mm.length; i += 3)
        b.expandByPoint(v.set(p.positions_mm[i] * 1e-3, p.positions_mm[i + 1] * 1e-3, p.positions_mm[i + 2] * 1e-3));
      this.partBoxes.set(p.name, b);
      if (p.kind === "panel" && p.region) {
        const r = this.regionBoxes.get(p.region) ?? new THREE.Box3();
        this.regionBoxes.set(p.region, r.union(b));
      }
      if (p.kind === "panel" || p.kind === "body") this.body.union(b);
    }
    if (this.body.isEmpty()) this.body.set(new THREE.Vector3(-0.9, -2.3, 0.15), new THREE.Vector3(0.9, 2.3, 1.4));
    this.halfW = Math.max(-this.body.min.x, this.body.max.x);
  }

  /** Box of a body region's panels (null when the model has none). */
  region(name: string): THREE.Box3 | null {
    return this.regionBoxes.get(name)?.clone() ?? null;
  }

  /** Box of a named part (null when the model has none). */
  part(name: string): THREE.Box3 | null {
    return this.partBoxes.get(name)?.clone() ?? null;
  }

  /** Union of the parts whose name passes `pick` (null when none do). */
  parts(pick: (p: MeshPartJson) => boolean): THREE.Box3 | null {
    let out: THREE.Box3 | null = null;
    for (const [name, b] of this.partBoxes) {
      const p = this.partsByName.get(name)!;
      if (pick(p)) out = (out ?? new THREE.Box3()).union(b);
    }
    return out;
  }

  /**
   * Most forward point of the nose over x in [x0, x1], z in [z0, z1] (front bumper box, and the
   * grilles, lamps and skid plates standing in front of it; not the police push bar and light
   * bars, which bolt-ons replace or leave alone).
   */
  frontMost(x0: number, x1: number, z0: number, z1: number): number {
    let y = this.bumperFront.max.y;
    for (const b of this.endParts(x0, x1, z0, z1)) if (b.min.y > 0) y = Math.max(y, b.max.y);
    return y;
  }

  /** Most rearward point of the tail over x in [x0, x1], z in [z0, z1] (as `frontMost`). */
  rearMost(x0: number, x1: number, z0: number, z1: number): number {
    let y = this.bumperRear.min.y;
    for (const b of this.endParts(x0, x1, z0, z1)) if (b.max.y < 0) y = Math.min(y, b.min.y);
    return y;
  }

  /** Boxes of the body, detail and lamp parts overlapping an x / z window (no Accessory). */
  private endParts(x0: number, x1: number, z0: number, z1: number): THREE.Box3[] {
    const out: THREE.Box3[] = [];
    for (const [name, b] of this.partBoxes) {
      const p = this.partsByName.get(name)!;
      if (p.region === "Accessory" || !["panel", "body", "detail", "light"].includes(p.kind)) continue;
      if (b.max.x < x0 || b.min.x > x1 || b.max.z < z0 || b.min.z > z1) continue;
      out.push(b);
    }
    return out;
  }

  /** Front bumper box, or the front 0.3 m of the body. */
  get bumperFront(): THREE.Box3 {
    const b = this.region("FrontBumper");
    if (b) return b;
    const r = this.body.clone();
    r.min.y = r.max.y - 0.3;
    r.max.z = r.min.z + 0.5;
    return r;
  }

  /** Rear bumper box, or the rear 0.3 m of the body. */
  get bumperRear(): THREE.Box3 {
    const b = this.region("RearBumper");
    if (b) return b;
    const r = this.body.clone();
    r.max.y = r.min.y + 0.3;
    r.max.z = r.min.z + 0.5;
    return r;
  }

  /** Hood box, or a guess ahead of the windshield. */
  get hood(): THREE.Box3 {
    return this.region("Hood") ?? new THREE.Box3(new THREE.Vector3(-this.halfW + 0.05, this.body.max.y - 1.2, 0.7), new THREE.Vector3(this.halfW - 0.05, this.body.max.y, 0.85));
  }

  /** Trunk lid box (the tailgate on trucks and vans), or a guess at the back. */
  get trunk(): THREE.Box3 {
    return this.region("Trunk") ?? new THREE.Box3(new THREE.Vector3(-this.halfW + 0.05, this.body.min.y, 0.7), new THREE.Vector3(this.halfW - 0.05, this.body.min.y + 1.0, 0.85));
  }

  /**
   * Where a rear wing stands: the back edge `y` it sits on, the surface height `z` there and the
   * width `w` of what it sits on. A trunk lid: its back edge. An upright tailgate (hatchback,
   * estate, van, SUV) has no lid to stand on, so the back edge of the roof, where the stock roof
   * spoilers are; a pickup (roof far ahead of the tailgate): the top of the tailgate.
   */
  get wingMount(): { y: number; z: number; w: number } {
    const t = this.trunk;
    const depth = t.max.y - t.min.y;
    if (depth >= 0.25) {
      const y = t.min.y + 0.06;
      return { y, z: this.surfaceZ("trunk", 0, y + 0.1, t.max.z), w: t.max.x - t.min.x };
    }
    const r = this.region("Roof");
    if (r && r.min.y - t.min.y < 1.2) {
      const y = r.min.y + 0.06;
      return { y, z: this.surfaceZ("roof", 0, y + 0.1, r.max.z), w: r.max.x - r.min.x };
    }
    return { y: t.min.y + 0.06, z: t.max.z, w: t.max.x - t.min.x };
  }

  /** Roof box, or the top of the body. */
  get roof(): THREE.Box3 {
    const b = this.region("Roof");
    if (b) return b;
    const r = this.body.clone();
    r.min.z = r.max.z - 0.05;
    r.min.y = -0.6;
    r.max.y = 0.3;
    return r;
  }

  /** Cabin interior box (seats, dash), or a guess between the axles. */
  get cabin(): THREE.Box3 {
    return this.parts((p) => p.kind === "interior") ?? new THREE.Box3(new THREE.Vector3(-this.halfW + 0.15, -1.0, 0.22), new THREE.Vector3(this.halfW - 0.15, 0.9, 0.85));
  }

  /** Engine bay contents box (engine kind parts in the front frame). */
  get engine(): THREE.Box3 {
    return (
      this.parts((p) => p.kind === "engine" && p.region === "FrameFront" && !p.name.includes("radiator") && !p.name.includes("intercooler")) ??
      new THREE.Box3(new THREE.Vector3(-0.45, this.hood.min.y + 0.1, 0.15), new THREE.Vector3(0.4, this.hood.max.y - 0.5, 0.72))
    );
  }

  /** Windshield box (null for a model without one). */
  get windshield(): THREE.Box3 | null {
    return this.region("Windshield") ?? this.parts((p) => p.region === "Windshield" && p.kind === "glass");
  }

  /** Head lamp box of one side (−1 left, +1 right), or a guess on the bumper. */
  headlight(side: -1 | 1): THREE.Box3 {
    const b = this.part(side < 0 ? "headlight_l" : "headlight_r");
    if (b) return b;
    const f = this.bumperFront;
    const c = new THREE.Vector3(side * (this.halfW - 0.3), f.max.y, (f.min.z + f.max.z) / 2 + 0.1);
    return new THREE.Box3().setFromCenterAndSize(c, new THREE.Vector3(0.3, 0.04, 0.12));
  }

  /** Tail lamp box of one side, or a guess on the rear bumper. */
  taillight(side: -1 | 1): THREE.Box3 {
    const b = this.part(side < 0 ? "taillight_l" : "taillight_r");
    if (b) return b;
    const r = this.bumperRear;
    const c = new THREE.Vector3(side * (this.halfW - 0.3), r.min.y, r.max.z + 0.05);
    return new THREE.Box3().setFromCenterAndSize(c, new THREE.Vector3(0.3, 0.04, 0.12));
  }

  /**
   * Exhaust outlet: where the stock pipe ends (centre of its last 5 cm), the pipe radius, and
   * which way it blows: out the back (`dir` 0) or out of the side under the sills (−1 left,
   * +1 right; rally raiders, vans and pickups with side exits). Tips on both sides of the car
   * (`both`) give the left one: builders mirror their pieces to the right.
   */
  get exhaustOutlet(): { at: THREE.Vector3; radius: number; dir: -1 | 0 | 1; both: boolean } {
    const tip = this.partsByName.get("exhaust_tip") ?? this.partsByName.get("exhaust_pipe");
    if (tip) {
      const pts: THREE.Vector3[] = [];
      const a = tip.positions_mm;
      for (let i = 0; i < a.length; i += 3) pts.push(new THREE.Vector3(a[i] * 1e-3, a[i + 1] * 1e-3, a[i + 2] * 1e-3));
      // Tips on both sides of the car: keep the left one.
      const both = pts.some((p) => p.x < -0.2) && pts.some((p) => p.x > 0.2) && tip.name === "exhaust_tip";
      const own = both ? pts.filter((p) => p.x < 0) : pts;
      const ext = (f: (p: THREE.Vector3) => number) => own.reduce((m, p) => Math.max(m, f(p)), -Infinity);
      // The end nearest the outside of the body is the outlet: the back, or a side.
      const back = -ext((p) => -p.y) - this.body.min.y;
      const right = this.halfW - ext((p) => p.x);
      const left = this.halfW - ext((p) => -p.x);
      const span = ext((p) => p.y) + ext((p) => -p.y);
      const dir: -1 | 0 | 1 = span > 0.5 && tip.name === "exhaust_tip" ? 0 : back <= Math.min(left, right) ? 0 : right < left ? 1 : -1;
      // Distance of a point along the outlet direction (larger = further out).
      const out = (p: THREE.Vector3) => (dir === 0 ? -p.y : dir * p.x);
      const end = ext(out);
      const ring = own.filter((p) => out(p) >= end - 0.05);
      const c = ring.reduce((s, p) => s.add(p), new THREE.Vector3()).divideScalar(Math.max(ring.length, 1));
      let rx = 0;
      for (const p of ring) rx = Math.max(rx, dir === 0 ? Math.abs(p.x - c.x) : Math.abs(p.y - c.y));
      return { at: c, radius: Math.min(Math.max(rx, 0.03), 0.07), dir, both };
    }
    const r = this.bumperRear;
    return { at: new THREE.Vector3(-this.halfW * 0.55, r.min.y, r.min.z + 0.05), radius: 0.035, dir: 0, both: false };
  }

  /**
   * Top of a part's surface near (x, y): the highest vertex of `name` within `reach` metres in
   * plan, so a scoop sits on a sloping hood. Falls back to the part box top (or `fallback`).
   */
  surfaceZ(name: string, x: number, y: number, fallback = 0.8, reach = 0.18): number {
    const p = this.partsByName.get(name);
    if (!p) return this.partBoxes.get(name)?.max.z ?? fallback;
    const a = p.positions_mm;
    let best = -Infinity;
    const r2 = reach * reach;
    for (let i = 0; i < a.length; i += 3) {
      const dx = a[i] * 1e-3 - x;
      const dy = a[i + 1] * 1e-3 - y;
      if (dx * dx + dy * dy <= r2) best = Math.max(best, a[i + 2] * 1e-3);
    }
    return Number.isFinite(best) ? best : (this.partBoxes.get(name)?.max.z ?? fallback);
  }

  /** Stock rim radius of wheel `i` (from its rim parts), or 62 % of the tyre radius. */
  rimRadius(i: number): number {
    let r = 0;
    for (const p of this.mesh.parts) {
      if (p.wheel !== i || p.kind !== "rim") continue;
      const a = p.positions_mm;
      for (let k = 0; k < a.length; k += 3) r = Math.max(r, Math.hypot(a[k + 1], a[k + 2]) * 1e-3);
    }
    const tyre = this.wheels[i]?.radius_m ?? 0.32;
    return r > 0.1 && r < tyre ? r : tyre * 0.62;
  }

  /** Wheels of an axle (front: steerable or y > 0). */
  axle(which: "front" | "rear"): { info: WheelInfo; index: number }[] {
    return this.wheels.map((info, index) => ({ info, index })).filter((w) => (w.info.position_m[1] > 0) === (which === "front"));
  }
}
