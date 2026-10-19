// Race course in the 3D world (workstream 15): the next gate as a glowing arch with a
// bobbing arrow, and the route as a faint line on the road. Lives in the scene's Z-up `world`
// group (sim coordinates). Static per race: `setCourse(race_course())` once, `update()` per frame.
import * as THREE from "three";

/** One gate from `race_course()`: centre, travel direction, full width. */
export interface Gate {
  pos: [number, number, number];
  dir: [number, number];
  width: number;
}

/** Parsed `race_course()`: gates, then the route centreline. */
export interface Course {
  gates: Gate[];
  route: [number, number, number][];
}

/** Decode `race_course()`: `[gates, (x, y, z, dx, dy, w) × gates, (x, y, z) × points]`. */
export function parseCourse(a: Float32Array): Course | null {
  if (a.length < 1) return null;
  const n = a[0];
  const gates: Gate[] = [];
  let o = 1;
  for (let i = 0; i < n; i++, o += 6) gates.push({ pos: [a[o], a[o + 1], a[o + 2]], dir: [a[o + 3], a[o + 4]], width: a[o + 5] });
  const route: [number, number, number][] = [];
  for (; o + 2 < a.length; o += 3) route.push([a[o], a[o + 1], a[o + 2]]);
  return { gates, route };
}

const GATE_COLOUR = 0x2ee59d;
const FINISH_COLOUR = 0xffd23f;
const POST_H = 5.5;

export class RaceMarker {
  readonly group = new THREE.Group();
  private readonly arch = new THREE.Group();
  private readonly arrow: THREE.Mesh;
  private readonly postMat = new THREE.MeshBasicMaterial({ color: GATE_COLOUR });
  private readonly beamMat = new THREE.MeshBasicMaterial({ color: GATE_COLOUR, transparent: true, opacity: 0.18, side: THREE.DoubleSide, depthWrite: false });
  private readonly posts: THREE.Mesh[] = [];
  private readonly bar: THREE.Mesh;
  private readonly beam: THREE.Mesh;
  private routeLine: THREE.Line | null = null;
  private course: Course | null = null;
  /** Gate index currently shown (−1 = hidden); e2e reads it. */
  shown = -1;

  constructor(parent: THREE.Object3D) {
    const post = new THREE.BoxGeometry(0.35, 0.35, POST_H);
    for (const side of [-1, 1]) {
      const m = new THREE.Mesh(post, this.postMat);
      m.userData.side = side;
      this.posts.push(m);
      this.arch.add(m);
    }
    this.bar = new THREE.Mesh(new THREE.BoxGeometry(1, 0.3, 0.5), this.postMat);
    this.beam = new THREE.Mesh(new THREE.PlaneGeometry(1, POST_H), this.beamMat);
    this.beam.rotation.x = Math.PI / 2; // plane spans X (across) × Z (up)
    this.arch.add(this.bar, this.beam);
    const cone = new THREE.ConeGeometry(0.9, 1.8, 4);
    cone.rotateX(-Math.PI / 2); // cone tip (+Y) → −Z: points down at the gate
    this.arrow = new THREE.Mesh(cone, this.postMat);
    this.arch.add(this.arrow);
    this.group.add(this.arch);
    this.group.visible = false;
    parent.add(this.group);
  }

  /** New race (or none). */
  setCourse(course: Course | null): void {
    this.course = course;
    this.shown = -1;
    if (this.routeLine) {
      this.group.remove(this.routeLine);
      this.routeLine.geometry.dispose();
      (this.routeLine.material as THREE.Material).dispose();
      this.routeLine = null;
    }
    this.group.visible = !!course && course.gates.length > 0;
    if (!course || course.route.length < 2) return;
    const pts = course.route.map(([x, y, z]) => new THREE.Vector3(x, y, z + 0.25));
    const geo = new THREE.BufferGeometry().setFromPoints(pts);
    const mat = new THREE.LineBasicMaterial({ color: GATE_COLOUR, transparent: true, opacity: 0.45 });
    this.routeLine = new THREE.Line(geo, mat);
    this.group.add(this.routeLine);
  }

  /** Show gate `next` (−1 hides the arch); `final` = it is the finish line. */
  update(next: number, final: boolean, tS: number): void {
    const c = this.course;
    if (!c || next < 0 || next >= c.gates.length) {
      this.arch.visible = false;
      this.shown = -1;
      return;
    }
    this.arch.visible = true;
    const g = c.gates[next];
    if (this.shown !== next) {
      this.shown = next;
      const w = Math.max(6, g.width);
      this.arch.position.set(g.pos[0], g.pos[1], g.pos[2]);
      // Local +Y along travel, +X across the road.
      this.arch.rotation.set(0, 0, Math.atan2(g.dir[1], g.dir[0]) - Math.PI / 2);
      for (const p of this.posts) p.position.set((p.userData.side as number) * w * 0.5, 0, POST_H / 2);
      this.bar.scale.set(w, 1, 1);
      this.bar.position.set(0, 0, POST_H);
      this.beam.scale.set(w, 1, 1);
      this.beam.position.set(0, 0, POST_H / 2);
      const colour = final ? FINISH_COLOUR : GATE_COLOUR;
      this.postMat.color.setHex(colour);
      this.beamMat.color.setHex(colour);
    }
    this.arrow.position.set(0, 0, POST_H + 2.2 + Math.sin(tS * 3) * 0.4);
    this.arrow.rotation.z = tS * 1.5;
  }
}
