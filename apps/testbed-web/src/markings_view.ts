// Road markings: white and yellow paint from road_markings() (wasm), built by the same rules
// the sim uses for SurfaceType::PaintedLine, so what you see is what the tyres feel. The mesh
// already lies 2 cm above the banked road surface; a polygon offset keeps it from z-fighting
// with the road at a distance. Fetch once per session. The paint is split into CHUNK_M cells
// (one draw per cell for both colours, via vertex colours; frustum-culled) and, like the city's
// small detail layers, not drawn beyond the quality profile's detail distance (250 m on the
// low / touch profiles), so the whole city's paint is not rasterized every frame on a software
// GL and adds only a handful of draw calls.
import * as THREE from "three";

/** Cell size of the marking chunks, m (sim space). */
const CHUNK_M = 192;
const WHITE = 0xe9e9e4;
const YELLOW = 0xe8b923;

export class MarkingsView {
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = new THREE.Group();
  /** Triangles in the whole city's paint (for automation). */
  triangles = 0;
  private chunks: { mesh: THREE.Mesh; center: THREE.Vector3; radius: number }[] = [];
  private readonly sim = new THREE.Vector3();
  private material: THREE.MeshStandardMaterial | null = null;

  /** Rebuild from road_markings(false) (white) and road_markings(true) (yellow). */
  set(white: Float32Array, yellow: Float32Array): void {
    for (const c of [...this.group.children]) {
      (c as THREE.Mesh).geometry.dispose();
      c.removeFromParent();
    }
    this.material?.dispose();
    this.material = null;
    this.chunks = [];
    this.triangles = (white.length + yellow.length) / 9;
    if (this.triangles === 0) return;
    // Bucket whole triangles of both colours by their first vertex's cell.
    const cells = new Map<string, { pos: Float32Array; at: number; colour: THREE.Color }[]>();
    for (const [pos, hex] of [
      [white, WHITE],
      [yellow, YELLOW],
    ] as const) {
      const colour = new THREE.Color(hex);
      for (let t = 0; t < pos.length; t += 9) {
        const key = `${Math.floor(pos[t] / CHUNK_M)},${Math.floor(pos[t + 1] / CHUNK_M)}`;
        let tris = cells.get(key);
        if (!tris) cells.set(key, (tris = []));
        tris.push({ pos, at: t, colour });
      }
    }
    const mat = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.75 });
    mat.polygonOffset = true;
    mat.polygonOffsetFactor = -3;
    mat.polygonOffsetUnits = -12;
    this.material = mat;
    for (const tris of cells.values()) {
      const arr = new Float32Array(tris.length * 9);
      const col = new Float32Array(tris.length * 9);
      tris.forEach(({ pos, at, colour }, i) => {
        arr.set(pos.subarray(at, at + 9), i * 9);
        for (let v = 0; v < 3; v++) colour.toArray(col, i * 9 + v * 3);
      });
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.BufferAttribute(arr, 3));
      geo.setAttribute("color", new THREE.BufferAttribute(col, 3));
      geo.computeVertexNormals();
      geo.computeBoundingSphere();
      const mesh = new THREE.Mesh(geo, mat);
      this.group.add(mesh);
      const s = geo.boundingSphere!;
      this.chunks.push({ mesh, center: s.center.clone(), radius: s.radius });
    }
  }

  /** Hide chunks beyond `far` m of the camera (`camPos` in Three world space). */
  update(camPos: THREE.Vector3, far: number): void {
    // Three (x, y, z) → sim (x, -z, y): the group sits in SceneView.world (rotated -90° about X).
    this.sim.set(camPos.x, -camPos.z, camPos.y);
    for (const c of this.chunks) c.mesh.visible = c.center.distanceTo(this.sim) - c.radius < far;
  }
}
