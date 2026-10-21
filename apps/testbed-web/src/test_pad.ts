// The handling test pad's course (from Drift Lab B / C, which the unify removed): an open
// asphalt pad with three tight 90° corners and one long sweeper. Paint and cones only (no
// physics): the car drives on the flat world's ground. Used by the tune panel's test pad
// (?drift=w, drift_lab_w.ts).
import * as THREE from "three";

/** `deg` > 0: a right-hand (clockwise) arc; < 0: left-hand. */
type Seg = { kind: "straight"; len: number } | { kind: "arc"; r: number; deg: number };

/** Clockwise loop from the spawn (0, 0) heading +Y; closes on itself (see drift-lab-b.md). */
const COURSE: Seg[] = [
  { kind: "straight", len: 160 },
  { kind: "arc", r: 22, deg: 90 },
  // S-bend (row 107, lift-off transitions): two linked 40 m corners, left 40° → right 80° →
  // left 40°, in place of the 130 m straight (13.6 + 4 · 40 · sin 40° + 13.6 = 130 m, no
  // lateral offset, so the loop still closes).
  { kind: "straight", len: 13.6 },
  { kind: "arc", r: 40, deg: -40 },
  { kind: "arc", r: 40, deg: 80 },
  { kind: "arc", r: 40, deg: -40 },
  { kind: "straight", len: 13.6 },
  { kind: "arc", r: 22, deg: 90 },
  { kind: "straight", len: 160 },
  { kind: "arc", r: 70, deg: 90 }, // the long sweeper
  { kind: "straight", len: 82 },
  { kind: "arc", r: 22, deg: 90 },
  { kind: "straight", len: 48 },
];
const TRACK_W = 14;

/** Centre-line samples [x, y, heading] (heading: direction of travel, rad from +X). */
function centreLine(step = 2): [number, number, number][] {
  const pts: [number, number, number][] = [];
  let x = 0,
    y = 0,
    h = Math.PI / 2;
  pts.push([x, y, h]);
  for (const s of COURSE) {
    if (s.kind === "straight") {
      const n = Math.max(1, Math.round(s.len / step));
      for (let i = 0; i < n; i++) {
        x += (Math.cos(h) * s.len) / n;
        y += (Math.sin(h) * s.len) / n;
        pts.push([x, y, h]);
      }
    } else {
      // Right-hand (clockwise) arc for deg > 0, left-hand for deg < 0.
      const dir = Math.sign(s.deg);
      const a = (Math.abs(s.deg) * Math.PI) / 180;
      const n = Math.max(4, Math.round((s.r * a) / step));
      const cx = x + Math.cos(h - (dir * Math.PI) / 2) * s.r;
      const cy = y + Math.sin(h - (dir * Math.PI) / 2) * s.r;
      const a0 = Math.atan2(y - cy, x - cx);
      for (let i = 1; i <= n; i++) {
        const t = a0 - (dir * a * i) / n;
        x = cx + Math.cos(t) * s.r;
        y = cy + Math.sin(t) * s.r;
        pts.push([x, y, h - (dir * a * i) / n]);
      }
      h -= dir * a;
    }
  }
  return pts;
}

/** A flat strip along `pts` offset `off` m to the left of travel, `w` m wide. */
function strip(pts: [number, number, number][], off: number, w: number, z: number): THREE.BufferGeometry {
  const pos: number[] = [];
  for (const [x, y, h] of pts) {
    const lx = -Math.sin(h),
      ly = Math.cos(h);
    pos.push(x + lx * (off - w / 2), y + ly * (off - w / 2), z, x + lx * (off + w / 2), y + ly * (off + w / 2), z);
  }
  const idx: number[] = [];
  for (let i = 0; i + 1 < pts.length; i++) {
    const a = 2 * i;
    // Counter-clockwise seen from above (+Z), so the strip faces up.
    idx.push(a, a + 2, a + 1, a + 1, a + 2, a + 3);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.Float32BufferAttribute(pos, 3));
  g.setIndex(idx);
  g.computeVertexNormals();
  return g;
}

/** The lab's paint and cones, in sim coordinates (+Z up), for `scene.world`. */
export function buildCourse(): THREE.Group {
  const g = new THREE.Group();
  g.name = "drift-lab-b";
  const pad = new THREE.Mesh(new THREE.PlaneGeometry(420, 420), new THREE.MeshStandardMaterial({ color: 0x3a3c40, roughness: 0.95 }));
  pad.position.set(60, 70, 0.012);
  g.add(pad);
  const pts = centreLine();
  const road = new THREE.Mesh(strip(pts, 0, TRACK_W, 0.016), new THREE.MeshStandardMaterial({ color: 0x2a2b2e, roughness: 0.9 }));
  g.add(road);
  const paint = new THREE.MeshStandardMaterial({ color: 0xf2f2f2, roughness: 0.6 });
  g.add(new THREE.Mesh(strip(pts, TRACK_W / 2, 0.35, 0.02), paint));
  g.add(new THREE.Mesh(strip(pts, -TRACK_W / 2, 0.35, 0.02), paint));
  const kerb = new THREE.MeshStandardMaterial({ color: 0xd03030, roughness: 0.6 });
  // Cones on the inside of every corner (apex markers), every few samples.
  const cone = new THREE.ConeGeometry(0.25, 0.7, 10);
  cone.rotateX(Math.PI / 2);
  cone.translate(0, 0, 0.35);
  for (let i = 1; i < pts.length - 1; i += 3) {
    const turn = pts[i + 1][2] - pts[i - 1][2];
    if (Math.abs(turn) <= 1e-4) continue;
    // Inside of the corner: right of travel in a right-hander (heading decreasing), left in a
    // left-hander.
    const side = turn < 0 ? 1 : -1;
    const [x, y, h] = pts[i];
    const off = side * (TRACK_W / 2 + 0.6);
    const m = new THREE.Mesh(cone, kerb);
    m.position.set(x + Math.sin(h) * off, y - Math.cos(h) * off, 0);
    g.add(m);
  }
  // Start line.
  const start = new THREE.Mesh(new THREE.PlaneGeometry(TRACK_W, 1.2), paint);
  start.position.set(0, 6, 0.022);
  g.add(start);
  return g;
}
