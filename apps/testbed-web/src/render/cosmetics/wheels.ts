// Wheel corner looks: rims, tyres, sidewall graphics, hub spikes, big brakes, coil springs,
// dampers and anti-roll bars. Rim and tyre pieces sit in wheel space (x across the wheel,
// outward = the side the wheel is on), so they spin and steer with it.
import * as THREE from "three";
import type { Builder, LookCtx } from "./context";
import { NAMED, box, cylX, ringX, tube } from "./kit";

const CORNERS = ["fl", "fr", "rl", "rr"] as const;
const corner = (i: number) => CORNERS[i] ?? "fl";

/** Outward sign of wheel `i` (left wheels face −x). */
const outward = (c: LookCtx, i: number) => Math.sign(c.frame.wheels[i].position_m[0]) || 1;

/** A part on the wheel face at radius `r`, angle `a` (rotated so its local +y points outward along the radius). */
function radial(mesh: THREE.Mesh, x: number, r: number, a: number): THREE.Mesh {
  mesh.rotation.x = a;
  mesh.position.set(x, r * Math.cos(a), r * Math.sin(a));
  return mesh;
}

/** Spoke from radius r0 to r1 at angle a: a box `w` wide, `t` thick. */
function spoke(m: THREE.Material, x: number, r0: number, r1: number, a: number, w: number, t: number): THREE.Mesh {
  return radial(new THREE.Mesh(new THREE.BoxGeometry(t, r1 - r0, w), m), x, (r0 + r1) / 2, a);
}

/** Rims replacing the stock ones: steel (silver / black), mesh (silver / gold), forged five-spoke,
 * turbofan, deep dish and split spoke. */
const rims: Builder = (c, p) => {
  const style = p.style ?? "forged_5spoke";
  const k = c.kit;
  c.hide((d) => d.wheel != null && d.kind === "rim");
  c.frame.wheels.forEach((info, i) => {
    const sx = outward(c, i);
    const rr = c.frame.rimRadius(i);
    const hw = info.width_m * 0.42;
    const g = new THREE.Group();
    const faceM =
      style === "black_steel" ? k.black : style === "gold_mesh" ? k.gold : style === "split_spoke" ? k.gunmetal : style === "turbofan" ? k.white : style === "steel" ? k.steel : k.alloy;
    const dish = style === "deep_dish" ? 0.075 : style === "steel" || style === "black_steel" ? 0.035 : 0.025;
    const face = sx * (hw - dish);
    // Barrel (open cylinder) and the outer lip.
    g.add(new THREE.Mesh(new THREE.CylinderGeometry(rr * 0.97, rr * 0.97, hw * 2, 28, 1, true).rotateZ(Math.PI / 2), faceM === k.white ? k.alloy : faceM));
    const lipM = style === "deep_dish" ? k.chrome : faceM;
    g.add(ringX(style === "deep_dish" ? rr * 0.74 : rr * 0.9, rr, lipM, sx * hw));
    if (style === "deep_dish") g.add(new THREE.Mesh(new THREE.CylinderGeometry(rr * 0.74, rr * 0.74, dish, 28, 1, true).rotateZ(Math.PI / 2), k.chrome).translateX(sx * (hw - dish / 2)));
    // Dark disc behind the spokes so the brake shows through the gaps.
    g.add(ringX(rr * 0.2, rr * 0.92, k.hole, face - sx * 0.02));
    const hub = cylX(rr * 0.2, 0.04, faceM, { x: face, y: 0, z: 0 }, 18);
    g.add(hub);
    g.add(cylX(rr * 0.08, 0.045, style === "gold_mesh" ? k.chrome : k.black, { x: face + sx * 0.003, y: 0, z: 0 }, 12));
    switch (style) {
      case "steel":
      case "black_steel": {
        g.add(ringX(rr * 0.2, rr * 0.92, faceM, face));
        for (let n = 0; n < 8; n++) {
          const a = (n / 8) * Math.PI * 2;
          g.add(cylX(rr * 0.09, 0.01, k.hole, { x: face + sx * 0.002, y: rr * 0.6 * Math.cos(a), z: rr * 0.6 * Math.sin(a) }, 10));
        }
        break;
      }
      case "mesh":
      case "gold_mesh": {
        for (let n = 0; n < 14; n++) {
          const a = (n / 14) * Math.PI * 2;
          for (const twist of [-0.32, 0.32]) {
            const r0 = rr * 0.2;
            const a0 = a;
            const a1 = a + twist;
            g.add(tube({ x: face, y: r0 * Math.cos(a0), z: r0 * Math.sin(a0) }, { x: face, y: rr * 0.9 * Math.cos(a1), z: rr * 0.9 * Math.sin(a1) }, 0.006, faceM, 5));
          }
        }
        break;
      }
      case "turbofan": {
        g.add(ringX(rr * 0.2, rr * 0.9, k.alloy, face - sx * 0.01));
        for (let n = 0; n < 12; n++) {
          const blade = spoke(k.white, face, rr * 0.25, rr * 0.88, (n / 12) * Math.PI * 2, rr * 0.18, 0.012);
          blade.rotateY(0.5);
          g.add(blade);
        }
        break;
      }
      case "split_spoke": {
        for (let n = 0; n < 5; n++) {
          const a = (n / 5) * Math.PI * 2;
          for (const d of [-0.11, 0.11]) g.add(spoke(faceM, face, rr * 0.18, rr * 0.92, a + d, rr * 0.08, 0.025));
        }
        break;
      }
      case "deep_dish": {
        for (let n = 0; n < 6; n++) g.add(spoke(k.black, face, rr * 0.18, rr * 0.76, (n / 6) * Math.PI * 2, rr * 0.16, 0.03));
        break;
      }
      default: {
        for (let n = 0; n < 5; n++) g.add(spoke(faceM, face, rr * 0.18, rr * 0.92, (n / 5) * Math.PI * 2, rr * 0.22, 0.035));
      }
    }
    c.wheel(i, g, true, `wheel_${corner(i)}`);
  });
};

/** Tyre styles: slick (glossy), sport, drag radials (tall rears, skinny fronts), all-terrain
 * tread blocks, wide. */
const tyres: Builder = (c, p) => {
  switch (p.style) {
    case "slick":
      c.tweaks.tyreGloss = 1;
      return;
    case "sport":
      c.tweaks.tyreGloss = 0.4;
      return;
    case "wide":
      c.tweaks.wheelWidth = 1.15;
      return;
    case "drag":
      c.tweaks.dragRadials = true;
      return;
    case "all_terrain": {
      c.frame.wheels.forEach((info, i) => {
        const g = new THREE.Group();
        const R = info.radius_m;
        const n = 30;
        for (let k = 0; k < n; k++) {
          const a = (k / n) * Math.PI * 2;
          const off = (k % 2 ? 1 : -1) * info.width_m * 0.12;
          const block = new THREE.Mesh(new THREE.BoxGeometry(info.width_m * 0.62, 0.028, 0.06), c.kit.rubber);
          block.rotation.x = a;
          block.position.set(off, (R + 0.006) * Math.cos(a), (R + 0.006) * Math.sin(a));
          g.add(block);
        }
        c.wheel(i, g, true, `tyre_${corner(i)}`);
      });
      return;
    }
  }
};

/** Sidewall graphics on the outer face: whitewall, raised white letters, a red line. */
const sidewall: Builder = (c, p) => {
  c.frame.wheels.forEach((info, i) => {
    const sx = outward(c, i);
    const R = info.radius_m;
    const x = sx * (info.width_m / 2 + 0.003);
    const g = new THREE.Group();
    switch (p.style) {
      case "whitewall":
        g.add(ringX(R * 0.7, R * 0.86, c.kit.white, x));
        break;
      case "red_line":
        g.add(ringX(R * 0.84, R * 0.87, c.kit.red, x));
        break;
      default: {
        // Raised letters: two arcs of block "letters" (no words: generic sidewall lettering).
        for (const a0 of [0.3, Math.PI + 0.3]) {
          for (let k = 0; k < 11; k++) {
            const a = a0 + k * 0.17;
            const tall = k % 3 === 1 ? 0.035 : 0.028;
            const letter = new THREE.Mesh(new THREE.BoxGeometry(0.004, 0.022, tall), c.kit.white);
            letter.rotation.x = a + Math.PI / 2;
            letter.position.set(x, R * 0.82 * Math.cos(a), R * 0.82 * Math.sin(a));
            g.add(letter);
          }
        }
      }
    }
    c.wheel(i, g, true, `tyre_${corner(i)}`);
  });
};

/** Spiked hub covers (they spin with the wheel). */
const hubGuards: Builder = (c) => {
  c.frame.wheels.forEach((info, i) => {
    const sx = outward(c, i);
    const g = new THREE.Group();
    const x = sx * (info.width_m * 0.42);
    g.add(cylX(0.075, 0.04, c.kit.chrome, { x: x + sx * 0.02, y: 0, z: 0 }, 16));
    const spike = new THREE.Mesh(new THREE.ConeGeometry(0.045, 0.2, 12).rotateZ(-sx * (Math.PI / 2)), c.kit.chrome);
    spike.position.x = x + sx * 0.14;
    g.add(spike);
    c.wheel(i, g, true, `wheel_${corner(i)}`);
  });
};

/** Big brakes: a large slotted disc face and coloured calipers (red, then gold). */
const brakes: Builder = (c, p) => {
  c.tweaks.caliper = (p.level ?? 1) >= 2 ? 0xd4a236 : 0xc0161e;
  const discMat = new THREE.MeshStandardMaterial({ color: 0x9a9da2, metalness: 0.85, roughness: 0.35, map: discTexture(), side: THREE.DoubleSide });
  c.frame.wheels.forEach((_, i) => {
    const sx = outward(c, i);
    const rr = c.frame.rimRadius(i);
    const g = new THREE.Group();
    g.add(ringX(rr * 0.32, rr * (p.level === 2 ? 0.9 : 0.84), discMat, sx * 0.012, 36));
    c.wheel(i, g, true, `brake_${corner(i)}`);
  });
};

let discTex: THREE.CanvasTexture | null = null;
/** Cross-drilled, slotted disc face. */
function discTexture(): THREE.CanvasTexture {
  if (discTex) return discTex;
  const cv = document.createElement("canvas");
  cv.width = cv.height = 128;
  const g = cv.getContext("2d")!;
  g.fillStyle = "#a3a6ab";
  g.fillRect(0, 0, 128, 128);
  g.fillStyle = "#2a2b2e";
  for (let k = 0; k < 24; k++) {
    const a = (k / 24) * Math.PI * 2;
    for (const r of [34, 46, 56]) {
      if ((k + r) % 2) continue;
      g.beginPath();
      g.arc(64 + r * Math.cos(a + r * 0.01), 64 + r * Math.sin(a + r * 0.01), 2.4, 0, Math.PI * 2);
      g.fill();
    }
  }
  discTex = new THREE.CanvasTexture(cv);
  discTex.colorSpace = THREE.SRGBColorSpace;
  return discTex;
}

/** One coil spring between the wheel and the body, at a corner (chassis space). */
function coil(colour: number, at: THREE.Vector3, height: number): THREE.Mesh {
  const turns = 6;
  const r = 0.055;
  const pts: THREE.Vector3[] = [];
  for (let k = 0; k <= turns * 16; k++) {
    const t = k / (turns * 16);
    const a = t * turns * Math.PI * 2;
    pts.push(new THREE.Vector3(r * Math.cos(a), r * Math.sin(a), t * height));
  }
  const geo = new THREE.TubeGeometry(new THREE.CatmullRomCurve3(pts), turns * 16, 0.009, 6, false);
  const m = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color: colour, metalness: 0.4, roughness: 0.4 }));
  m.position.copy(at);
  return m;
}

/** Where a corner's spring stands: inboard of the wheel, from just above the hub. */
function springBase(c: LookCtx, i: number): THREE.Vector3 {
  const [x, y, z] = c.frame.wheels[i].position_m;
  return new THREE.Vector3(x - Math.sign(x) * 0.17, y, z + 0.04);
}

/** Coloured coil springs at every corner (lowering springs blue, coilovers red / yellow, lift orange). */
const springs: Builder = (c, p) => {
  const colour = NAMED[p.colour ?? "red"] ?? NAMED.red;
  c.frame.wheels.forEach((_, i) => c.body(coil(colour, springBase(c, i), 0.3), null, `suspension_${corner(i)}`));
};

/** Gold damper bodies inside the springs. */
const dampers: Builder = (c) => {
  c.frame.wheels.forEach((_, i) => {
    const b = springBase(c, i);
    const body = tube(b.clone().setZ(b.z + 0.02), b.clone().setZ(b.z + 0.3), 0.024, c.kit.gold, 10);
    c.body(body, null, `suspension_${corner(i)}`);
  });
};

/** Anti-roll bar across an axle (red), with end links. */
const swayBar: Builder = (c, p) => {
  const axle = c.frame.axle(p.axle ?? "front");
  if (axle.length < 2) return;
  const [a, b] = axle.map((w) => w.info.position_m);
  const y = a[1] - 0.18;
  const z = a[2] - 0.04;
  const g = new THREE.Group();
  const xa = a[0] - Math.sign(a[0]) * 0.26;
  const xb = b[0] - Math.sign(b[0]) * 0.26;
  g.add(tube({ x: xa, y, z }, { x: xb, y, z }, 0.013, c.kit.red, 8));
  for (const x of [xa, xb]) g.add(tube({ x, y, z }, { x, y: y + 0.16, z: z + 0.02 }, 0.012, c.kit.red, 8));
  g.add(box(0.05, 0.05, 0.04, c.kit.rubber, xa * 0.6, y, z));
  g.add(box(0.05, 0.05, 0.04, c.kit.rubber, xb * 0.6, y, z));
  c.body(g, p.axle === "rear" ? "FrameRear" : "FrameFront", p.axle === "rear" ? "arb_rear" : "arb_front");
};

/** Wheel-corner builders by piece name. */
export const WHEEL_BUILDERS: Record<string, Builder> = {
  rims,
  tyres,
  sidewall,
  hub_guards: hubGuards,
  brakes,
  springs,
  dampers,
  sway_bar: swayBar,
};
