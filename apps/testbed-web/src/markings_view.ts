// Road markings: white and yellow paint from road_markings() (wasm), built by the same rules
// the sim uses for SurfaceType::PaintedLine, so what you see is what the tyres feel. The mesh
// already lies 2 cm above the banked road surface; a polygon offset keeps it from z-fighting
// with the road at a distance. Fetch once per session. The paint is split into CHUNK_M cells
// (one draw per cell and colour, frustum-culled) and, like the city's small detail layers, not
// drawn beyond the quality profile's detail distance (250 m on the low / touch profiles), so
// the whole city's paint is not rasterized every frame on a software GL.
import * as THREE from "three";

/** Cell size of the marking chunks, m (sim space). */
const CHUNK_M = 192;

export class MarkingsView {
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = new THREE.Group();
  /** Triangles in the whole city's paint (for automation). */
  triangles = 0;
  private chunks: { mesh: THREE.Mesh; center: THREE.Vector3; radius: number }[] = [];
  private readonly sim = new THREE.Vector3();

  /** Rebuild from road_markings(false) (white) and road_markings(true) (yellow). */
  set(white: Float32Array, yellow: Float32Array): void {
    for (const c of [...this.group.children]) {
      const m = c as THREE.Mesh;
      m.geometry.dispose();
      (m.material as THREE.Material).dispose();
      m.removeFromParent();
    }
    this.chunks = [];
    this.triangles = 0;
    for (const [pos, colour] of [
      [white, 0xe9e9e4],
      [yellow, 0xe8b923],
    ] as const) {
      if (pos.length === 0) continue;
      this.triangles += pos.length / 9;
      // Bucket whole triangles by their first vertex's cell.
      const cells = new Map<string, number[]>();
      for (let t = 0; t < pos.length; t += 9) {
        const key = `${Math.floor(pos[t] / CHUNK_M)},${Math.floor(pos[t + 1] / CHUNK_M)}`;
        let idx = cells.get(key);
        if (!idx) cells.set(key, (idx = []));
        idx.push(t);
      }
      const mat = new THREE.MeshStandardMaterial({ color: colour, roughness: 0.75 });
      mat.polygonOffset = true;
      mat.polygonOffsetFactor = -3;
      mat.polygonOffsetUnits = -12;
      for (const idx of cells.values()) {
        const arr = new Float32Array(idx.length * 9);
        idx.forEach((t, i) => arr.set(pos.subarray(t, t + 9), i * 9));
        const geo = new THREE.BufferGeometry();
        geo.setAttribute("position", new THREE.BufferAttribute(arr, 3));
        geo.computeVertexNormals();
        geo.computeBoundingSphere();
        const mesh = new THREE.Mesh(geo, mat);
        this.group.add(mesh);
        const s = geo.boundingSphere!;
        this.chunks.push({ mesh, center: s.center.clone(), radius: s.radius });
      }
    }
  }

  /** Hide chunks beyond `far` m of the camera (`camPos` in Three world space). */
  update(camPos: THREE.Vector3, far: number): void {
    // Three (x, y, z) → sim (x, -z, y): the group sits in SceneView.world (rotated -90° about X).
    this.sim.set(camPos.x, -camPos.z, camPos.y);
    for (const c of this.chunks) c.mesh.visible = c.center.distanceTo(this.sim) - c.radius < far;
  }
}
