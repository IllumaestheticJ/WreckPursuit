// Garage workshop: the car's hidden hardware, drawn where it lives so every part slot has a
// place on the car. The panel meshes model the shell, lamps, engine bay dress-up, exhaust,
// suspension and wheels; the battery, engine computer, gearbox, differential, fuel tank, steering
// rack, shifter, crash beams and the rest have no mesh. This module builds each of them in code
// for the slots that hold a part and are not drawn by anything else (a mesh part or a cosmetic
// piece), placed from the car's geometry and layout:
//
// - engine position from the engine-bay mesh (front, or behind the cabin for mid-engined cars);
// - drive from the car definition (`drivetrain.layout`: FWD gets a transaxle and half shafts,
//   RWD a tunnel gearbox, prop shaft and rear diff, AWD both ends);
// - axles, track and ride height from the wheels; the bay, cabin and bumpers from the mesh.
//
// Everything is a group tagged `userData.slot`, so it is picked and highlighted like any part.
// Under-floor parts hang just below the floor pan, where they show from the lift. Aftermarket
// grades are anodised by tier (blue, gold, red), like the parts on the engine stand. Chassis
// space: +X right, +Y forward, +Z up, metres. Workshop only: nothing here is drawn while driving.
import * as THREE from "three";
import type { SlotView } from "../../../ui-kit/src/garage/types";
import type { WheelInfo } from "../wasm/sim";
import type { CarMeshJson, MeshPartJson } from "../render/panels";

const std = (color: number, roughness: number, metalness: number) => new THREE.MeshStandardMaterial({ color, roughness, metalness });

/** Shared materials (made once). */
const M = {
  case: std(0x1c1d20, 0.55, 0.1),
  caseTop: std(0x3a3d42, 0.5, 0.1),
  red: std(0xc8241e, 0.4, 0.2),
  lead: std(0x8a8d90, 0.35, 0.8),
  iron: std(0x6a6f76, 0.6, 0.55),
  alu: std(0xb4b9bf, 0.38, 0.75),
  steel: std(0x8f949a, 0.32, 0.85),
  black: std(0x141518, 0.6, 0.3),
  rubber: std(0x0e0e0e, 0.95, 0),
  tank: std(0x9aa0a6, 0.45, 0.7),
  weld: std(0xa8642c, 0.55, 0.6),
  label: std(0xe0e2e4, 0.6, 0),
  glass: new THREE.MeshStandardMaterial({ color: 0x1d2a33, roughness: 0.08, metalness: 0.3, transparent: true, opacity: 0.55, side: THREE.DoubleSide }),
  tier: [std(0x2f6fd6, 0.3, 0.7), std(0xd6a12f, 0.3, 0.75), std(0xc63a2f, 0.3, 0.7)],
};

/** How the car is laid out (engine place and driven wheels). */
export interface Layout {
  /** Engine ahead of the cabin (false: behind it, mid-engined). */
  frontEngine: boolean;
  /** Driven axles. */
  front: boolean;
  rear: boolean;
  /** Engine across the car with a transaxle beside it (front-drive based). */
  transverse: boolean;
}

/** Layout from the car definition's `drivetrain.layout` and the engine's place. */
export function layoutOf(drive: unknown, frontEngine: boolean): Layout {
  const key = typeof drive === "string" ? drive : drive && typeof drive === "object" ? Object.keys(drive)[0] : "Rwd";
  const inner = drive && typeof drive === "object" ? (drive as Record<string, Record<string, number>>)[key ?? ""] : undefined;
  switch (key) {
    case "Fwd":
      return { frontEngine, front: true, rear: false, transverse: frontEngine };
    case "Awd":
      return { frontEngine, front: true, rear: true, transverse: false };
    case "AwdOnDemand":
      // Front-biased on-demand systems sit on a transverse front-drive base.
      return { frontEngine, front: true, rear: true, transverse: frontEngine && (inner?.base_front_split ?? 0) > 0.5 };
    default:
      return { frontEngine, front: false, rear: true, transverse: false };
  }
}

/** Box of size (x, y, z) at (cx, cy, cz). */
function box(x: number, y: number, z: number, m: THREE.Material, cx: number, cy: number, cz: number): THREE.Mesh {
  const b = new THREE.Mesh(new THREE.BoxGeometry(x, y, z), m);
  b.position.set(cx, cy, cz);
  return b;
}

/** Cylinder of radius `r` and length `len` along `axis`, centred at (cx, cy, cz). */
function cyl(r: number, len: number, m: THREE.Material, axis: "x" | "y" | "z", cx: number, cy: number, cz: number, r2 = r, seg = 18): THREE.Mesh {
  const c = new THREE.Mesh(new THREE.CylinderGeometry(r2, r, len, seg), m);
  if (axis === "x") c.rotation.z = -Math.PI / 2;
  if (axis === "z") c.rotation.x = Math.PI / 2;
  c.position.set(cx, cy, cz);
  return c;
}

/** Round bar from a to b. */
function bar(a: THREE.Vector3, b: THREE.Vector3, r: number, m: THREE.Material): THREE.Mesh {
  const len = a.distanceTo(b);
  const t = new THREE.Mesh(new THREE.CylinderGeometry(r, r, len, 12), m);
  t.position.copy(a).add(b).multiplyScalar(0.5);
  t.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), b.clone().sub(a).normalize());
  return t;
}

const V = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);

/** Car geometry the hardware is placed from. */
export class Bay {
  /** Body without wheels. */
  readonly body = new THREE.Box3();
  /** Engine-bay engine parts (any region). */
  readonly engine: THREE.Box3;
  /** Floor pan underside height. */
  readonly floorZ: number;
  readonly cabin: THREE.Box3;
  readonly bumperFront: THREE.Box3;
  readonly bumperRear: THREE.Box3;
  readonly frontY: number;
  readonly rearY: number;
  /** Wheel centre height and half track (front, rear). */
  readonly wheelZ: number;
  readonly halfTrackF: number;
  readonly halfTrackR: number;
  readonly halfW: number;
  private readonly parts = new Map<string, MeshPartJson>();

  constructor(mesh: CarMeshJson, wheels: WheelInfo[]) {
    const v = new THREE.Vector3();
    const boxOf = (p: MeshPartJson) => {
      const b = new THREE.Box3();
      const a = p.positions_mm;
      for (let i = 0; i < a.length; i += 3) b.expandByPoint(v.set(a[i] * 1e-3, a[i + 1] * 1e-3, a[i + 2] * 1e-3));
      return b;
    };
    const union = (pick: (p: MeshPartJson) => boolean) => {
      let out: THREE.Box3 | null = null;
      for (const p of mesh.parts) if (p.wheel == null && p.positions_mm.length && pick(p)) out = (out ?? new THREE.Box3()).union(boxOf(p));
      return out;
    };
    for (const p of mesh.parts) {
      if (p.wheel != null) continue;
      this.parts.set(p.name, p);
      if (p.kind === "panel" || p.kind === "body") this.body.union(boxOf(p));
    }
    if (this.body.isEmpty()) this.body.set(V(-0.9, -2.3, 0.15), V(0.9, 2.3, 1.4));
    this.halfW = Math.max(-this.body.min.x, this.body.max.x);
    const front = wheels.filter((w) => w.position_m[1] > 0);
    const rear = wheels.filter((w) => w.position_m[1] <= 0);
    this.frontY = front[0]?.position_m[1] ?? 1.3;
    this.rearY = rear[0]?.position_m[1] ?? -1.3;
    this.wheelZ = wheels[0]?.position_m[2] ?? 0.32;
    this.halfTrackF = Math.abs(front[0]?.position_m[0] ?? 0.75);
    this.halfTrackR = Math.abs(rear[0]?.position_m[0] ?? 0.75);
    const isBlock = (p: MeshPartJson) =>
      p.kind === "engine" && !/radiator|intercooler|charge_pipes|headers/.test(p.name) && p.region !== null;
    this.engine =
      union(isBlock) ?? new THREE.Box3(V(-0.3, this.frontY - 0.3, this.wheelZ - 0.15), V(0.3, this.frontY + 0.5, this.wheelZ + 0.35));
    this.floorZ = union((p) => p.name === "underbody")?.min.z ?? this.body.min.z;
    this.cabin =
      union((p) => p.kind === "interior") ?? new THREE.Box3(V(-this.halfW + 0.15, -1.0, 0.22), V(this.halfW - 0.15, 0.9, 0.85));
    this.bumperFront = union((p) => p.kind === "panel" && p.region === "FrontBumper") ?? new THREE.Box3(V(-0.8, this.body.max.y - 0.25, 0.15), V(0.8, this.body.max.y, 0.6));
    this.bumperRear = union((p) => p.kind === "panel" && p.region === "RearBumper") ?? new THREE.Box3(V(-0.8, this.body.min.y, 0.15), V(0.8, this.body.min.y + 0.25, 0.6));
  }

  /** The model has a mesh part called `name`. */
  has(name: string): boolean {
    return this.parts.has(name);
  }

  /** Highest vertex of part `name` near (x, y) in plan, or `fallback`. */
  topOf(name: string, x: number, y: number, fallback: number, reach = 0.2): number {
    const p = this.parts.get(name);
    if (!p) return fallback;
    let best = -Infinity;
    const a = p.positions_mm;
    for (let i = 0; i < a.length; i += 3) {
      const dx = a[i] * 1e-3 - x;
      const dy = a[i + 1] * 1e-3 - y;
      if (dx * dx + dy * dy <= reach * reach) best = Math.max(best, a[i + 2] * 1e-3);
    }
    return Number.isFinite(best) ? best : fallback;
  }

  /** Lowest vertex of part `name` near (x, y) in plan (the underside of a lid), or `fallback`. */
  bottomOf(name: string, x: number, y: number, fallback: number, reach = 0.2): number {
    const p = this.parts.get(name);
    if (!p) return fallback;
    let best = Infinity;
    const a = p.positions_mm;
    for (let i = 0; i < a.length; i += 3) {
      const dx = a[i] * 1e-3 - x;
      const dy = a[i + 1] * 1e-3 - y;
      if (dx * dx + dy * dy <= reach * reach) best = Math.min(best, a[i + 2] * 1e-3);
    }
    return Number.isFinite(best) ? best : fallback;
  }
}

/** What a builder gets. */
interface Ctx {
  bay: Bay;
  lay: Layout;
  /** Material for the slot's fitted grade: tier colour for aftermarket, else `stock`. */
  grade(stock: THREE.Material): THREE.Material;
  /** Grade id fitted (null = stock). */
  id: string | null;
}

/** Where the gearbox ends (output flange), for shafts and the transfer case. */
function gearboxEnd(c: Ctx): THREE.Vector3 {
  const { bay, lay } = c;
  const e = bay.engine;
  if (lay.transverse) return V(e.min.x - 0.12, bay.frontY, bay.wheelZ);
  if (lay.frontEngine) return V(0, e.min.y - 0.6, bay.floorZ + 0.02);
  return V(0, e.min.y - 0.42, bay.wheelZ);
}

/** Centre of the driven axle's differential. */
function diffAt(c: Ctx, axle: "front" | "rear"): THREE.Vector3 {
  const { bay, lay } = c;
  if (axle === "front") {
    // Transaxle: the diff is beside the gearbox at the front axle; else a front diff by the sump.
    return lay.transverse ? V(bay.engine.min.x - 0.02, bay.frontY - 0.08, bay.wheelZ - 0.04) : V(0.08, bay.frontY, bay.wheelZ - 0.06);
  }
  return lay.frontEngine ? V(0, bay.rearY, Math.max(bay.wheelZ - 0.05, bay.floorZ + 0.04)) : V(0, bay.rearY + 0.05, bay.wheelZ - 0.04);
}

type Builder = (c: Ctx) => THREE.Object3D[];

/** Battery: case, lid, posts and hold-down; a light aftermarket battery is smaller, in tier colour. */
const battery: Builder = (c) => {
  const { bay, lay } = c;
  const light = c.id !== null;
  const [w, l, h] = light ? [0.19, 0.13, 0.15] : [0.26, 0.17, 0.19];
  const e = bay.engine;
  let at: THREE.Vector3;
  if (lay.frontEngine) {
    // Engine-bay corner on the left, between the block and the fender, behind the radiator.
    const x = -Math.min(Math.max(-e.min.x, e.max.x) + 0.06 + w / 2, bay.halfW - 0.2 - w / 2);
    const y = Math.min(e.max.y - 0.2, bay.frontY + 0.25);
    const lid = bay.bottomOf("hood", x, y, e.max.z + 0.1);
    at = V(x, y, Math.min(bay.wheelZ + 0.14 + h / 2, lid - 0.04 - h / 2));
  } else {
    // Mid-engined: in the front luggage bay.
    const lid = bay.bottomOf("hood", 0.3, bay.frontY + 0.15, bay.wheelZ + 0.4);
    at = V(0.3, bay.frontY + 0.15, Math.min(bay.wheelZ + 0.05 + h / 2, lid - 0.04 - h / 2));
  }
  const g: THREE.Object3D[] = [
    box(w, l, h * 0.88, c.grade(M.case), at.x, at.y, at.z - h * 0.06),
    box(w * 1.01, l * 1.01, h * 0.12, light ? M.black : M.caseTop, at.x, at.y, at.z + h * 0.44),
    cyl(0.013, 0.03, M.red, "z", at.x - w * 0.32, at.y + l * 0.25, at.z + h / 2 + 0.015),
    cyl(0.013, 0.03, M.lead, "z", at.x + w * 0.32, at.y + l * 0.25, at.z + h / 2 + 0.015),
    box(0.025, l * 1.1, 0.012, M.black, at.x, at.y, at.z + h / 2 + 0.008),
    box(w * 0.4, 0.004, h * 0.3, M.label, at.x, at.y - l / 2 - 0.002, at.z),
  ];
  return g;
};

/** Engine computer: finned aluminium box with its loom plug, on the firewall (mid: front bay). */
const ecu: Builder = (c) => {
  const { bay, lay } = c;
  const e = bay.engine;
  let at: THREE.Vector3;
  if (lay.frontEngine) {
    const x = Math.min(Math.max(-e.min.x, e.max.x) + 0.12, bay.halfW - 0.3);
    const y = e.min.y + 0.12;
    const lid = bay.bottomOf("hood", x, y, e.max.z + 0.1);
    at = V(x, y, Math.min(e.max.z - 0.05, lid - 0.08));
  } else {
    const lid = bay.bottomOf("hood", -0.3, bay.frontY + 0.1, bay.wheelZ + 0.4);
    at = V(-0.3, bay.frontY + 0.1, Math.min(bay.wheelZ + 0.1, lid - 0.06));
  }
  const m = c.grade(M.alu);
  const out: THREE.Object3D[] = [box(0.2, 0.15, 0.04, m, at.x, at.y, at.z)];
  for (let k = -3; k <= 3; k++) out.push(box(0.008, 0.13, 0.015, m, at.x + k * 0.026, at.y, at.z + 0.027));
  out.push(box(0.07, 0.04, 0.03, M.black, at.x, at.y + 0.09, at.z));
  out.push(bar(V(at.x, at.y + 0.11, at.z), V(at.x - 0.1, at.y + 0.25, at.z - 0.05), 0.012, M.black));
  return out;
};

/** Air intake: filter box and the duct to the throttle. */
const intake: Builder = (c) => {
  const { bay, lay } = c;
  const e = bay.engine;
  const x = lay.frontEngine ? Math.min(e.max.x + 0.1, bay.halfW - 0.3) : 0;
  const y = lay.frontEngine ? e.max.y - 0.12 : e.max.y - 0.1;
  const lid = bay.bottomOf(lay.frontEngine ? "hood" : "trunk", x, y, e.max.z + 0.1);
  const z = Math.min(e.max.z - 0.05, lid - 0.12);
  const m = c.grade(M.black);
  return [
    box(0.24, 0.18, 0.12, m, x, y, z),
    box(0.25, 0.19, 0.02, M.iron, x, y, z + 0.035),
    bar(V(x - 0.1, y - 0.05, z), V((e.min.x + e.max.x) / 2 + 0.05, (e.min.y + e.max.y) / 2, e.max.z - 0.06), 0.04, M.rubber),
  ];
};

/** Fuel rail with injectors along the top of the engine. */
const fuelSystem: Builder = (c) => {
  const e = c.bay.engine;
  const x = (e.min.x + e.max.x) / 2 + 0.07;
  const len = Math.max(0.3, (e.max.y - e.min.y) * 0.45);
  const y = (e.min.y + e.max.y) / 2;
  const z = e.max.z - 0.03;
  const m = c.grade(M.steel);
  const out: THREE.Object3D[] = [cyl(0.012, len, m, "y", x, y, z)];
  for (let k = 0; k < 4; k++) out.push(cyl(0.008, 0.06, M.black, "z", x, y - len / 2 + (k + 0.5) * (len / 4), z - 0.035));
  return out;
};

/** Gearbox: bell housing and case (tunnel box, transaxle beside a transverse engine, or behind a mid engine). */
const gearbox: Builder = (c) => {
  const { bay, lay } = c;
  const e = bay.engine;
  const m = c.grade(M.alu);
  if (lay.transverse) {
    const x = e.min.x - 0.1;
    return [
      cyl(0.16, 0.12, m, "x", x + 0.04, bay.frontY - 0.02, bay.wheelZ + 0.04, 0.12),
      box(0.22, 0.28, 0.24, m, x - 0.12, bay.frontY - 0.02, bay.wheelZ + 0.03),
      box(0.06, 0.06, 0.06, M.iron, x - 0.12, bay.frontY - 0.02, bay.wheelZ + 0.19),
    ];
  }
  if (lay.frontEngine) {
    const z = bay.floorZ + 0.06;
    const y0 = e.min.y;
    return [
      cyl(0.13, 0.16, m, "y", 0, y0 - 0.08, z + 0.02, 0.17),
      cyl(0.11, 0.3, m, "y", 0, y0 - 0.31, z, 0.12),
      cyl(0.07, 0.14, m, "y", 0, y0 - 0.53, z - 0.01, 0.09),
    ];
  }
  const y0 = e.min.y;
  return [
    cyl(0.14, 0.12, m, "y", 0, y0 - 0.06, bay.wheelZ + 0.02, 0.17),
    box(0.3, 0.32, 0.22, m, 0, y0 - 0.28, bay.wheelZ),
  ];
};

/** Prop shaft (front engine, rear drive) or half shafts (transaxles). */
const driveshaft: Builder = (c) => {
  const { bay, lay } = c;
  const m = c.grade(M.steel);
  const out: THREE.Object3D[] = [];
  const halfShafts = (from: THREE.Vector3, y: number, track: number) => {
    for (const sx of [-1, 1]) {
      const hub = V(sx * (track - 0.12), y, bay.wheelZ);
      out.push(bar(V(from.x + sx * 0.06, from.y, from.z), hub, 0.018, m));
      out.push(cyl(0.035, 0.07, M.rubber, "x", from.x + sx * 0.1, from.y, from.z));
      out.push(cyl(0.035, 0.07, M.rubber, "x", hub.x - sx * 0.05, hub.y, hub.z));
    }
  };
  const prop = (a: THREE.Vector3, b: THREE.Vector3) => {
    const mid = V((a.x + b.x) / 2, (a.y + b.y) / 2, (a.z + b.z) / 2);
    out.push(bar(a, mid, 0.032, m), bar(mid, b, 0.032, m));
    out.push(box(0.09, 0.05, 0.05, M.rubber, mid.x, mid.y, mid.z + 0.04));
  };
  const rear = diffAt(c, "rear");
  const front = diffAt(c, "front");
  if (lay.rear && lay.frontEngine) {
    // Prop shaft from the gearbox tail (a transverse engine's take-off) back to the rear diff.
    const a = lay.transverse ? V(bay.engine.min.x + 0.1, bay.frontY - 0.25, bay.floorZ + 0.02) : gearboxEnd(c);
    prop(a, V(rear.x, rear.y + 0.14, rear.z));
  }
  if (lay.front && !lay.frontEngine) prop(V(0, bay.engine.max.y + 0.05, bay.wheelZ - 0.02), V(front.x, front.y - 0.1, front.z));
  if (lay.front) halfShafts(front, bay.frontY - 0.03, bay.halfTrackF);
  if (lay.rear) halfShafts(rear, bay.rearY, bay.halfTrackR);
  return out;
};

/** Differential: the driven axle's pumpkin with its cover (LSD covers finned, in tier colour; welded: bead). */
const differential: Builder = (c) => {
  const { lay } = c;
  const at = diffAt(c, lay.rear ? "rear" : "front");
  const out: THREE.Object3D[] = [];
  const pumpkin = new THREE.Mesh(new THREE.SphereGeometry(0.12, 18, 12), M.iron);
  pumpkin.scale.set(1.15, 0.9, 1);
  pumpkin.position.copy(at);
  out.push(pumpkin);
  if (!lay.transverse || lay.rear) {
    // Rear-facing cover with a bolt ring, and the pinion snout pointing at the prop shaft.
    const back = lay.rear ? -1 : 1;
    const cover = c.grade(M.iron);
    out.push(cyl(0.11, 0.035, cover, "y", at.x, at.y + back * 0.1, at.z));
    if (c.id) for (let k = -2; k <= 2; k++) out.push(box(0.012, 0.02, 0.15, cover, at.x + k * 0.035, at.y + back * 0.125, at.z));
    if (c.id?.includes("welded")) out.push(new THREE.Mesh(new THREE.TorusGeometry(0.1, 0.008, 6, 24), M.weld).translateX(at.x).translateY(at.y + back * 0.12).translateZ(at.z).rotateX(Math.PI / 2));
    for (let k = 0; k < 10; k++) {
      const a = (k / 10) * Math.PI * 2;
      out.push(cyl(0.007, 0.015, M.steel, "y", at.x + Math.cos(a) * 0.095, at.y + back * 0.12, at.z + Math.sin(a) * 0.095));
    }
    out.push(cyl(0.045, 0.14, M.iron, "y", at.x, at.y - back * 0.14, at.z, 0.06));
  } else if (c.id) {
    out.push(cyl(0.08, 0.03, c.grade(M.iron), "x", at.x - 0.11, at.y, at.z));
  }
  // Axle tubes out to the hubs on a beam axle (trucks), stub flanges otherwise.
  const track = lay.rear ? c.bay.halfTrackR : c.bay.halfTrackF;
  for (const sx of [-1, 1]) out.push(cyl(0.04, 0.08, M.iron, "x", at.x + sx * 0.14, at.y, at.z));
  if (lay.rear && lay.frontEngine && c.bay.wheelZ > 0.36) for (const sx of [-1, 1]) out.push(bar(V(at.x + sx * 0.16, at.y, at.z), V(sx * (track - 0.15), at.y, c.bay.wheelZ), 0.04, M.iron));
  // Four-wheel drive: the second diff on the front axle.
  if (lay.front && lay.rear) {
    const f = diffAt(c, "front");
    const front = new THREE.Mesh(new THREE.SphereGeometry(0.1, 16, 10), M.iron);
    front.position.copy(f);
    out.push(front, cyl(0.09, 0.03, c.grade(M.iron), "y", f.x, f.y + 0.085, f.z));
  }
  return out;
};

/** Transfer case: box at the gearbox tail with the front prop shaft. */
const transferCase: Builder = (c) => {
  const a = gearboxEnd(c);
  const f = diffAt(c, "front");
  const m = c.grade(M.alu);
  return [box(0.2, 0.18, 0.18, m, a.x + 0.06, a.y + 0.06, a.z), bar(V(a.x + 0.12, a.y + 0.12, a.z), V(f.x, f.y - 0.12, f.z), 0.025, M.steel)];
};

/** Fuel tank: flat tank under the floor ahead of (mid engine: behind) the rear axle, with filler neck. */
const fuelTank: Builder = (c) => {
  const { bay, lay } = c;
  const w = Math.min(0.8, bay.halfTrackR * 2 - 0.55);
  const l = 0.42;
  const h = 0.2;
  const y = lay.frontEngine ? bay.rearY - 0.12 - (lay.rear ? 0.45 : 0.3) : bay.frontY - 0.55;
  const z = bay.floorZ + 0.04;
  const m = c.grade(M.tank);
  const out: THREE.Object3D[] = [box(w, l, h, m, 0, y, z)];
  for (const k of [-1, 1]) out.push(box(0.03, l + 0.04, 0.02, M.black, k * w * 0.3, y, z - h / 2 - 0.005));
  const side = bay.has("fuel_filler_r") ? 1 : -1;
  out.push(bar(V(side * w * 0.4, y, z + 0.05), V(side * (bay.halfW - 0.08), y + 0.05, z + 0.4), 0.03, M.rubber));
  return out;
};

/** Steering rack across the front axle, with boots and tie rods. */
const steeringRack: Builder = (c) => {
  const { bay } = c;
  const y = bay.frontY - 0.15;
  const z = bay.wheelZ + 0.02;
  const half = bay.halfTrackF - 0.3;
  const m = c.grade(M.alu);
  const out: THREE.Object3D[] = [cyl(0.03, half * 2, m, "x", 0, y, z), box(0.08, 0.08, 0.08, m, -0.2, y, z + 0.02)];
  for (const sx of [-1, 1]) {
    out.push(cyl(0.035, 0.1, M.rubber, "x", sx * (half + 0.03), y, z));
    out.push(bar(V(sx * (half + 0.08), y, z), V(sx * (bay.halfTrackF - 0.1), y + 0.02, z), 0.012, M.steel));
  }
  out.push(bar(V(-0.2, y, z + 0.05), V(-0.35, bay.cabin.max.y - 0.2, bay.cabin.min.z + 0.4), 0.015, M.steel));
  return out;
};

/** Gear lever on the centre console (stock; aftermarket shifters are cosmetic pieces). */
const shifter: Builder = (c) => {
  const cab = c.bay.cabin;
  const y = (cab.min.y + cab.max.y) / 2 + 0.25;
  const z = cab.min.z + 0.2;
  return [
    box(0.1, 0.12, 0.04, M.black, 0, y, z),
    cyl(0.04, 0.05, M.rubber, "z", 0, y, z + 0.04, 0.025),
    cyl(0.008, 0.16, c.grade(M.steel), "z", 0, y, z + 0.12),
    new THREE.Mesh(new THREE.SphereGeometry(0.025, 12, 8), M.black).translateY(y).translateZ(z + 0.21),
  ];
};

/** Crash beam inside a bumper (it shows once the bumper cover is off). */
const beam = (front: boolean): Builder => (c) => {
  const b = front ? c.bay.bumperFront : c.bay.bumperRear;
  const w = (b.max.x - b.min.x) * 0.84;
  const y = front ? b.min.y + 0.06 : b.max.y - 0.06;
  const z = (b.min.z + b.max.z) / 2 - 0.03;
  const m = c.grade(M.steel);
  const back = front ? -1 : 1;
  const out: THREE.Object3D[] = [box(w, 0.07, 0.11, m, 0, y, z)];
  for (const sx of [-1, 1]) out.push(box(0.09, 0.16, 0.09, M.iron, sx * w * 0.36, y + back * 0.1, z));
  return out;
};

/** Radiator core behind the front bumper (mid-engined cars have no bay mesh for it). */
const radiator: Builder = (c) => {
  const b = c.bay.bumperFront;
  const y = b.min.y - 0.05;
  const w = Math.min(0.7, (b.max.x - b.min.x) * 0.6);
  const z = (b.min.z + b.max.z) / 2 + 0.05;
  return [box(w, 0.05, 0.38, c.grade(M.alu), 0, y, z), box(w + 0.04, 0.06, 0.04, M.black, 0, y, z + 0.21), box(w + 0.04, 0.06, 0.04, M.black, 0, y, z - 0.21)];
};

/** Cab back window, for models whose mesh has no rear glass (pickup cabs). */
const rearGlass: Builder = (c) => {
  const cab = c.bay.cabin;
  const w = (cab.max.x - cab.min.x) * 0.8;
  const top = c.bay.body.max.z - 0.06;
  const y = cab.min.y - 0.02;
  return [box(w, 0.012, 0.34, M.glass, 0, y, top - 0.2), box(w + 0.04, 0.02, 0.025, M.rubber, 0, y, top - 0.02), box(w + 0.04, 0.02, 0.025, M.rubber, 0, y, top - 0.38)];
};

/** Builders by slot. Slots not here are drawn by the mesh or a cosmetic piece, or live inside
 * another part (engine internals). */
const BUILDERS: Record<string, Builder> = {
  battery,
  ecu,
  intake,
  fuel_system: fuelSystem,
  gearbox,
  driveshaft,
  differential,
  transfer_case: transferCase,
  fuel_tank: fuelTank,
  steering_rack: steeringRack,
  shifter,
  front_bumper_beam: beam(true),
  rear_bumper_beam: beam(false),
  radiator,
  rear_glass: rearGlass,
};

/** Slots this module can draw. */
export const HARDWARE_SLOTS: readonly string[] = Object.keys(BUILDERS);

/**
 * The hardware of every slot in `slots` that holds a part and is in `draw` (the caller passes
 * the slots nothing else draws), as one group in chassis space.
 */
export function buildHardware(bay: Bay, lay: Layout, slots: readonly SlotView[], draw: ReadonlySet<string>): THREE.Group {
  const root = new THREE.Group();
  root.name = "workshop-hardware";
  for (const s of slots) {
    const b = BUILDERS[s.id];
    if (!b || !s.fitted || !draw.has(s.id)) continue;
    const tier = s.fitted.grade ? M.tier[Math.min(2, Math.max(0, s.fitted.tier - 1))] : null;
    const ctx: Ctx = { bay, lay, id: s.fitted.grade, grade: (stock) => tier ?? stock };
    const g = new THREE.Group();
    g.name = `hw:${s.id}`;
    g.userData.slot = s.id;
    for (const o of b(ctx)) g.add(o);
    root.add(g);
  }
  // Inspection lamps under the floor pan: the lift's work lights, so the underside reads.
  for (const y of [bay.frontY * 0.6, bay.rearY * 0.6]) {
    const lamp = new THREE.PointLight(0xfff4e0, 6, 3.5, 2);
    lamp.position.set(0, y, bay.floorZ - 0.75);
    root.add(lamp);
  }
  return root;
}
