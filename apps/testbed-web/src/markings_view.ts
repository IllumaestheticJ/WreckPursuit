// Road markings: white and yellow paint from road_markings() (wasm), built by the same rules
// the sim uses for SurfaceType::PaintedLine, so what you see is what the tyres feel. The mesh
// already lies 2 cm above the banked road surface; a polygon offset keeps it from z-fighting
// with the road at a distance. Fetch once per session; two draw calls for the whole city.
import * as THREE from "three";

export class MarkingsView {
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = new THREE.Group();
  /** Triangles drawn (for automation). */
  triangles = 0;

  /** Rebuild from road_markings(false) (white) and road_markings(true) (yellow). */
  set(white: Float32Array, yellow: Float32Array): void {
    for (const c of [...this.group.children]) {
      const m = c as THREE.Mesh;
      m.geometry.dispose();
      (m.material as THREE.Material).dispose();
      m.removeFromParent();
    }
    this.triangles = 0;
    for (const [pos, colour] of [
      [white, 0xe9e9e4],
      [yellow, 0xe8b923],
    ] as const) {
      if (pos.length === 0) continue;
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
      geo.computeVertexNormals();
      const mat = new THREE.MeshStandardMaterial({ color: colour, roughness: 0.75 });
      mat.polygonOffset = true;
      mat.polygonOffsetFactor = -3;
      mat.polygonOffsetUnits = -12;
      const mesh = new THREE.Mesh(geo, mat);
      mesh.frustumCulled = false;
      this.group.add(mesh);
      this.triangles += pos.length / 9;
    }
  }
}
