// Garage workshop: the engine on its stand, built in code from the loose engine's internals
// (wp_economy `StandView`). Every internal is its own group tagged `userData.slot`, so a part
// that is out of the engine is simply not drawn, and the parts under it show: take the head off
// and you look down the bores at the pistons; take the pan off and the crank and rods show.
//
// Engine frame: crank along +Y (front of the engine = timing end at +Y), +Z up, +X right; the
// crank centre is the origin. Inline four for the stock engines, a V8 for the swap engine.
// Aftermarket parts are anodised (blue / gold / red by tier) so upgrades stand out.
import * as THREE from "three";
import type { StandView } from "../../../ui-kit/src/garage/types";

const mats = (() => {
  const m = (color: number, roughness: number, metalness: number) => new THREE.MeshStandardMaterial({ color, roughness, metalness });
  return {
    iron: m(0x4a4d52, 0.7, 0.45),
    alu: m(0xb8bcc2, 0.4, 0.75),
    steel: m(0x9ea3a8, 0.3, 0.9),
    dark: m(0x202225, 0.6, 0.3),
    gasket: m(0x6d6a63, 0.8, 0.2),
    cover: m(0x2b2d31, 0.45, 0.4),
    rubber: m(0x111111, 0.9, 0),
    friction: m(0x5b4a3a, 0.95, 0),
    rust: m(0x7a4a2c, 0.8, 0.3),
    standBlue: m(0x1f5fa8, 0.45, 0.3),
    tier: [m(0x2f6fd6, 0.3, 0.7), m(0xd6a12f, 0.3, 0.75), m(0xc63a2f, 0.3, 0.7)],
  };
})();

function box(w: number, l: number, h: number, m: THREE.Material, x = 0, y = 0, z = 0): THREE.Mesh {
  const b = new THREE.Mesh(new THREE.BoxGeometry(w, l, h), m);
  b.position.set(x, y, z);
  return b;
}

/** Cylinder of radius `r`, length `len` along `axis`, centred at (x, y, z). */
function cyl(r: number, len: number, m: THREE.Material, axis: "x" | "y" | "z", x = 0, y = 0, z = 0, open = false, seg = 20): THREE.Mesh {
  const c = new THREE.Mesh(new THREE.CylinderGeometry(r, r, len, seg, 1, open), m);
  if (axis === "z") c.rotation.x = Math.PI / 2;
  if (axis === "x") c.rotation.z = Math.PI / 2;
  c.position.set(x, y, z);
  return c;
}

/** A tube through `pts` (engine frame). */
function tube(pts: [number, number, number][], r: number, m: THREE.Material): THREE.Mesh {
  const curve = new THREE.CatmullRomCurve3(pts.map((p) => new THREE.Vector3(...p)));
  return new THREE.Mesh(new THREE.TubeGeometry(curve, 24, r, 10), m);
}

/** Size of the engine for a layout. */
interface Layout {
  /** Cylinders per bank. */
  n: number;
  /** Bank tilt angles from vertical, rad (one bank: [0]). */
  banks: number[];
  bore: number;
  pitch: number;
  /** Deck height above the crank centre. */
  deck: number;
}

const INLINE4: Layout = { n: 4, banks: [0], bore: 0.043, pitch: 0.098, deck: 0.27 };
const V8: Layout = { n: 4, banks: [-0.62, 0.62], bore: 0.05, pitch: 0.112, deck: 0.27 };

/** The loose engine `stand` as a model; parts are groups with `userData.slot`. */
export function buildEngine(stand: StandView, grade: string | null): THREE.Group {
  const lay = grade?.includes("v8") ? V8 : INLINE4;
  const root = new THREE.Group();
  root.name = "engine-model";
  const fitted = new Map(stand.slots.filter((s) => s.fitted).map((s) => [s.id, s.fitted!]));
  const has = (slot: string) => fitted.has(slot);
  const tierMat = (slot: string, base: THREE.Material) => {
    const f = fitted.get(slot);
    return f?.grade ? mats.tier[Math.min(2, Math.max(0, f.tier - 1))] : base;
  };
  const worn = (slot: string, base: THREE.Material) => ((fitted.get(slot)?.condition ?? 1) < 0.6 ? mats.rust : base);
  const part = (slot: string) => {
    const g = new THREE.Group();
    g.userData.slot = slot;
    root.add(g);
    return g;
  };
  const len = lay.n * lay.pitch + 0.06;
  const ys = Array.from({ length: lay.n }, (_, i) => (i - (lay.n - 1) / 2) * lay.pitch);
  /** Bank frame: rotate about the crank axis by `a` (positive tilts the bank to +X). */
  const bankGroup = (parent: THREE.Object3D, a: number) => {
    const g = new THREE.Group();
    g.rotation.y = a;
    parent.add(g);
    return g;
  };

  // Block (always there: it is the part on the stand). Open top and bottom: the bores are
  // liners, so pistons show with the head off and the crank with the pan off.
  const block = part("engine");
  const bw = lay.banks.length > 1 ? 0.42 : 0.26;
  const skirtZ = -0.07;
  for (const sx of [-1, 1]) block.add(box(0.025, len, lay.deck * 0.55, mats.iron, sx * (bw / 2), 0, skirtZ + lay.deck * 0.27));
  for (const sy of [-1, 1]) block.add(box(bw, 0.025, lay.deck - skirtZ, mats.iron, 0, sy * (len / 2), (lay.deck + skirtZ) / 2));
  for (const a of lay.banks) {
    const b = bankGroup(block, a);
    for (const y of ys) b.add(cyl(lay.bore + 0.006, lay.deck * 0.62, mats.iron, "z", 0, y, lay.deck * 0.69, true));
    // Deck rails either side of the bores.
    for (const sx of [-1, 1]) b.add(box(0.02, len, 0.03, mats.iron, sx * (lay.bore + 0.018), 0, lay.deck - 0.015));
  }
  // Main bearing webs under the crank.
  for (let i = 0; i <= lay.n; i++) block.add(box(bw * 0.8, 0.018, 0.05, mats.iron, 0, (i - lay.n / 2) * lay.pitch, -0.03));

  // Crankshaft: main journal, a throw and counterweight per cylinder.
  if (has("crankshaft")) {
    const g = part("crankshaft");
    const m = tierMat("crankshaft", worn("crankshaft", mats.steel));
    g.add(cyl(0.022, len + 0.08, m, "y"));
    ys.forEach((y, i) => {
      const up = i % 2 ? 1 : -1;
      g.add(box(0.07, 0.02, 0.09, m, 0, y - 0.025, up * 0.025));
      g.add(box(0.07, 0.02, 0.09, m, 0, y + 0.025, up * 0.025));
      g.add(cyl(0.02, 0.06, m, "y", 0, y, up * 0.045));
    });
    g.add(cyl(0.05, 0.03, m, "y", 0, len / 2 + 0.06, 0));
  }
  // Rods and pistons, per bank.
  const throwZ = (i: number) => (i % 2 ? 1 : -1) * 0.045;
  for (const a of lay.banks) {
    if (has("con_rods")) {
      const g = part("con_rods");
      const b = bankGroup(g, a);
      const m = tierMat("con_rods", worn("con_rods", mats.steel));
      ys.forEach((y, i) => {
        const top = lay.deck - 0.09;
        const bottom = throwZ(i);
        b.add(box(0.016, 0.012, top - bottom, m, 0, y + (a > 0 ? 0.012 : 0), (top + bottom) / 2));
      });
    }
    if (has("pistons")) {
      const g = part("pistons");
      const b = bankGroup(g, a);
      const m = tierMat("pistons", worn("pistons", mats.alu));
      ys.forEach((y, i) => {
        const z = lay.deck - 0.06 + (i % 2 ? -0.02 : 0);
        b.add(cyl(lay.bore - 0.002, 0.05, m, "z", 0, y, z));
        b.add(cyl(lay.bore - 0.001, 0.006, mats.dark, "z", 0, y, z + 0.016));
      });
    }
  }
  // Oil pan and pump.
  if (has("oil_pan")) {
    const g = part("oil_pan");
    const m = worn("oil_pan", mats.cover);
    g.add(box(bw - 0.02, len, 0.02, m, 0, 0, -0.08));
    g.add(box(bw * 0.8, len * 0.55, 0.12, m, 0, -len * 0.18, -0.15));
    for (const sx of [-1, 1]) g.add(box(0.012, len, 0.05, m, sx * (bw / 2 - 0.016), 0, -0.1));
  }
  if (has("oil_pump")) {
    const g = part("oil_pump");
    g.add(box(0.1, 0.05, 0.06, tierMat("oil_pump", mats.alu), 0, len / 2 - 0.05, -0.09));
    g.add(tube([[0, len / 2 - 0.08, -0.1], [0, len / 2 - 0.18, -0.16], [0, 0, -0.18]], 0.01, mats.steel));
  }

  // Top end, per bank: gasket, head, valves, cams, valve cover.
  for (const a of lay.banks) {
    const top = lay.deck;
    if (has("head_gasket")) {
      const b = bankGroup(part("head_gasket"), a);
      b.add(box(lay.bore * 2 + 0.05, len, 0.004, tierMat("head_gasket", mats.gasket), 0, 0, top + 0.002));
    }
    if (has("cylinder_head")) {
      const b = bankGroup(part("cylinder_head"), a);
      const m = tierMat("cylinder_head", worn("cylinder_head", mats.alu));
      b.add(box(lay.bore * 2 + 0.05, len, 0.1, m, 0, 0, top + 0.054));
      for (const sx of [-1, 1]) for (const y of ys) b.add(box(0.02, 0.05, 0.04, m, sx * (lay.bore + 0.035), y, top + 0.05));
    }
    if (has("valvetrain")) {
      const b = bankGroup(part("valvetrain"), a);
      const m = tierMat("valvetrain", worn("valvetrain", mats.steel));
      for (const y of ys) for (const sx of [-1, 1]) for (const dy of [-0.016, 0.016]) b.add(cyl(0.009, 0.035, m, "z", sx * 0.022, y + dy, top + 0.12, false, 10));
    }
    if (has("camshafts")) {
      const b = bankGroup(part("camshafts"), a);
      const m = tierMat("camshafts", worn("camshafts", mats.steel));
      for (const sx of [-1, 1]) {
        b.add(cyl(0.011, len, m, "y", sx * 0.022, 0, top + 0.155));
        for (const y of ys) for (const dy of [-0.016, 0.016]) b.add(box(0.018, 0.01, 0.026, m, sx * 0.022, y + dy, top + 0.15));
      }
      // Cam sprockets at the front.
      for (const sx of [-1, 1]) b.add(cyl(0.04, 0.01, mats.steel, "y", sx * 0.022, len / 2 + 0.01, top + 0.155));
    }
    if (has("valve_cover")) {
      const b = bankGroup(part("valve_cover"), a);
      const m = tierMat("valve_cover", mats.tier[2]);
      b.add(box(lay.bore * 2 + 0.03, len - 0.02, 0.05, m, 0, 0, top + 0.2));
      for (let i = 0; i < 5; i++) b.add(box(0.008, len - 0.08, 0.012, mats.alu, (i - 2) * 0.016, 0, top + 0.228));
      b.add(cyl(0.012, 0.02, mats.dark, "z", 0.03, len / 2 - 0.08, top + 0.235));
    }
  }
  // Timing belt cover across the front.
  if (has("timing")) {
    const g = part("timing");
    const m = tierMat("timing", mats.cover);
    const w = lay.banks.length > 1 ? 0.5 : 0.18;
    g.add(box(w, 0.03, lay.deck + 0.2, m, 0, len / 2 + 0.04, (lay.deck + 0.2) / 2 - 0.02));
    g.add(cyl(0.06, 0.03, mats.dark, "y", 0, len / 2 + 0.06, 0));
  }
  // Intake: runners from the head's left side to a plenum; throttle body on its front.
  const outer = (side: number): [number, number] => {
    // Left / right edge of the head (V: the outer side of each bank), as (x, z).
    if (lay.banks.length > 1) {
      const a = lay.banks[side > 0 ? 1 : 0];
      const r = lay.deck + 0.06;
      return [Math.sin(a) * r + side * Math.cos(a) * 0.08, Math.cos(a) * r - Math.abs(Math.sin(a)) * 0.05];
    }
    return [side * (lay.bore + 0.05), lay.deck + 0.05];
  };
  if (has("intake_manifold")) {
    const g = part("intake_manifold");
    const m = tierMat("intake_manifold", mats.alu);
    if (lay.banks.length > 1) {
      // V engine: a plenum in the valley.
      g.add(box(0.16, len * 0.8, 0.08, m, 0, 0, lay.deck + 0.15));
      for (const side of [-1, 1]) for (const y of ys) {
        const [x, z] = outer(side);
        g.add(tube([[x * 0.5, y, z + 0.02], [x * 0.25, y, lay.deck + 0.14], [0, y, lay.deck + 0.15]], 0.016, m));
      }
    } else {
      const [x, z] = outer(-1);
      g.add(box(0.07, len * 0.85, 0.08, m, x - 0.12, 0, z + 0.02));
      for (const y of ys) g.add(tube([[x, y, z], [x - 0.05, y, z + 0.05], [x - 0.1, y, z + 0.03]], 0.016, m));
    }
  }
  if (has("throttle_body")) {
    const g = part("throttle_body");
    const m = tierMat("throttle_body", mats.alu);
    if (lay.banks.length > 1) g.add(cyl(0.04, 0.06, m, "y", 0, len * 0.4 + 0.04, lay.deck + 0.15));
    else {
      const [x, z] = outer(-1);
      g.add(cyl(0.035, 0.06, m, "y", x - 0.12, len * 0.425 + 0.03, z + 0.02));
    }
  }
  // Exhaust: one pipe per cylinder from the outer side, merging below.
  if (has("exhaust_manifold")) {
    const g = part("exhaust_manifold");
    const m = tierMat("exhaust_manifold", worn("exhaust_manifold", mats.rust));
    for (const side of lay.banks.length > 1 ? [-1, 1] : [1]) {
      const [x, z] = outer(side);
      for (const y of ys) g.add(tube([[x, y, z - 0.02], [x + side * 0.07, y, z - 0.07], [x + side * 0.09, y * 0.4, z - 0.2], [x + side * 0.08, -len / 2, z - 0.28]], 0.014, m));
    }
  }
  // Flywheel and clutch on the back.
  if (has("flywheel")) {
    const g = part("flywheel");
    const m = tierMat("flywheel", mats.steel);
    g.add(cyl(0.15, 0.025, m, "y", 0, -len / 2 - 0.04, 0, false, 32));
    g.add(cyl(0.152, 0.012, mats.dark, "y", 0, -len / 2 - 0.04, 0, false, 32));
  }
  if (has("clutch")) {
    const g = part("clutch");
    g.add(cyl(0.12, 0.012, tierMat("clutch", mats.friction), "y", 0, -len / 2 - 0.06, 0, false, 28));
    g.add(cyl(0.13, 0.03, mats.alu, "y", 0, -len / 2 - 0.085, 0, false, 28));
  }
  for (const o of root.children) o.traverse((c) => ((c as THREE.Mesh).castShadow = true));
  return root;
}

/** An engine stand on casters: the engine bolts to its plate by the bell housing face (-Y). */
export function buildStand(): THREE.Group {
  const g = new THREE.Group();
  g.name = "engine-stand";
  const m = mats.standBlue;
  g.add(box(0.06, 0.06, 0.92, m, 0, -0.52, 0.46));
  g.add(box(0.7, 0.06, 0.06, m, 0, -0.52, 0.06));
  g.add(box(0.06, 0.9, 0.06, m, 0, -0.1, 0.06));
  for (const p of [[-0.35, -0.52], [0.35, -0.52], [0, 0.35]] as const) g.add(cyl(0.04, 0.04, mats.rubber, "x", p[0], p[1], 0.04));
  g.add(box(0.06, 0.24, 0.06, m, 0, -0.4, 0.9));
  g.add(cyl(0.11, 0.02, mats.steel, "y", 0, -0.28, 0.9, false, 24));
  for (const a of [0.6, 2.2, 4.0, 5.4]) g.add(box(0.025, 0.12, 0.025, mats.steel, Math.cos(a) * 0.12, -0.24, 0.9 + Math.sin(a) * 0.12));
  return g;
}

/** Height of the engine's crank centre on the stand, m. */
export const STAND_CRANK_Z = 0.9;
