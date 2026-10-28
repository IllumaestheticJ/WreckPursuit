// Car lot visit dressing (owner 2026-10-27: "the dealer is poorly built/rendered"). A lot visit
// (crates/wp_wasm/src/lots.rs, `career_visit_lot`) is a flat session whose fences, gate, sign,
// showroom, bunting poles, sales hut and scrap stacks are plain static barriers; the generic
// barrier drawing painted all of them as red-and-white crash barriers. This draws them as what
// they are, per lot kind, on a paved pad with painted parking bays and lamp posts. Every solid
// piece keeps its barrier's exact box (what you see is what you hit); only flat paint, signs,
// hedges, bunting and lamps are extra. Sim space (+Z up, metres); the caller adds `root` to the
// scene's sim-space world group.
import * as THREE from "three";
import type { Barrier } from "../wasm/sim";

/** Parking layout, mirrored from crates/wp_wasm/src/lots.rs (ROW_LEN, SPACE_M, ROW_GAP_M, FIRST_ROW_M). */
const ROW_LEN = 5;
const SPACE_M = 4.8;
const ROW_GAP_M = 11.0;
const FIRST_ROW_M = 16.0;
/** Painted bay depth along the car, m. */
const BAY_LEN_M = 6.0;
/** Gate opening's near edge (lots.rs `front`), m. */
const FRONT_M = -12.0;

/** The lot being drawn. */
export interface LotLook {
  kind: "Dealership" | "UsedLot" | "Junkyard";
  name: string;
  tier: number;
  /** The session's barriers (`lot_fixtures`, in its order). */
  barriers: Barrier[];
}

/** A canvas texture `w`×`h` px drawn by `draw`. */
function canvasTex(w: number, h: number, draw: (g: CanvasRenderingContext2D) => void, repeat: [number, number] = [1, 1]): THREE.CanvasTexture {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  draw(c.getContext("2d")!);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  if (repeat[0] !== 1 || repeat[1] !== 1) {
    t.wrapS = t.wrapT = THREE.RepeatWrapping;
    t.repeat.set(repeat[0], repeat[1]);
  }
  return t;
}

/** Speckled ground: `base` colour with lighter and darker grit. */
function groundTex(base: string, grit: number, seed: number, repeat: [number, number]): THREE.CanvasTexture {
  return canvasTex(
    256,
    256,
    (g) => {
      g.fillStyle = base;
      g.fillRect(0, 0, 256, 256);
      let s = seed;
      const rnd = () => (s = (s * 16807) % 2147483647) / 2147483647;
      for (let i = 0; i < 3500; i++) {
        const v = rnd() < 0.5 ? 0 : 255;
        g.fillStyle = `rgba(${v},${v},${v},${grit * (0.3 + 0.7 * rnd())})`;
        g.fillRect(rnd() * 256, rnd() * 256, 1 + rnd() * 2, 1 + rnd() * 2);
      }
    },
    repeat,
  );
}

/** A sign face: `text` (and an optional second line) on a dark or light panel. */
function signTex(text: string, sub: string, bg: string, fg: string, accent: string): THREE.CanvasTexture {
  return canvasTex(1024, 128, (g) => {
    g.fillStyle = bg;
    g.fillRect(0, 0, 1024, 128);
    g.fillStyle = accent;
    g.fillRect(0, 112, 1024, 16);
    g.fillStyle = fg;
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.font = "700 64px system-ui, sans-serif";
    g.fillText(text.toUpperCase(), 512, sub ? 46 : 58, 980);
    if (sub) {
      g.font = "600 28px system-ui, sans-serif";
      g.fillStyle = accent;
      g.fillText(sub, 512, 92, 980);
    }
  });
}

/** Corrugated sheet metal: vertical ribs with rust streaks. */
function corrugatedTex(len: number): THREE.CanvasTexture {
  return canvasTex(
    128,
    64,
    (g) => {
      for (let x = 0; x < 128; x++) {
        const v = 110 + 40 * Math.sin((x / 128) * Math.PI * 16);
        g.fillStyle = `rgb(${v + 10},${v - 6},${v - 24})`;
        g.fillRect(x, 0, 1, 64);
      }
      for (let i = 0; i < 10; i++) {
        g.fillStyle = "rgba(120,60,20,0.35)";
        g.fillRect((i * 37) % 128, 0, 3, 20 + ((i * 13) % 40));
      }
    },
    [Math.max(1, len / 4), 1],
  );
}

/** Showroom glass front: panes between mullions, lit from inside. */
function glassTex(len: number): THREE.CanvasTexture {
  return canvasTex(
    128,
    128,
    (g) => {
      const grad = g.createLinearGradient(0, 0, 0, 128);
      grad.addColorStop(0, "#9fc4d8");
      grad.addColorStop(1, "#dcecf2");
      g.fillStyle = grad;
      g.fillRect(0, 0, 128, 128);
      g.fillStyle = "#2b3036";
      g.fillRect(0, 0, 6, 128);
      g.fillRect(0, 0, 128, 5);
      g.fillRect(0, 70, 128, 4);
    },
    [Math.max(1, Math.round(len / 3)), 1],
  );
}

const box = (size: [number, number, number], mat: THREE.Material, pos: [number, number, number]): THREE.Mesh => {
  const m = new THREE.Mesh(new THREE.BoxGeometry(...size), mat);
  m.position.set(...pos);
  return m;
};

/** Draws a lot visit's fixtures and ground; `set(null)` clears it. */
export class LotDressing {
  /** Parent under the sim-space world group. */
  readonly root = new THREE.Group();

  constructor() {
    this.root.name = "lot-dressing";
  }

  /** Draw `look` (replacing what was there), or nothing for null. */
  set(look: LotLook | null): void {
    this.clear();
    if (!look || look.barriers.length < 5) return;
    const [left, , back] = look.barriers;
    const halfW = Math.abs(left.pos[0]);
    const backY = back.pos[1];
    const rows = Math.max(1, Math.round((backY - 9 - FIRST_ROW_M) / ROW_GAP_M) + 1);
    this.ground(look.kind, halfW, backY);
    if (look.kind !== "Junkyard") this.bays(look.kind, rows);
    const steel = new THREE.MeshStandardMaterial({ color: 0x2c3036, metalness: 0.6, roughness: 0.45 });
    // Gate posts.
    for (const b of look.barriers.slice(3, 5)) this.root.add(box(b.size, steel, b.pos));
    const rest = look.barriers.slice(5);
    if (look.kind === "Dealership") this.dealership(look, rest, halfW, backY, steel);
    else if (look.kind === "UsedLot") this.usedLot(look, rest);
    else this.junkyard(look, rest);
    for (const b of look.barriers.slice(0, 3)) this.fence(look.kind, b);
    this.lamps(look.kind, halfW, backY);
  }

  private clear(): void {
    this.root.traverse((o) => {
      const m = o as THREE.Mesh;
      if (!m.isMesh) return;
      m.geometry.dispose();
      for (const mat of Array.isArray(m.material) ? m.material : [m.material]) {
        (mat as THREE.MeshStandardMaterial).map?.dispose();
        (mat as THREE.MeshStandardMaterial).emissiveMap?.dispose();
        mat.dispose();
      }
    });
    this.root.clear();
  }

  /** The lot's surface out to the fences and a strip in front of the gate. */
  private ground(kind: LotLook["kind"], halfW: number, backY: number): void {
    const w = 2 * halfW + 1;
    const d = backY - FRONT_M + (kind === "Dealership" ? 0 : 1);
    const tex =
      kind === "Dealership"
        ? groundTex("#8d9196", 0.08, 11, [w / 8, d / 8])
        : kind === "UsedLot"
          ? groundTex("#3d4044", 0.12, 23, [w / 6, d / 6])
          : groundTex("#5d5246", 0.22, 37, [w / 5, d / 5]);
    const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: kind === "Dealership" ? 0.7 : 0.95 });
    const pad = new THREE.Mesh(new THREE.PlaneGeometry(w, d), mat);
    // At road height (the flat world's own ground sits 0.2 m below).
    pad.position.set(0, (backY + FRONT_M) / 2, 0.0);
    pad.receiveShadow = true;
    this.root.add(pad);
  }

  /** Painted parking bays: a line either side of each space, and a stop bar at its head. */
  private bays(kind: LotLook["kind"], rows: number): void {
    const paint = new THREE.MeshBasicMaterial({ color: kind === "Dealership" ? 0xf2f2f2 : 0xe8c547, toneMapped: false });
    paint.polygonOffset = true;
    paint.polygonOffsetFactor = -2;
    const geo = new THREE.PlaneGeometry(0.12, BAY_LEN_M);
    const bar = new THREE.PlaneGeometry(SPACE_M * ROW_LEN, 0.12);
    for (let r = 0; r < rows; r++) {
      const y = FIRST_ROW_M + r * ROW_GAP_M;
      for (let c = 0; c <= ROW_LEN; c++) {
        const line = new THREE.Mesh(geo, paint);
        line.position.set((c - ROW_LEN / 2) * SPACE_M, y, 0.01);
        this.root.add(line);
      }
      const head = new THREE.Mesh(bar, paint);
      head.position.set(0, y + BAY_LEN_M / 2, 0.01);
      this.root.add(head);
    }
    // The drive aisle from the gate: two dashed edge lines.
    for (const x of [-4.5, 4.5]) {
      for (let y = FRONT_M + 2; y < FIRST_ROW_M - BAY_LEN_M / 2 - 1; y += 3) {
        const dash = new THREE.Mesh(new THREE.PlaneGeometry(0.12, 1.5), paint);
        dash.position.set(x, y, 0.01);
        this.root.add(dash);
      }
    }
  }

  /** A side or back fence at its barrier's box. */
  private fence(kind: LotLook["kind"], b: Barrier): void {
    const len = Math.max(b.size[0], b.size[1]);
    if (kind === "Junkyard") {
      const mat = new THREE.MeshStandardMaterial({ map: corrugatedTex(len), metalness: 0.4, roughness: 0.8 });
      this.root.add(box(b.size, mat, b.pos));
      return;
    }
    if (kind === "Dealership") {
      // Low concrete planter with a clipped hedge on top.
      const kerb = new THREE.MeshStandardMaterial({ color: 0xc9cbcc, roughness: 0.85 });
      this.root.add(box(b.size, kerb, b.pos));
      const hedge = new THREE.MeshStandardMaterial({ color: 0x2f5a2c, roughness: 1 });
      const s: [number, number, number] = [b.size[0] * 0.8, b.size[1] * 0.98, 0.45];
      if (b.size[0] > b.size[1]) [s[0], s[1]] = [b.size[0] * 0.98, b.size[1] * 0.8];
      this.root.add(box(s, hedge, [b.pos[0], b.pos[1], b.size[2] + 0.22]));
      return;
    }
    // Used lot: painted kerb, black and white.
    const kerb = canvasTex(64, 8, (g) => {
      g.fillStyle = "#f1f1f1";
      g.fillRect(0, 0, 32, 8);
      g.fillStyle = "#1d1d1d";
      g.fillRect(32, 0, 32, 8);
    }, [len / 2, 1]);
    this.root.add(box(b.size, new THREE.MeshStandardMaterial({ map: kerb, roughness: 0.8 }), b.pos));
  }

  /** Pylons, the gate sign and the glass showroom. */
  private dealership(look: LotLook, rest: Barrier[], halfW: number, backY: number, steel: THREE.Material): void {
    const [p1, p2, sign, showroom] = rest;
    for (const p of [p1, p2]) if (p) this.root.add(box(p.size, steel, p.pos));
    if (sign) {
      const face = signTex(look.name, "★".repeat(look.tier) + "  NEW CARS", "#14171b", "#ffffff", "#3dff8f");
      const faceMat = new THREE.MeshStandardMaterial({ map: face, emissive: 0xffffff, emissiveMap: face, emissiveIntensity: 0.55, roughness: 0.4 });
      const edge = new THREE.MeshStandardMaterial({ color: 0x14171b, roughness: 0.5 });
      // Box faces: +x, -x, +y, -y, +z, -z. The sign reads from the road (-y) and from the lot (+y).
      const m = new THREE.Mesh(new THREE.BoxGeometry(...sign.size), [edge, edge, faceMat, faceMat, edge, edge]);
      m.position.set(...sign.pos);
      this.root.add(m);
    }
    if (showroom) {
      const [sx, sy, sz] = showroom.size;
      const wall = new THREE.MeshStandardMaterial({ color: 0xe9ebec, roughness: 0.6 });
      const glass = glassTex(sx);
      const front = new THREE.MeshStandardMaterial({ map: glass, emissive: 0xffffff, emissiveMap: glass, emissiveIntensity: 0.35, metalness: 0.2, roughness: 0.15 });
      const roof = new THREE.MeshStandardMaterial({ color: 0x55595e, roughness: 0.8 });
      // The front (-y face) is the glass wall facing the cars.
      const m = new THREE.Mesh(new THREE.BoxGeometry(sx, sy, sz), [wall, wall, wall, front, roof, wall]);
      m.position.set(...showroom.pos);
      this.root.add(m);
      // Fascia band over the glass with the name, and a canopy over the doors.
      const fascia = signTex(look.name, "SHOWROOM", "#1b1f24", "#ffffff", "#3dff8f");
      const fasciaMat = new THREE.MeshStandardMaterial({ map: fascia, emissive: 0xffffff, emissiveMap: fascia, emissiveIntensity: 0.5 });
      const band = new THREE.Mesh(new THREE.PlaneGeometry(sx * 0.6, sx * 0.6 * 0.125), fasciaMat);
      band.rotation.x = Math.PI / 2;
      band.position.set(0, showroom.pos[1] - sy / 2 - 0.05, sz - 0.1 - (sx * 0.6 * 0.125) / 2);
      this.root.add(band);
      const canopy = box([8, 2.2, 0.18], roof, [0, showroom.pos[1] - sy / 2 - 1.1, 3.0]);
      this.root.add(canopy);
      // Flags along the front of the lot.
      const colours = [0x3dff8f, 0xffffff, 0x1b1f24];
      for (let i = 0; i < 6; i++) {
        const x = -halfW + 2 + (i * (2 * halfW - 4)) / 5;
        this.root.add(box([0.1, 0.1, 6], steel, [x, backY - 1.2, 3]));
        const flag = new THREE.Mesh(new THREE.PlaneGeometry(1.2, 0.8), new THREE.MeshStandardMaterial({ color: colours[i % 3], side: THREE.DoubleSide }));
        flag.rotation.x = Math.PI / 2;
        flag.position.set(x + 0.6, backY - 1.2, 5.5);
        this.root.add(flag);
      }
    }
  }

  /** Bunting poles strung with pennants, and the sales hut. */
  private usedLot(look: LotLook, rest: Barrier[]): void {
    const hut = rest[rest.length - 1];
    const poles = rest.slice(0, -1);
    const white = new THREE.MeshStandardMaterial({ color: 0xf0f0f0, roughness: 0.6 });
    for (const p of poles) this.root.add(box(p.size, white, p.pos));
    // Pennants along each side, pole to pole, sagging a little.
    const colours = [0xe23b3b, 0xf5c518, 0x2f7de1, 0x3ec46d];
    const tri = new THREE.BufferGeometry().setAttribute("position", new THREE.Float32BufferAttribute([-0.18, 0, 0, 0.18, 0, 0, 0, 0, -0.4], 3));
    tri.computeVertexNormals();
    const mats = colours.map((c) => new THREE.MeshStandardMaterial({ color: c, side: THREE.DoubleSide, roughness: 0.8 }));
    for (const side of [-1, 1]) {
      const ps = poles.filter((p) => Math.sign(p.pos[0]) === side).sort((a, b) => a.pos[1] - b.pos[1]);
      for (let i = 0; i + 1 < ps.length; i++) {
        const a = ps[i].pos;
        const b = ps[i + 1].pos;
        const n = 10;
        for (let k = 0; k <= n; k++) {
          const t = k / n;
          const flag = new THREE.Mesh(tri, mats[(i * n + k) % mats.length]);
          flag.position.set(a[0], a[1] + (b[1] - a[1]) * t, 3.9 - 0.6 * Math.sin(Math.PI * t));
          flag.rotation.z = Math.PI / 2;
          this.root.add(flag);
        }
      }
    }
    if (hut) {
      const [hx, hy, hz] = hut.size;
      const wall = new THREE.MeshStandardMaterial({ color: 0xe8dcc0, roughness: 0.8 });
      const win = canvasTex(128, 64, (g) => {
        g.fillStyle = "#e8dcc0";
        g.fillRect(0, 0, 128, 64);
        g.fillStyle = "#33465a";
        g.fillRect(14, 16, 100, 26);
        g.fillStyle = "#9a3b2b";
        g.fillRect(0, 54, 128, 10);
      });
      const winMat = new THREE.MeshStandardMaterial({ map: win, roughness: 0.6 });
      const roof = new THREE.MeshStandardMaterial({ color: 0x9a3b2b, roughness: 0.9 });
      const m = new THREE.Mesh(new THREE.BoxGeometry(hx, hy, hz), [winMat, winMat, wall, winMat, roof, wall]);
      m.position.set(...hut.pos);
      this.root.add(m);
      const face = signTex(look.name, "USED CARS · CASH TODAY", "#f5c518", "#1d1d1d", "#e23b3b");
      const sign = new THREE.Mesh(new THREE.PlaneGeometry(hx + 1.2, (hx + 1.2) * 0.125), new THREE.MeshStandardMaterial({ map: face, emissive: 0xffffff, emissiveMap: face, emissiveIntensity: 0.3, side: THREE.DoubleSide }));
      sign.rotation.x = Math.PI / 2;
      sign.position.set(hut.pos[0], hut.pos[1] - hy / 2 - 0.02, hz + 0.45);
      this.root.add(sign);
    }
  }

  /** Crushed-car stacks in faded paint and the crane gantry. */
  private junkyard(look: LotLook, rest: Barrier[]): void {
    const gantry = rest.slice(-3);
    const stacks = rest.slice(0, -3);
    const paints = [0x7b2d26, 0x2f4a6b, 0x5c6b3a, 0x8a7a5a, 0x4a4a4a, 0x6b3f6b, 0x9a6a2a, 0x3b5f5f];
    const glass = new THREE.MeshStandardMaterial({ color: 0x1e2226, roughness: 0.5 });
    stacks.forEach((b, i) => {
      const mat = new THREE.MeshStandardMaterial({ color: paints[(i * 5 + 3) % paints.length], metalness: 0.3, roughness: 0.75 });
      this.root.add(box(b.size, mat, b.pos));
      // A dark crushed window band round the middle of each wreck.
      this.root.add(box([b.size[0] * 0.7, b.size[1] + 0.02, b.size[2] * 0.25], glass, [b.pos[0] + (i % 2 ? 0.2 : -0.2), b.pos[1], b.pos[2] + b.size[2] * 0.15]));
    });
    const yellow = canvasTex(64, 64, (g) => {
      g.fillStyle = "#e0a419";
      g.fillRect(0, 0, 64, 64);
      g.fillStyle = "#1d1d1d";
      for (let k = -64; k < 64; k += 16) {
        g.beginPath();
        g.moveTo(k, 64);
        g.lineTo(k + 8, 64);
        g.lineTo(k + 72, 0);
        g.lineTo(k + 64, 0);
        g.fill();
      }
    }, [1, 4]);
    const crane = new THREE.MeshStandardMaterial({ color: 0xe0a419, roughness: 0.6, metalness: 0.3 });
    const hazard = new THREE.MeshStandardMaterial({ map: yellow, roughness: 0.6 });
    gantry.forEach((b, i) => this.root.add(box(b.size, i < 2 ? hazard : crane, b.pos)));
    // A painted board on the gantry beam.
    const beam = gantry[2];
    if (beam) {
      const face = signTex(look.name, "SALVAGE · AS SEEN", "#2a2a2a", "#e0a419", "#e0a419");
      const sign = new THREE.Mesh(new THREE.PlaneGeometry(9, 1.125), new THREE.MeshStandardMaterial({ map: face, side: THREE.DoubleSide, roughness: 0.8 }));
      sign.rotation.x = Math.PI / 2;
      sign.position.set(0, beam.pos[1] - beam.size[1] / 2 - 0.02, beam.pos[2] - 1.0);
      this.root.add(sign);
    }
  }

  /** Lamp posts outside the side fences, heads glowing. */
  private lamps(kind: LotLook["kind"], halfW: number, backY: number): void {
    const pole = new THREE.MeshStandardMaterial({ color: kind === "Dealership" ? 0x2c3036 : 0x5a5f63, metalness: 0.5, roughness: 0.5 });
    const head = new THREE.MeshStandardMaterial({ color: 0xfff4d6, emissive: kind === "Junkyard" ? 0xffb760 : 0xfff4d6, emissiveIntensity: 1.6 });
    const h = kind === "Junkyard" ? 6 : 7.5;
    for (let y = 2; y < backY; y += 16) {
      for (const side of [-1, 1]) {
        const x = side * (halfW + 1.2);
        this.root.add(box([0.18, 0.18, h], pole, [x, y, h / 2]));
        this.root.add(box([1.4, 0.18, 0.12], pole, [x - side * 0.6, y, h]));
        this.root.add(box([0.6, 0.35, 0.12], head, [x - side * 1.1, y, h - 0.1]));
      }
    }
  }
}
