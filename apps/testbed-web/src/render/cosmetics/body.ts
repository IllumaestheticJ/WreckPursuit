// Exterior bolt-ons: aero, body kits, bull bars and armour, roof pieces, lamps, exhaust tips.
// Each builder places itself from the car's frame (frame.ts), so one builder fits every model.
import * as THREE from "three";
import type { Builder } from "./context";
import { NAMED, bentTube, box, cylY, lampMat, meshTexture, tube, wedge } from "./kit";

const V = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);

/** Front splitter: a flat blade under the bumper, sticking out a little (low-drag: longer, with an undertray lip). */
const splitter: Builder = (c, p) => {
  const f = c.frame.bumperFront;
  const w = f.max.x - f.min.x;
  const depth = p.style === "lowdrag" ? 0.32 : 0.22;
  const g = new THREE.Group();
  g.add(box(w * 0.98, depth, 0.022, c.kit.carbon, 0, f.max.y - depth / 2 + 0.07, f.min.z + 0.01));
  for (const sx of [-1, 1]) g.add(box(0.02, depth * 0.7, 0.07, c.kit.carbon, sx * w * 0.47, f.max.y - depth * 0.4, f.min.z + 0.05));
  c.body(g, "FrontBumper", "front_splitter");
};

/** Canards: two angled fins on each bumper corner. */
const canards: Builder = (c) => {
  const f = c.frame.bumperFront;
  const g = new THREE.Group();
  for (const sx of [-1, 1])
    for (let k = 0; k < 2; k++) {
      const fin = box(0.2, 0.16, 0.018, c.kit.carbon, sx * (f.max.x - 0.06), f.max.y - 0.07, f.min.z + 0.17 + k * 0.12);
      fin.rotation.y = sx * 0.12;
      fin.rotation.x = 0.18;
      g.add(fin);
    }
  c.body(g, "FrontBumper", "front_splitter");
};

/** Wheel-arch flares in body colour over one axle (widebody kits). */
const flares: Builder = (c, p) => {
  for (const { info } of c.frame.axle(p.axle ?? "front")) {
    const [x, y, z] = info.position_m;
    const sx = Math.sign(x) || 1;
    const r = info.radius_m + 0.07;
    // Half torus over the top of the wheel, in the car's YZ plane at the body side.
    const arch = new THREE.Mesh(new THREE.TorusGeometry(r, 0.055, 6, 18, Math.PI), c.paint);
    // Torus x → car y (along the car), torus y → car z (the arc goes over the top), torus z → car x.
    arch.setRotationFromMatrix(new THREE.Matrix4().makeBasis(V(0, 1, 0), V(0, 0, 1), V(1, 0, 0)));
    arch.position.set(sx * (c.frame.halfW + 0.02), y, z);
    arch.scale.set(1, 1, 0.75);
    const front = y > 0;
    c.body(arch, front ? (sx < 0 ? "FrontLeftFender" : "FrontRightFender") : sx < 0 ? "RearLeftQuarter" : "RearRightQuarter", front ? "front_bumper" : "rear_bumper");
  }
};

/** Side skirts in body colour between the wheels (wide: deeper, with flares on both axles). */
const skirts: Builder = (c, p) => {
  const front = c.frame.axle("front")[0]?.info;
  const rear = c.frame.axle("rear")[0]?.info;
  if (!front || !rear) return;
  const y0 = rear.position_m[1] + rear.radius_m + 0.06;
  const y1 = front.position_m[1] - front.radius_m - 0.06;
  const wide = p.style === "wide";
  const z = c.frame.body.min.z + 0.02;
  for (const sx of [-1, 1]) {
    const s = box(wide ? 0.09 : 0.06, y1 - y0, wide ? 0.15 : 0.11, c.paint, sx * (c.frame.halfW + (wide ? 0.04 : 0.015)), (y0 + y1) / 2, z + 0.05);
    c.body(s, sx < 0 ? "FrameLeft" : "FrameRight", "side_skirts");
  }
  if (wide) {
    flares(c, { piece: "flares", axle: "front" });
    flares(c, { piece: "flares", axle: "rear" });
  }
};

/** Diffuser: a carbon tray under the rear bumper with five tall fins. */
const diffuser: Builder = (c) => {
  const r = c.frame.bumperRear;
  const w = r.max.x - r.min.x;
  const g = new THREE.Group();
  const tray = box(w * 0.7, 0.4, 0.02, c.kit.carbon, 0, r.min.y + 0.15, r.min.z - 0.01);
  tray.rotation.x = -0.12;
  g.add(tray);
  for (let k = -2; k <= 2; k++) g.add(box(0.015, 0.36, 0.1, c.kit.carbon, k * w * 0.13, r.min.y + 0.14, r.min.z - 0.06));
  c.body(g, "RearBumper", "diffuser");
};

/** Rear wings: street lip, ducktail, GT, drift and swan-neck. Hides the model's own wing. */
const wing: Builder = (c, p) => {
  const t = c.frame.trunk;
  const w = t.max.x - t.min.x;
  const edgeY = t.min.y + 0.06;
  const topZ = c.frame.surfaceZ("trunk", 0, edgeY + 0.1, t.max.z);
  c.hide((d) => d.name.startsWith("wing") || d.name === "spoiler");
  const g = new THREE.Group();
  switch (p.style) {
    case "ducktail": {
      // Tall at the rear edge, blending into the lid going forward.
      g.add(wedge(w * 0.92, 0.3, 0.1, 0.02, c.paint).translateY(t.min.y).translateZ(topZ - 0.01));
      break;
    }
    case "street": {
      const blade = box(w * 0.86, 0.16, 0.025, c.paint, 0, edgeY + 0.05, topZ + 0.07);
      blade.rotation.x = 0.12;
      g.add(blade);
      for (const sx of [-1, 1]) g.add(box(0.05, 0.08, 0.07, c.paint, sx * w * 0.36, edgeY + 0.05, topZ + 0.03));
      break;
    }
    default: {
      // Plate on stands: GT (carbon, medium), drift (black, tall, big end plates), swan (stands
      // hang the plate from above).
      const drift = p.style === "drift";
      const swan = p.style === "swan";
      const m = drift ? c.kit.black : c.kit.carbon;
      const h = drift ? 0.42 : swan ? 0.36 : 0.3;
      const chord = drift ? 0.26 : 0.3;
      const span = Math.min(w * 1.02, c.frame.halfW * 2 - 0.04);
      const plateZ = topZ + h;
      const plate = box(span, chord, 0.026, m, 0, edgeY - 0.02, plateZ);
      plate.rotation.x = -0.14;
      g.add(plate);
      const ep = drift ? 0.36 : 0.3;
      for (const sx of [-1, 1]) {
        g.add(box(0.012, ep, drift ? 0.2 : 0.14, m, sx * (span / 2 + 0.006), edgeY - 0.02, plateZ - 0.02));
        const sxp = sx * w * 0.3;
        if (swan) g.add(bentTube([V(sxp, edgeY + 0.12, topZ), V(sxp, edgeY + 0.1, plateZ + 0.07), V(sxp, edgeY - 0.04, plateZ + 0.07), V(sxp, edgeY - 0.04, plateZ + 0.015)], 0.014, c.kit.black));
        else g.add(box(0.02, 0.12, h, c.kit.black, sxp, edgeY + 0.02, topZ + h / 2));
      }
    }
  }
  c.body(g, "Trunk", "rear_wing");
};

/** Hood vents: two louvred panels in front of the windshield. */
const hoodVents: Builder = (c) => {
  const h = c.frame.hood;
  const len = h.max.y - h.min.y;
  const g = new THREE.Group();
  for (const sx of [-1, 1]) {
    const x = sx * (h.max.x - h.min.x) * 0.22;
    const y = h.min.y + len * 0.55;
    const z = c.frame.surfaceZ("hood", x, y, h.max.z);
    for (let k = 0; k < 5; k++) {
      const slat = box(0.32, 0.03, 0.025, c.kit.black, x, y - 0.12 + k * 0.06, z + 0.006);
      slat.rotation.x = -0.35;
      g.add(slat);
    }
  }
  c.body(g, "Hood", "hood_vent");
};

/** Hood scoop: a body-colour wedge with a black mouth. Hides the model's own scoop. */
const hoodScoop: Builder = (c) => {
  const h = c.frame.hood;
  const y = h.min.y + (h.max.y - h.min.y) * 0.45;
  const z = c.frame.surfaceZ("hood", 0, y, h.max.z);
  c.hide((d) => d.name.startsWith("hood_scoop"));
  const g = new THREE.Group();
  // Low at the windshield end, mouth facing forward.
  g.add(wedge(0.62, 0.55, 0.02, 0.11, c.paint).translateY(y - 0.27).translateZ(z - 0.01));
  g.add(box(0.5, 0.01, 0.07, c.kit.hole, 0, y + 0.285, z + 0.06));
  c.body(g, "Hood", "hood_vent");
};

/** Roof scoop at the front of the roof. Hides the model's own. */
const roofScoop: Builder = (c) => {
  const r = c.frame.roof;
  const y = r.max.y - 0.25;
  const z = c.frame.surfaceZ("roof", 0, y, r.max.z);
  c.hide((d) => d.name.startsWith("roof_scoop"));
  const g = new THREE.Group();
  g.add(wedge(0.42, 0.36, 0.02, 0.1, c.paint).translateY(y - 0.18).translateZ(z - 0.01));
  g.add(box(0.34, 0.01, 0.07, c.kit.hole, 0, y + 0.185, z + 0.055));
  c.body(g, "Roof", "roof_rack");
};

/** Roof rack: two side rails and three cross bars. */
const roofRack: Builder = (c) => {
  const r = c.frame.roof;
  const zTop = r.max.z + 0.07;
  const g = new THREE.Group();
  const xR = (r.max.x - r.min.x) * 0.42;
  for (const sx of [-1, 1]) {
    g.add(tube(V(sx * xR, r.min.y + 0.05, zTop), V(sx * xR, r.max.y - 0.05, zTop), 0.014, c.kit.black));
    for (const y of [r.min.y + 0.08, r.max.y - 0.08]) g.add(box(0.04, 0.04, 0.08, c.kit.black, sx * xR, y, zTop - 0.04));
  }
  for (let k = 0; k < 3; k++) {
    const y = r.min.y + 0.12 + ((r.max.y - r.min.y - 0.24) * k) / 2;
    g.add(box(xR * 2 + 0.06, 0.03, 0.022, c.kit.alloy, 0, y, zTop + 0.012));
  }
  c.body(g, "Roof", "roof_rack");
};

/** LED light bar across the front of the roof (lit with the headlights). */
const roofLedBar: Builder = (c) => {
  const r = c.frame.roof;
  const w = (r.max.x - r.min.x) * 0.85;
  const z = r.max.z + 0.08;
  const y = r.max.y - 0.06;
  const g = new THREE.Group();
  g.add(box(w, 0.07, 0.07, c.kit.black, 0, y, z));
  const lens = lampMat(0xf4f7ff);
  c.lamp(lens, 3);
  g.add(box(w * 0.96, 0.01, 0.045, lens, 0, y + 0.036, z));
  for (const sx of [-1, 1]) g.add(box(0.03, 0.03, 0.08, c.kit.black, sx * w * 0.42, y - 0.02, z - 0.05));
  c.body(g, "Roof", "roof_rack");
};

/** Head lamp styles: lens tint goes through tweaks; halo adds a glowing ring round each lamp. */
const lights: Builder = (c, p) => {
  c.tweaks.lamps = p.style;
  if (p.style !== "halo") return;
  const ring = lampMat(0xdff4ff);
  c.lamp(ring, 2.5);
  const g = new THREE.Group();
  for (const sx of [-1, 1] as const) {
    const b = c.frame.headlight(sx);
    const centre = b.getCenter(new THREE.Vector3());
    const r = Math.min(b.max.x - b.min.x, b.max.z - b.min.z) * 0.42;
    for (const dx of [-0.25, 0.25]) {
      const halo = new THREE.Mesh(new THREE.TorusGeometry(r, 0.008, 6, 24), ring);
      halo.rotation.x = Math.PI / 2;
      halo.position.set(centre.x + dx * (b.max.x - b.min.x), b.max.y + 0.006, centre.z);
      g.add(halo);
    }
  }
  c.body(g, "FrameFront", "lights");
};

/** Fog lamps low in the front bumper (round chrome pots or LED pods). */
const fogLights: Builder = (c, p) => {
  const f = c.frame.bumperFront;
  const lens = lampMat(p.style === "led_pods" ? 0xf4f7ff : 0xfff1b8);
  c.lamp(lens, 2);
  const g = new THREE.Group();
  for (const sx of [-1, 1]) {
    const x = sx * (f.max.x - f.min.x) * 0.36;
    const z = f.min.z + 0.13;
    if (p.style === "led_pods") {
      g.add(box(0.11, 0.06, 0.06, c.kit.black, x, f.max.y + 0.02, z));
      g.add(box(0.09, 0.01, 0.04, lens, x, f.max.y + 0.052, z));
    } else {
      g.add(cylY(0.055, 0.06, c.kit.chrome, V(x, f.max.y + 0.01, z)));
      g.add(cylY(0.045, 0.01, lens, V(x, f.max.y + 0.043, z)));
    }
  }
  c.body(g, "FrontBumper", null);
};

/** Underglow: neon strips under the sills and a soft pool of light on the road. */
const underglow: Builder = (c, p) => {
  const colour = NAMED[p.colour ?? "blue"] ?? NAMED.blue;
  const strip = lampMat(colour);
  c.lamp(strip, 2.4);
  const b = c.frame.body;
  const z = b.min.z - 0.01;
  const len = (b.max.y - b.min.y) * 0.7;
  const g = new THREE.Group();
  for (const sx of [-1, 1]) g.add(box(0.03, len, 0.02, strip, sx * (c.frame.halfW - 0.12), (b.min.y + b.max.y) / 2, z));
  g.add(box(c.frame.halfW * 1.2, 0.03, 0.02, strip, 0, b.max.y - 0.5, z));
  g.add(box(c.frame.halfW * 1.2, 0.03, 0.02, strip, 0, b.min.y + 0.5, z));
  // Pool on the road: additive radial glow under the car, faded with the strips.
  const cv = document.createElement("canvas");
  cv.width = cv.height = 64;
  const gx = cv.getContext("2d")!;
  const grad = gx.createRadialGradient(32, 32, 4, 32, 32, 32);
  grad.addColorStop(0, "rgba(255,255,255,0.6)");
  grad.addColorStop(1, "rgba(255,255,255,0)");
  gx.fillStyle = grad;
  gx.fillRect(0, 0, 64, 64);
  const poolMat = new THREE.MeshBasicMaterial({ color: colour, map: new THREE.CanvasTexture(cv), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, opacity: 0 });
  const pool = new THREE.Mesh(new THREE.PlaneGeometry(c.frame.halfW * 2 + 1.0, b.max.y - b.min.y + 0.6), poolMat);
  pool.position.set(0, (b.min.y + b.max.y) / 2, 0.02);
  pool.renderOrder = 1;
  pool.userData.pool = true;
  g.add(pool);
  // The pool's opacity follows the strips (look.ts lights): a basic material, so a proxy lamp.
  strip.userData.pool = poolMat;
  c.body(g, null, null);
};

/** Rubber mud flaps behind every wheel. */
const mudFlaps: Builder = (c) => {
  for (const { info } of c.frame.axle("front").concat(c.frame.axle("rear"))) {
    const [x, y, z] = info.position_m;
    const sx = Math.sign(x) || 1;
    const flap = box(Math.min(info.width_m + 0.04, 0.26), 0.012, 0.3, c.kit.rubber, x + sx * 0.01, y - info.radius_m - 0.06, z - info.radius_m + 0.2);
    c.body(flap, y > 0 ? (sx < 0 ? "FrontLeftFender" : "FrontRightFender") : sx < 0 ? "RearLeftQuarter" : "RearRightQuarter", null);
  }
};

/** Exhaust tips at the stock outlet: sport, race (twin), cannon, dual, quad (both sides), burnt titanium. */
const exhaustTip: Builder = (c, p) => {
  const { at, radius } = c.frame.exhaustOutlet;
  const style = p.style ?? "sport";
  c.hide((d) => d.name.startsWith("exhaust_tip"));
  const g = new THREE.Group();
  const m = style === "burnt" ? c.kit.titanium : c.kit.chrome;
  const one = (x: number, r: number, len: number) => {
    const tip = cylY(r, len, m, V(x, at.y - len / 2 + 0.06, at.z));
    const hole = cylY(r * 0.8, 0.005, c.kit.hole, V(x, at.y - len + 0.058, at.z));
    g.add(tip, hole);
  };
  const r0 = Math.max(radius, 0.035);
  const xs = (n: number, gap: number) => Array.from({ length: n }, (_, k) => at.x + (k - (n - 1) / 2) * gap);
  switch (style) {
    case "cannon":
      one(at.x, r0 * 1.9, 0.22);
      break;
    case "race":
    case "dual":
      for (const x of xs(2, r0 * (style === "race" ? 2.9 : 2.5))) one(x, r0 * (style === "race" ? 1.35 : 1.15), 0.16);
      break;
    case "quad":
      for (const side of [1, -1]) for (const x of xs(2, r0 * 2.4)) one(side * x, r0 * 1.05, 0.15);
      break;
    case "burnt":
      one(at.x, r0 * 1.45, 0.18);
      break;
    default:
      one(at.x, r0 * 1.35, 0.16);
  }
  c.body(g, "RearBumper", style === "sport" || style === "race" ? "exhaust" : null);
};

/** Bull bars on the front: tube, grille guard, ram bar, police push bar. */
const bullBar: Builder = (c, p) => {
  const f = c.frame.bumperFront;
  const w = f.max.x - f.min.x;
  const y = f.max.y + 0.1;
  const z0 = f.min.z + 0.05;
  const z1 = f.max.z + (p.style === "guard" ? 0.18 : 0.08);
  c.hide((d) => d.name.startsWith("pushbar"));
  const g = new THREE.Group();
  switch (p.style) {
    case "tube": {
      const m = c.kit.chrome;
      const xo = w * 0.3;
      g.add(bentTube([V(-xo, y - 0.08, z0), V(-xo, y, z0 + 0.1), V(-xo, y, z1 - 0.06), V(-xo + 0.08, y, z1), V(xo - 0.08, y, z1), V(xo, y, z1 - 0.06), V(xo, y, z0 + 0.1), V(xo, y - 0.08, z0)], 0.03, m));
      g.add(tube(V(-xo, y, z0 + 0.22), V(xo, y, z0 + 0.22), 0.025, m));
      for (const x of [-0.12, 0.12]) g.add(tube(V(x * w, y, z0 + 0.22), V(x * w, y, z1), 0.02, m));
      break;
    }
    case "guard": {
      const m = c.kit.black;
      const xo = w * 0.47;
      g.add(bentTube([V(-xo, y - 0.12, z0 + 0.05), V(-xo, y, z0 + 0.12), V(-xo, y, z1), V(xo, y, z1), V(xo, y, z0 + 0.12), V(xo, y - 0.12, z0 + 0.05)], 0.028, m));
      g.add(tube(V(-xo, y, z0 + 0.15), V(xo, y, z0 + 0.15), 0.028, m));
      // Lamp guards: three verticals over each headlight, and the centre grille bars.
      for (const sx of [-1, 1] as const) {
        const hb = c.frame.headlight(sx);
        for (let k = 0; k < 3; k++) {
          const x = hb.min.x + ((hb.max.x - hb.min.x) * (k + 0.5)) / 3;
          g.add(tube(V(x, y, z0 + 0.15), V(x, y, z1), 0.012, m));
        }
      }
      for (const x of [-0.1, 0, 0.1]) g.add(tube(V(x * w, y, z0 + 0.15), V(x * w, y, z1), 0.018, m));
      break;
    }
    case "ram": {
      const m = c.kit.black;
      const xo = w * 0.36;
      g.add(box(xo * 2, 0.1, 0.12, m, 0, y + 0.02, z0 + 0.12));
      g.add(box(xo * 1.6, 0.08, 0.1, m, 0, y + 0.03, z0 + 0.36));
      const plate = box(xo * 1.4, 0.03, 0.22, c.kit.steel, 0, y + 0.06, z0 + 0.24);
      plate.rotation.x = -0.25;
      g.add(plate);
      for (const sx of [-1, 1]) g.add(box(0.1, 0.22, 0.42, m, sx * xo, y - 0.05, z0 + 0.22));
      break;
    }
    default: {
      // Police push bar: two padded uprights and a cross bar.
      const m = c.kit.black;
      const xo = w * 0.2;
      for (const sx of [-1, 1]) {
        g.add(box(0.08, 0.06, z1 - z0, m, sx * xo, y, (z0 + z1) / 2));
        g.add(box(0.1, 0.035, (z1 - z0) * 0.8, c.kit.rubber, sx * xo, y + 0.04, (z0 + z1) / 2));
      }
      g.add(box(xo * 2 + 0.08, 0.06, 0.06, m, 0, y, z0 + (z1 - z0) * 0.55));
    }
  }
  c.body(g, "FrontBumper", "push_bar");
};

/** A-pillar spotlight on the driver's side (pursuit package). */
const spotlight: Builder = (c) => {
  const ws = c.frame.windshield;
  if (!ws) return;
  const x = ws.min.x - 0.04;
  const y = ws.min.y + 0.12;
  const z = ws.max.z - 0.2;
  const g = new THREE.Group();
  g.add(tube(V(x + 0.03, y, z - 0.05), V(x, y, z), 0.012, c.kit.chrome));
  const lens = lampMat(0xffffff);
  c.lamp(lens, 2);
  g.add(cylY(0.06, 0.12, c.kit.chrome, V(x, y + 0.04, z + 0.03)));
  g.add(cylY(0.05, 0.01, lens, V(x, y + 0.105, z + 0.03)));
  c.body(g, "LeftDoorF", "push_bar");
};

/** Steel tube bumper with D-ring shackles (reinforced bumper beam). */
const steelBumper: Builder = (c, p) => {
  const front = p.end !== "rear";
  const b = front ? c.frame.bumperFront : c.frame.bumperRear;
  const w = b.max.x - b.min.x;
  const y = front ? b.max.y + 0.05 : b.min.y - 0.05;
  const z = b.min.z + 0.2;
  const g = new THREE.Group();
  g.add(box(w * 1.0, 0.11, 0.15, c.kit.black, 0, y, z));
  for (const sx of [-1, 1]) {
    g.add(box(0.06, 0.12, 0.17, c.kit.steel, sx * w * 0.5, y, z));
    const ring = new THREE.Mesh(new THREE.TorusGeometry(0.045, 0.012, 6, 12), c.kit.red);
    ring.rotation.y = Math.PI / 2;
    ring.position.set(sx * w * 0.3, y + (front ? 0.08 : -0.08), z - 0.02);
    g.add(ring);
  }
  c.body(g, front ? "FrontBumper" : "RearBumper", front ? "front_bumper_beam" : "rear_bumper_beam");
};

/** Aluminium skid plate under the engine. */
const skidPlate: Builder = (c) => {
  const f = c.frame.bumperFront;
  // Under the front bay (the front trunk of a mid-engined car), never the whole underside.
  const e = c.frame.frontBay;
  const y0 = e.min.y - 0.1;
  const y1 = f.max.y - 0.08;
  const g = new THREE.Group();
  const plate = box((f.max.x - f.min.x) * 0.7, y1 - y0, 0.012, c.kit.alloy, 0, (y0 + y1) / 2, Math.max(f.min.z - 0.03, 0.09));
  g.add(plate);
  for (let k = 0; k < 4; k++) g.add(box(0.5, 0.012, 0.004, c.kit.steel, 0, y0 + 0.1 + (k * (y1 - y0 - 0.2)) / 3, plate.position.z - 0.008));
  c.body(g, "FrameFront", "skid_plate");
};

/** Mesh guard in front of the radiator / grille. */
const radGuard: Builder = (c) => {
  const grille = c.frame.part("grille") ?? c.frame.part("engine_radiator");
  const f = c.frame.bumperFront;
  const b = grille ?? new THREE.Box3(V(-0.35, f.max.y - 0.02, f.min.z + 0.15), V(0.35, f.max.y, f.min.z + 0.45));
  const m = new THREE.MeshStandardMaterial({ map: meshTexture("#b8bdc2", 12, 5), transparent: true, alphaTest: 0.4, metalness: 0.8, roughness: 0.3, side: THREE.DoubleSide, forceSinglePass: true });
  const g = new THREE.Group();
  const w = b.max.x - b.min.x + 0.06;
  const h = Math.max(b.max.z - b.min.z + 0.06, 0.14);
  g.add(box(w, 0.006, h, m, (b.min.x + b.max.x) / 2, Math.max(b.max.y, f.max.y) + 0.02, (b.min.z + b.max.z) / 2));
  c.body(g, "FrontBumper", "radiator_guard");
};

/** Riveted armour plates over the doors. */
const armorPlates: Builder = (c) => {
  for (const [region, slot] of [
    ["LeftDoorF", "door_fl"],
    ["RightDoorF", "door_fr"],
    ["LeftDoorR", "door_rl"],
    ["RightDoorR", "door_rr"],
  ] as const) {
    const d = c.frame.region(region);
    if (!d) continue;
    const sx = d.min.x < 0 ? -1 : 1;
    const g = new THREE.Group();
    const y0 = d.min.y + 0.04;
    const y1 = d.max.y - 0.04;
    const z0 = d.min.z + 0.06;
    const z1 = d.max.z - 0.08;
    const x = sx * (Math.max(Math.abs(d.min.x), Math.abs(d.max.x)) + 0.012);
    g.add(box(0.012, y1 - y0, z1 - z0, c.kit.gunmetal, x, (y0 + y1) / 2, (z0 + z1) / 2));
    for (const y of [y0 + 0.04, (y0 + y1) / 2, y1 - 0.04])
      for (const z of [z0 + 0.04, z1 - 0.04]) {
        const rivet = new THREE.Mesh(new THREE.SphereGeometry(0.01, 6, 4), c.kit.steel);
        rivet.position.set(x + sx * 0.007, y, z);
        g.add(rivet);
      }
    c.body(g, region, slot);
  }
};

/** Door bars: an X of tube inside each front door, its top showing in the window line. */
const doorBars: Builder = (c) => {
  for (const region of ["LeftDoorF", "RightDoorF"] as const) {
    const d = c.frame.region(region);
    if (!d) continue;
    const sx = d.min.x < 0 ? -1 : 1;
    const x = sx * (Math.min(Math.abs(d.min.x), Math.abs(d.max.x)) - 0.06);
    const zTop = d.max.z + 0.03;
    const g = new THREE.Group();
    g.add(tube(V(x, d.min.y + 0.05, d.min.z + 0.1), V(x, d.max.y - 0.05, zTop), 0.02, c.kit.cage));
    g.add(tube(V(x, d.max.y - 0.05, d.min.z + 0.1), V(x, d.min.y + 0.05, zTop), 0.02, c.kit.cage));
    g.add(tube(V(x, d.min.y + 0.05, zTop), V(x, d.max.y - 0.05, zTop), 0.02, c.kit.cage));
    c.body(g, "Cabin", "door_bars");
  }
};

/** Whip aerial (scanner) on the trunk, or a stub on the back of the roof. */
const antenna: Builder = (c, p) => {
  const g = new THREE.Group();
  if (p.style === "stub") {
    const r = c.frame.roof;
    const y = r.min.y + 0.12;
    const z = c.frame.surfaceZ("roof", 0, y, r.max.z);
    const fin = wedge(0.03, 0.14, 0.06, 0.01, c.kit.black).translateY(y - 0.07).translateZ(z);
    g.add(fin);
    c.body(g, "Roof", null);
    return;
  }
  const t = c.frame.trunk;
  const x = t.max.x - 0.12;
  const y = t.min.y + 0.18;
  const z = c.frame.surfaceZ("trunk", x, y, t.max.z);
  g.add(cylY(0.018, 0.04, c.kit.black, V(x, y, z + 0.01)).rotateX(Math.PI / 2));
  g.add(tube(V(x, y, z), V(x, y - 0.1, z + 0.85), 0.004, c.kit.black));
  c.body(g, "Trunk", null);
};

/** Number plates swapped for fakes: a generic plate in different colours over the stock one. */
const plates: Builder = (c) => {
  for (const end of ["f", "r"] as const) {
    const pb = c.frame.part(`plate_${end}`) ?? c.frame.part("plate_recess");
    if (!pb || (end === "f" && !c.frame.part("plate_f"))) continue;
    const cv = document.createElement("canvas");
    cv.width = 256;
    cv.height = 64;
    const g = cv.getContext("2d")!;
    g.fillStyle = "#e8d23a";
    g.fillRect(0, 0, 256, 64);
    g.strokeStyle = "#1b1b1b";
    g.lineWidth = 6;
    g.strokeRect(3, 3, 250, 58);
    g.fillStyle = "#1b1b1b";
    g.font = "bold 40px monospace";
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.fillText("WRK 0P5", 128, 35);
    const tex = new THREE.CanvasTexture(cv);
    tex.colorSpace = THREE.SRGBColorSpace;
    const m = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.5 });
    const w = pb.max.x - pb.min.x;
    const h = pb.max.z - pb.min.z;
    const plate = new THREE.Mesh(new THREE.PlaneGeometry(w, h), m);
    const front = end === "f";
    // Face out of the bumper with the text upright and reading left to right from outside.
    const basis = new THREE.Matrix4().makeBasis(V(front ? -1 : 1, 0, 0), V(0, 0, 1), V(0, front ? 1 : -1, 0));
    plate.quaternion.setFromRotationMatrix(basis);
    plate.position.set((pb.min.x + pb.max.x) / 2, front ? pb.max.y + 0.004 : pb.min.y - 0.004, (pb.min.z + pb.max.z) / 2);
    c.body(plate, front ? "FrontBumper" : "RearBumper", null);
  }
};

/** Exterior builders by piece name. */
export const BODY_BUILDERS: Record<string, Builder> = {
  splitter,
  canards,
  flares,
  skirts,
  diffuser,
  wing,
  hood_vents: hoodVents,
  hood_scoop: hoodScoop,
  roof_scoop: roofScoop,
  roof_rack: roofRack,
  roof_led_bar: roofLedBar,
  lights,
  fog_lights: fogLights,
  underglow,
  mud_flaps: mudFlaps,
  exhaust_tip: exhaustTip,
  bull_bar: bullBar,
  spotlight,
  steel_bumper: steelBumper,
  skid_plate: skidPlate,
  rad_guard: radGuard,
  armor_plates: armorPlates,
  door_bars: doorBars,
  antenna,
  plates,
};
