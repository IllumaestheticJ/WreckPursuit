// Under the hood and inside the car: coolers, intakes, turbos and blowers, engine dress-up,
// roll cages, seats, shifters, nitrous, fuel cell, window net; and the looks that are material
// changes (paint finish, vinyl, tint, glazing, interior colour).
import * as THREE from "three";
import type { Builder, LookCtx } from "./context";
import { bentTube, box, cylX, cylY, lampMat, meshTexture, tube } from "./kit";

const V = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);

/** Seat row position: the front seats sit a little behind the front doors' middle. */
function seatY(c: LookCtx): number {
  const d = c.frame.region("LeftDoorF") ?? c.frame.region("RightDoorF");
  return d ? (d.min.y + d.max.y) / 2 - 0.18 : (c.frame.cabin.min.y + c.frame.cabin.max.y) / 2;
}

/** Cabin floor height. */
const floorZ = (c: LookCtx) => c.frame.cabin.min.z + 0.04;

/**
 * Top of the stock interior at (x, y): where a console or parcel-shelf piece stands. Many models
 * have one dark block for the whole interior; a piece below its top would be hidden inside it.
 * −Infinity for a model without an `interior` part (pieces then stand on the floor).
 */
const interiorTop = (c: LookCtx, x: number, y: number) => c.frame.surfaceZ("interior", x, y, -Infinity, 0.12);

/** Inside of the roof. */
const roofInZ = (c: LookCtx) => c.frame.roof.min.z - 0.05;

/** A bucket seat facing forward at (x, y) on the floor. */
function bucket(c: LookCtx, x: number, y: number): THREE.Group {
  const k = c.kit;
  const z = floorZ(c);
  const g = new THREE.Group();
  g.add(box(0.44, 0.46, 0.1, k.fabric, x, y, z + 0.28));
  const back = new THREE.Group();
  back.add(box(0.44, 0.08, 0.62, k.fabric, 0, 0, 0.31));
  for (const sx of [-1, 1]) back.add(box(0.06, 0.14, 0.5, k.red, sx * 0.22, 0.04, 0.3));
  back.add(box(0.12, 0.02, 0.08, k.hole, 0, 0.04, 0.5));
  back.position.set(x, y - 0.24, z + 0.3);
  back.rotation.x = -0.18;
  g.add(back);
  for (const sx of [-1, 1]) g.add(box(0.06, 0.4, 0.1, k.red, x + sx * 0.22, y, z + 0.36));
  return g;
}

/** Bucket seats for driver and passenger. */
const seats: Builder = (c) => {
  const y = seatY(c);
  const x = c.frame.halfW * 0.42;
  const g = new THREE.Group();
  g.add(bucket(c, -x, y), bucket(c, x, y));
  c.body(g, "Cabin", "seats");
};

/** Interior trim colour (suede look). */
const interior: Builder = (c, p) => {
  c.tweaks.interior = p.colour === "suede" ? 0x4c4f56 : 0x2b2724;
};

/** Stripped interior: bare grey trim (level 1); everything out but one race seat (level 2). */
const strip: Builder = (c, p) => {
  const level = p.level ?? 1;
  c.tweaks.strip = level;
  if (level < 2) return;
  c.hide((d) => d.kind === "interior");
  const g = new THREE.Group();
  const cab = c.frame.cabin;
  g.add(box(cab.max.x - cab.min.x, cab.max.y - cab.min.y, 0.01, c.kit.steel, 0, (cab.min.y + cab.max.y) / 2, floorZ(c)));
  g.add(bucket(c, -c.frame.halfW * 0.42, seatY(c)));
  // Bare dash beam and the steering wheel.
  const ws = c.frame.windshield;
  const dashY = ws ? ws.max.y - 0.2 : cab.max.y - 0.2;
  const dashZ = ws ? ws.min.z - 0.02 : floorZ(c) + 0.5;
  g.add(tube(V(cab.min.x + 0.05, dashY, dashZ), V(cab.max.x - 0.05, dashY, dashZ), 0.025, c.kit.steel));
  const wheel = new THREE.Mesh(new THREE.TorusGeometry(0.17, 0.016, 6, 20), c.kit.fabric);
  wheel.rotation.x = Math.PI / 2 - 0.35;
  wheel.position.set(-c.frame.halfW * 0.42, dashY - 0.15, dashZ - 0.02);
  g.add(wheel);
  c.body(g, "Cabin", "seats");
};

/** Roll cage: main hoop, A-pillar bars and a diagonal (level 2 adds door X-bars and a roof X). */
const cage: Builder = (c, p) => {
  const m = c.kit.cage;
  const r = 0.022;
  const xo = c.frame.halfW - 0.16;
  const y = seatY(c) - 0.4;
  const z0 = floorZ(c);
  const z1 = roofInZ(c);
  const ws = c.frame.windshield;
  const yFront = ws ? ws.min.y + 0.05 : y + 0.9;
  const yDash = ws ? ws.max.y - 0.15 : y + 1.2;
  const g = new THREE.Group();
  g.add(bentTube([V(-xo, y, z0), V(-xo, y, z1 - 0.06), V(-xo + 0.08, y, z1), V(xo - 0.08, y, z1), V(xo, y, z1 - 0.06), V(xo, y, z0)], r, m));
  for (const sx of [-1, 1]) g.add(bentTube([V(sx * (xo - 0.08), y, z1), V(sx * (xo - 0.04), yFront, z1 - 0.02), V(sx * (xo - 0.02), yDash, z0 + 0.35), V(sx * (xo - 0.02), yDash, z0)], r, m));
  g.add(tube(V(-xo, y, z0 + 0.05), V(xo, y, z1 - 0.06), r, m));
  // Rear stays to the floor behind.
  for (const sx of [-1, 1]) g.add(tube(V(sx * (xo - 0.05), y, z1 - 0.04), V(sx * (xo - 0.08), y - 0.7, z0 + 0.05), r * 0.9, m));
  if ((p.level ?? 1) >= 2) {
    for (const sx of [-1, 1]) {
      g.add(tube(V(sx * xo, y, z0 + 0.1), V(sx * (xo - 0.02), yDash, z0 + 0.5), r, m));
      g.add(tube(V(sx * xo, y, z0 + 0.5), V(sx * (xo - 0.02), yDash, z0 + 0.1), r, m));
    }
    g.add(tube(V(-xo + 0.08, y, z1), V(xo - 0.04, yFront, z1 - 0.02), r, m));
    g.add(tube(V(xo - 0.08, y, z1), V(-xo + 0.04, yFront, z1 - 0.02), r, m));
  }
  c.body(g, "Cabin", "roll_cage");
};

/** Short shifter knob, or a sequential lever with its gate. */
const shifter: Builder = (c, p) => {
  const y = seatY(c) + 0.38;
  const z = Math.max(floorZ(c) + 0.25, interiorTop(c, 0, y) - 0.02);
  const g = new THREE.Group();
  if (p.style === "sequential") {
    g.add(box(0.08, 0.16, 0.05, c.kit.black, 0, y, z));
    g.add(tube(V(0, y, z), V(0, y - 0.05, z + 0.28), 0.01, c.kit.alloy));
    g.add(cylY(0.025, 0.06, c.kit.black, V(0, y - 0.05, z + 0.3)));
  } else {
    g.add(tube(V(0, y, z), V(0, y, z + 0.12), 0.008, c.kit.chrome));
    const knob = new THREE.Mesh(new THREE.SphereGeometry(0.03, 10, 8), c.kit.red);
    knob.position.set(0, y, z + 0.14);
    g.add(knob);
  }
  c.body(g, "Cabin", "shifter");
};

/** Hydraulic handbrake: a tall red lever beside the shifter. */
const hydroHandbrake: Builder = (c) => {
  const y = seatY(c) + 0.2;
  const z = Math.max(floorZ(c) + 0.25, interiorTop(c, 0.1, y) - 0.02);
  const g = new THREE.Group();
  g.add(box(0.05, 0.08, 0.06, c.kit.black, 0.1, y, z));
  g.add(tube(V(0.1, y, z), V(0.1, y + 0.1, z + 0.32), 0.012, c.kit.red));
  c.body(g, "Cabin", null);
};

/** A small box on the dash (radar detector) with a red lamp. */
const dashBox: Builder = (c) => {
  const ws = c.frame.windshield;
  if (!ws) return;
  const y = ws.max.y - 0.16;
  const z = Math.max(ws.min.z + 0.02, interiorTop(c, -0.08, y) + 0.015);
  const lamp = lampMat(0xff3030);
  c.lamp(lamp, 1.5);
  const g = new THREE.Group();
  g.add(box(0.11, 0.07, 0.03, c.kit.black, -0.08, y, z));
  g.add(box(0.02, 0.005, 0.01, lamp, -0.06, y - 0.036, z + 0.005));
  c.body(g, "Cabin", null);
};

/** Nitrous bottles strapped down behind the front seats. */
const nitrous: Builder = (c, p) => {
  const n = p.bottles ?? 1;
  const cab = c.frame.cabin;
  const y = Math.max(cab.min.y + 0.3, seatY(c) - 0.6);
  const z = Math.max(floorZ(c) + 0.15, interiorTop(c, 0, y) + 0.09);
  const g = new THREE.Group();
  for (let k = 0; k < n; k++) {
    const yk = y - k * 0.22;
    g.add(cylX(0.09, 0.62, c.kit.nos, V(0, yk, z), 16));
    g.add(cylX(0.03, 0.08, c.kit.chrome, V(0.35, yk, z), 10));
    for (const x of [-0.18, 0.18]) g.add(cylX(0.095, 0.03, c.kit.black, V(x, yk, z), 16));
  }
  c.body(g, "Cabin", "nitrous");
};

/** Racing fuel cell in the boot. */
const fuelCell: Builder = (c) => {
  const t = c.frame.trunk;
  const y = (t.min.y + t.max.y) / 2;
  const z = Math.max(t.min.z - 0.25, floorZ(c) + 0.12);
  const g = new THREE.Group();
  g.add(box(0.55, 0.4, 0.24, c.kit.black, 0, y, z));
  g.add(box(0.57, 0.04, 0.26, c.kit.red, 0, y - 0.16, z));
  g.add(box(0.57, 0.04, 0.26, c.kit.red, 0, y + 0.16, z));
  g.add(cylX(0.05, 0.03, c.kit.alloy, V(0.1, y, z + 0.13)).rotateY(Math.PI / 2));
  c.body(g, "Trunk", "fuel_tank");
};

/** Window net in the driver's window. */
const windowNet: Builder = (c) => {
  const glass = c.frame.part("door_fl_glass");
  if (!glass) return;
  const m = new THREE.MeshStandardMaterial({ map: meshTexture("#141414", 8, 5), transparent: true, alphaTest: 0.35, side: THREE.DoubleSide, forceSinglePass: true, roughness: 1 });
  const w = glass.max.y - glass.min.y;
  const h = glass.max.z - glass.min.z;
  // Plane in the car's YZ plane (normal across the car).
  const net = new THREE.Mesh(new THREE.PlaneGeometry(h * 0.85, w * 0.8).rotateY(Math.PI / 2), m);
  net.position.set(glass.max.x + 0.03, (glass.min.y + glass.max.y) / 2 - w * 0.05, (glass.min.z + glass.max.z) / 2 - h * 0.05);
  c.body(net, "LeftDoorF", null);
};

/** Radiator: a polished alloy core over the stock one (race: with blue end tanks). */
const radiator: Builder = (c, p) => {
  const r = c.frame.part("engine_radiator");
  const f = c.frame.bumperFront;
  const b = r ?? new THREE.Box3(V(-0.36, f.min.y - 0.4, f.min.z + 0.1), V(0.36, f.min.y - 0.36, f.max.z));
  const g = new THREE.Group();
  const w = b.max.x - b.min.x;
  const h = b.max.z - b.min.z;
  const cx = (b.min.x + b.max.x) / 2;
  const cz = (b.min.z + b.max.z) / 2;
  g.add(box(w * 1.02, 0.035, h * 1.02, c.kit.core, cx, b.max.y + 0.012, cz));
  if (p.style === "race") for (const sx of [-1, 1]) g.add(box(0.05, 0.06, h * 1.05, c.kit.blue, cx + sx * (w / 2 + 0.03), b.max.y + 0.01, cz));
  c.body(g, "FrameFront", "radiator");
};

/** Front-mount intercooler in the bumper opening, piped into the bay. */
const intercooler: Builder = (c) => {
  const f = c.frame.bumperFront;
  const e = c.frame.engine;
  const w = (f.max.x - f.min.x) * 0.62;
  const y = f.max.y - 0.13;
  const z = f.min.z + 0.2;
  const g = new THREE.Group();
  g.add(box(w, 0.06, 0.2, c.kit.core, 0, y, z));
  for (const sx of [-1, 1]) {
    const x = sx * (w / 2 + 0.02);
    g.add(bentTube([V(x, y, z), V(x + sx * 0.04, y - 0.15, z + 0.05), V(sx * 0.25, e.max.y - 0.1, e.max.z - 0.2)], 0.035, c.kit.alloy));
  }
  c.body(g, "FrontBumper", "intercooler");
};

/** Oil cooler: a small core low in one corner of the bumper. */
const oilCooler: Builder = (c) => {
  const f = c.frame.bumperFront;
  const x = (f.max.x - f.min.x) * 0.3;
  const g = new THREE.Group();
  g.add(box(0.24, 0.04, 0.11, c.kit.core, x, f.max.y - 0.1, f.min.z + 0.12));
  g.add(tube(V(x - 0.1, f.max.y - 0.12, f.min.z + 0.12), V(0.15, c.frame.engine.max.y - 0.2, f.min.z + 0.2), 0.012, c.kit.black));
  c.body(g, "FrontBumper", "oil_cooler");
};

/** Intake: a cone filter in the bay (cold air: piped down to the bumper corner). */
const intake: Builder = (c, p) => {
  const e = c.frame.engine;
  const g = new THREE.Group();
  const filter = (at: THREE.Vector3) => {
    const f = new THREE.Mesh(new THREE.CylinderGeometry(0.05, 0.075, 0.15, 14).rotateZ(Math.PI / 2), c.kit.red);
    f.position.copy(at);
    g.add(f);
    g.add(cylX(0.078, 0.02, c.kit.chrome, V(at.x + 0.08, at.y, at.z), 14));
  };
  if (p.style === "cold_air") {
    const f = c.frame.bumperFront;
    const low = V(f.max.x - 0.25, f.max.y - 0.2, f.min.z + 0.2);
    g.add(bentTube([V(0.1, e.max.y - 0.15, e.max.z - 0.08), V(e.max.x - 0.05, e.max.y - 0.05, e.max.z - 0.1), V(low.x, low.y, e.max.z - 0.25), V(low.x + 0.05, low.y, low.z)], 0.035, c.kit.alloy));
    filter(V(low.x + 0.12, low.y, low.z));
  } else {
    g.add(tube(V(0.05, e.max.y - 0.15, e.max.z - 0.08), V(e.max.x - 0.12, e.max.y - 0.1, e.max.z - 0.1), 0.035, c.kit.alloy));
    filter(V(e.max.x - 0.05, e.max.y - 0.1, e.max.z - 0.1));
  }
  c.body(g, "FrameFront", "intake");
};

/** Turbo on the exhaust side of the engine (bigger per stage), with a blow-off valve. */
const turbo: Builder = (c, p) => {
  const s = 0.8 + 0.2 * (p.size ?? 1);
  const e = c.frame.engine;
  const at = V(e.min.x + 0.1, e.max.y - 0.18, e.min.z + (e.max.z - e.min.z) * 0.55);
  const g = new THREE.Group();
  const snail = new THREE.Mesh(new THREE.TorusGeometry(0.065 * s, 0.035 * s, 8, 16), c.kit.steel);
  snail.rotation.y = Math.PI / 2;
  snail.position.copy(at);
  g.add(snail);
  g.add(cylX(0.06 * s, 0.09 * s, c.kit.alloy, V(at.x + 0.06 * s, at.y, at.z), 16));
  g.add(cylX(0.045 * s, 0.05, c.kit.hole, V(at.x + 0.11 * s, at.y, at.z), 14));
  const bov = cylY(0.025, 0.05, c.kit.chrome, V(at.x + 0.15, at.y + 0.08, at.z + 0.1));
  bov.rotation.x = Math.PI / 2;
  g.add(bov);
  if ((p.size ?? 1) >= 3) {
    const f = c.frame.bumperFront;
    g.add(bentTube([V(at.x + 0.12, at.y, at.z), V(at.x + 0.12, f.min.y - 0.1, at.z - 0.1), V(at.x + 0.2, f.max.y - 0.15, f.min.z + 0.25)], 0.032, c.kit.alloy));
  }
  c.body(g, "FrameFront", "turbo");
};

/** Supercharger on top of the engine; stage 2 is a big blower through the hood with a scoop. */
const blower: Builder = (c, p) => {
  const e = c.frame.engine;
  const h = c.frame.hood;
  const y = (e.min.y + e.max.y) / 2;
  const g = new THREE.Group();
  const big = (p.size ?? 1) >= 2;
  if (!big) {
    g.add(box(0.28, 0.38, 0.11, c.kit.black, 0, y, e.max.z + 0.04));
    for (let k = 0; k < 6; k++) g.add(box(0.29, 0.015, 0.012, c.kit.alloy, 0, y - 0.15 + k * 0.06, e.max.z + 0.1));
    g.add(cylX(0.07, 0.04, c.kit.chrome, V(0, y + 0.2, e.max.z + 0.02)).rotateZ(Math.PI / 2));
  } else {
    const hoodZ = c.frame.surfaceZ("hood", 0, y, h.max.z);
    const top = hoodZ + 0.2;
    const base = e.max.z;
    g.add(box(0.34, 0.48, top - base, c.kit.chrome, 0, y, (top + base) / 2));
    for (let k = 0; k < 8; k++) g.add(box(0.35, 0.012, 0.03, c.kit.gunmetal, 0, y - 0.2 + k * 0.057, top - 0.06));
    // Scoop hat with butterflies.
    g.add(box(0.3, 0.3, 0.14, c.kit.black, 0, y + 0.02, top + 0.07));
    g.add(box(0.26, 0.012, 0.1, c.kit.hole, 0, y + 0.172, top + 0.07));
    for (const x of [-0.07, 0.07]) g.add(box(0.1, 0.004, 0.08, c.kit.alloy, x, y + 0.176, top + 0.07));
  }
  c.body(g, "FrameFront", "turbo");
};

/** V8 dress-up: chrome rocker covers and a round air cleaner. */
const engineDress: Builder = (c) => {
  const e = c.frame.engine;
  const y = (e.min.y + e.max.y) / 2;
  const len = Math.min(e.max.y - e.min.y, 0.55);
  const g = new THREE.Group();
  for (const sx of [-1, 1]) {
    const cover = box(0.1, len, 0.06, c.kit.chrome, sx * 0.2, y, e.max.z - 0.02);
    cover.rotation.y = sx * 0.4;
    g.add(cover);
  }
  g.add(new THREE.Mesh(new THREE.CylinderGeometry(0.17, 0.17, 0.06, 24).rotateX(Math.PI / 2), c.kit.chrome).translateY(y).translateZ(e.max.z + 0.05));
  g.add(new THREE.Mesh(new THREE.CylinderGeometry(0.15, 0.15, 0.065, 24).rotateX(Math.PI / 2), c.kit.core).translateY(y).translateZ(e.max.z + 0.05));
  c.body(g, "FrameFront", "engine");
};

/** Fuel rails: two red rails with blue fittings along the top of the engine. */
const fuelRail: Builder = (c) => {
  const e = c.frame.engine;
  const y0 = e.min.y + 0.08;
  const y1 = e.max.y - 0.12;
  const g = new THREE.Group();
  for (const sx of [-1, 1]) {
    g.add(tube(V(sx * 0.12, y0, e.max.z), V(sx * 0.12, y1, e.max.z), 0.012, c.kit.red, 8));
    for (const y of [y0, y1]) g.add(box(0.03, 0.03, 0.03, c.kit.blue, sx * 0.12, y, e.max.z));
  }
  c.body(g, "FrameFront", "fuel_system");
};

/** Material looks: recorded in the tweaks and applied by look.ts after every style change. */
const paint: Builder = (c, p) => void (c.tweaks.finish = p.finish);
const vinyl: Builder = (c, p) => void (c.tweaks.vinyl = p.design);
const tint: Builder = (c, p) => void (c.tweaks.tint = p.style === "limo" ? "limo" : "dark");
const glass: Builder = (c, p) => void (c.tweaks.glass = p.style === "poly" ? "poly" : "shatterproof");

/** Engine bay, cabin and material builders by piece name. */
export const CABIN_BUILDERS: Record<string, Builder> = {
  seats,
  interior,
  strip,
  cage,
  shifter,
  hydro_handbrake: hydroHandbrake,
  dash_box: dashBox,
  nitrous,
  fuel_cell: fuelCell,
  window_net: windowNet,
  radiator,
  intercooler,
  oil_cooler: oilCooler,
  intake,
  turbo,
  blower,
  engine_dress: engineDress,
  fuel_rail: fuelRail,
  paint,
  vinyl,
  tint,
  glass,
};
