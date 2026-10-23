// Garage workshop: the bay the car stands in. Built from boxes and cylinders in code (no art
// files): concrete floor with painted bay lines, block walls, a roller door, a two-post lift
// whose arms rise with the car, a workbench, a tool chest, a tyre rack, shelving and ceiling
// light strips. Coordinates are sim space (+X right, +Y forward, +Z up, metres), centred on the
// car; the caller adds `root` to the sim world group.
import * as THREE from "three";

/** Bay size, m: width (x), depth (y), height (z). */
export const BAY = { w: 9, d: 11, h: 4.6 } as const;
/** How high the lift raises the car, m. */
export const LIFT_HEIGHT_M = 1.5;

function canvasTexture(size: number, draw: (g: CanvasRenderingContext2D, s: number) => void, repeat: number): THREE.CanvasTexture {
  const c = document.createElement("canvas");
  c.width = c.height = size;
  draw(c.getContext("2d")!, size);
  const t = new THREE.CanvasTexture(c);
  t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.repeat.set(repeat, repeat);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

/** Sealed concrete: speckle, a few oil stains, expansion joints every 2 m. */
function concrete(): THREE.CanvasTexture {
  return canvasTexture(
    512,
    (g, s) => {
      g.fillStyle = "#6b6d6f";
      g.fillRect(0, 0, s, s);
      let seed = 7;
      const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
      for (let i = 0; i < 6000; i++) {
        const v = 90 + rnd() * 40;
        g.fillStyle = `rgba(${v},${v},${v + 2},0.35)`;
        g.fillRect(rnd() * s, rnd() * s, 2, 2);
      }
      for (let i = 0; i < 5; i++) {
        const r = 20 + rnd() * 45;
        const grad = g.createRadialGradient(0, 0, 0, 0, 0, r);
        grad.addColorStop(0, "rgba(20,20,22,0.45)");
        grad.addColorStop(1, "rgba(20,20,22,0)");
        g.save();
        g.translate(rnd() * s, rnd() * s);
        g.fillStyle = grad;
        g.fillRect(-r, -r, 2 * r, 2 * r);
        g.restore();
      }
      g.strokeStyle = "rgba(30,30,32,0.6)";
      g.lineWidth = 2;
      g.strokeRect(1, 1, s - 2, s - 2);
    },
    BAY.w / 2,
  );
}

/** Painted block wall: mortar lines on a grey base, a darker dado band at the bottom. */
function blocks(): THREE.CanvasTexture {
  return canvasTexture(
    256,
    (g, s) => {
      g.fillStyle = "#8a8f94";
      g.fillRect(0, 0, s, s);
      g.strokeStyle = "#7c8186";
      g.lineWidth = 6;
      const rows = 4;
      for (let r = 0; r <= rows; r++) {
        const y = (r * s) / rows;
        g.beginPath();
        g.moveTo(0, y);
        g.lineTo(s, y);
        g.stroke();
        const off = r % 2 ? s / 4 : 0;
        for (let x = off; x < s; x += s / 2) {
          g.beginPath();
          g.moveTo(x, y);
          g.lineTo(x, y + s / rows);
          g.stroke();
        }
      }
    },
    3,
  );
}

const mat = (color: number, extra: THREE.MeshStandardMaterialParameters = {}) =>
  new THREE.MeshStandardMaterial({ color, roughness: 0.8, metalness: 0.05, ...extra });

function box(w: number, d: number, h: number, m: THREE.Material, x: number, y: number, z: number): THREE.Mesh {
  const b = new THREE.Mesh(new THREE.BoxGeometry(w, d, h), m);
  b.position.set(x, y, z + h / 2);
  return b;
}

function cyl(r: number, h: number, m: THREE.Material, x: number, y: number, z: number, axis: "x" | "y" | "z" = "z"): THREE.Mesh {
  const c = new THREE.Mesh(new THREE.CylinderGeometry(r, r, h, 20), m);
  // Three's cylinders run along +Y; turn them onto the asked axis.
  if (axis === "z") c.rotation.x = Math.PI / 2;
  if (axis === "x") c.rotation.z = Math.PI / 2;
  c.position.set(x, y, z);
  return c;
}

/** The workshop bay. `setLift(t)` (0 = down, 1 = up) raises the lift arms. */
export class GarageRoom {
  readonly root = new THREE.Group();
  /** Lift carriage (arms and pads); rises with the car. */
  private readonly carriage = new THREE.Group();
  /** Everything that stays on the floor; lowered while the car is up (the car never moves). */
  readonly floorGroup = new THREE.Group();
  private readonly lights: THREE.PointLight[] = [];
  private lift = 0;

  constructor(carLength = 4.5, carWidth = 1.8) {
    this.root.name = "workshop-room";
    const f = this.floorGroup;
    this.root.add(f);
    const { w, d, h } = BAY;

    // Floor, bay lines and a drain.
    const floor = new THREE.Mesh(new THREE.PlaneGeometry(w, d), mat(0xffffff, { map: concrete(), roughness: 0.65 }));
    floor.position.z = 0.03;
    f.add(floor);
    const paint = mat(0xe8c21c, { roughness: 0.6 });
    for (const sx of [-1, 1]) f.add(box(0.1, d - 1.5, 0.004, paint, sx * (carWidth / 2 + 1.1), 0, 0.03));
    f.add(box(carWidth + 2.3, 0.1, 0.004, paint, 0, -d / 2 + 0.75, 0.03));
    f.add(box(0.6, 0.12, 0.004, mat(0x222326), 0, d / 2 - 2.2, 0.03));

    // Walls (back, left, right) and ceiling; the front is the roller door.
    const wall = mat(0xffffff, { map: blocks() });
    const dado = mat(0x3e4a57);
    f.add(box(w, 0.2, h, wall, 0, d / 2, 0));
    f.add(box(0.2, d, h, wall, -w / 2, 0, 0));
    f.add(box(0.2, d, h, wall, w / 2, 0, 0));
    f.add(box(w - 0.4, 0.02, 1.0, dado, 0, d / 2 - 0.11, 0));
    f.add(box(0.02, d, 1.0, dado, -w / 2 + 0.11, 0, 0));
    f.add(box(0.02, d, 1.0, dado, w / 2 - 0.11, 0, 0));
    f.add(box(w, d, 0.15, mat(0x50555a), 0, 0, h));
    // Roller door: slats on the open side.
    const slat = mat(0x9aa3ab, { metalness: 0.4, roughness: 0.5 });
    for (let z = 0; z < h; z += 0.22) f.add(box(w, 0.06, 0.2, slat, 0, -d / 2, z));

    // Two-post lift: posts beside the car, carriage with four arms and pads.
    const liftBlue = mat(0x1f5fa8, { metalness: 0.3, roughness: 0.45 });
    const steel = mat(0x8a8d90, { metalness: 0.6, roughness: 0.35 });
    const postX = carWidth / 2 + 0.55;
    for (const sx of [-1, 1]) {
      f.add(box(0.32, 0.32, 3.6, liftBlue, sx * postX, 0, 0));
      f.add(box(0.5, 0.5, 0.04, steel, sx * postX, 0, 0.03));
    }
    f.add(box(postX * 2 + 0.32, 0.25, 0.2, liftBlue, 0, 0, 3.6));
    this.root.add(this.carriage);
    for (const sx of [-1, 1]) {
      this.carriage.add(box(0.26, 0.4, 0.5, steel, sx * postX, 0, 0.12));
      for (const sy of [-1, 1]) {
        const reach = postX - 0.35;
        const arm = box(reach, 0.12, 0.08, liftBlue, sx * (postX - reach / 2), sy * 0.55, 0.12);
        this.carriage.add(arm);
        this.carriage.add(cyl(0.09, 0.06, mat(0x1a1a1a), sx * 0.45, sy * 0.95, 0.22));
      }
    }

    // Workbench with a vice and a pegboard, tool chest, tyre rack, shelving.
    const wood = mat(0x7a5a3a, { roughness: 0.9 });
    const red = mat(0xb3261e, { metalness: 0.35, roughness: 0.4 });
    const dark = mat(0x2a2c30);
    const benchX = -w / 2 + 0.45;
    f.add(box(0.8, 2.6, 0.06, wood, benchX, 2.4, 0.9));
    for (const sy of [-1.2, 1.2]) for (const sx of [-0.32, 0.32]) f.add(box(0.06, 0.06, 0.9, dark, benchX + sx, 2.4 + sy, 0));
    f.add(box(0.2, 0.3, 0.15, steel, benchX + 0.2, 1.5, 0.96));
    f.add(box(0.03, 2.4, 1.0, mat(0xc9b18a), -w / 2 + 0.12, 2.4, 1.2));
    f.add(box(0.7, 1.1, 1.05, red, -w / 2 + 0.45, -0.4, 0));
    for (let i = 0; i < 5; i++) f.add(box(0.72, 1.0, 0.012, steel, -w / 2 + 0.45, -0.4, 0.15 + i * 0.18));
    const rubber = mat(0x151515, { roughness: 0.95 });
    for (let i = 0; i < 4; i++) {
      const t = new THREE.Mesh(new THREE.TorusGeometry(0.29, 0.1, 10, 24), rubber);
      t.rotation.y = Math.PI / 2;
      t.position.set(w / 2 - 0.35, -2.8 + i * 0.25, 0.42);
      f.add(t);
    }
    f.add(box(0.5, 1.2, 0.04, steel, w / 2 - 0.35, -2.4, 0.08));
    for (let s = 0; s < 4; s++) f.add(box(0.5, 2.2, 0.04, steel, w / 2 - 0.35, 2.0, 0.4 + s * 0.6));
    for (const sy of [0.95, 3.05]) f.add(box(0.05, 0.05, 2.3, steel, w / 2 - 0.12, sy, 0));
    for (let s = 1; s < 4; s++)
      for (let i = 0; i < 3; i++) f.add(box(0.35, 0.4, 0.3, mat([0x3b6ea5, 0xc58a1f, 0x5a7d3a][i]), w / 2 - 0.35, 1.3 + i * 0.7, 0.42 + s * 0.6));

    // Light strips (emissive) with a few point lights under them.
    const strip = new THREE.MeshStandardMaterial({ color: 0xffffff, emissive: 0xf4f7ff, emissiveIntensity: 2.2 });
    for (const sx of [-1.8, 1.8]) {
      f.add(box(0.18, 3.2, 0.06, strip, sx, 0, h - 0.12));
      const l = new THREE.PointLight(0xf4f7ff, 28, 12, 1.6);
      l.position.set(sx, 0, h - 0.4);
      this.lights.push(l);
      f.add(l);
    }
    const bench = new THREE.PointLight(0xffe7c4, 8, 6, 1.8);
    bench.position.set(benchX + 0.6, 2.4, 2.2);
    this.lights.push(bench);
    f.add(bench);
    void carLength;
  }

  /** Lift position 0 (down) … 1 (up). The car stays put: the floor and posts go down instead,
   * the carriage stays under the car. */
  setLift(t: number): void {
    this.lift = Math.min(1, Math.max(0, t));
    this.floorGroup.position.z = -LIFT_HEIGHT_M * this.lift;
    this.carriage.position.z = -0.04;
  }

  get liftT(): number {
    return this.lift;
  }

  dispose(): void {
    this.root.removeFromParent();
    this.root.traverse((o) => {
      if (o instanceof THREE.Mesh) {
        o.geometry.dispose();
        const m = o.material as THREE.MeshStandardMaterial;
        m.map?.dispose();
        m.dispose();
      }
    });
  }
}
