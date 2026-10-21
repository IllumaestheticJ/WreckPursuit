// Road markings: white and yellow paint from road_markings() (wasm), built by the same rules
// the sim uses for SurfaceType::PaintedLine, so what you see is what the tyres feel. The mesh
// already lies 2 cm above the banked road surface; a polygon offset keeps it from z-fighting
// with the road at a distance.
//
// Render budget (lead/main-sync): the paint is ~94k triangles city-wide. Drawn as two whole-city
// meshes with culling off, every triangle was shaded every frame wherever the camera was. Now
// the paint is split into square tiles (the city's tile size, Quality.cityTileM, by triangle
// centroid) with white and yellow merged into one vertex-coloured mesh per tile: one draw call
// per tile, frustum-culled, and `update` hides tiles past the city draw distance (where the
// road under them is not drawn either). No small-detail cut-off: distant dashes still show
// from the air.
import * as THREE from "three";
import { CITY_TILE_M } from "./render/city_tiles";
import { tagFeature } from "./perf";

const WHITE = 0xe9e9e4;
const YELLOW = 0xe8b923;

export class MarkingsView {
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = tagFeature(new THREE.Group(), "city");
  /** Triangles in the city's paint, all tiles (for automation). */
  triangles = 0;
  private readonly tiles: THREE.Mesh[] = [];
  /** Per tile, its bounding sphere in Three world space (null until the first update). */
  private spheres: (THREE.Sphere | null)[] = [];
  private material: THREE.MeshStandardMaterial | null = null;
  private readonly lastCam = new THREE.Vector3(Infinity, 0, 0);
  private lastFar = -1;

  /** Rebuild from road_markings(false) (white) and road_markings(true) (yellow), in tiles of
   * `tileM` (the city's, Quality.cityTileM). */
  set(white: Float32Array, yellow: Float32Array, tileM = CITY_TILE_M): void {
    for (const m of this.tiles) {
      m.geometry.dispose();
      m.removeFromParent();
    }
    this.tiles.length = 0;
    this.spheres = [];
    this.lastFar = -1;
    this.triangles = (white.length + yellow.length) / 9;
    if (this.triangles === 0) return;
    if (!this.material) {
      this.material = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.75, vertexColors: true });
      this.material.polygonOffset = true;
      this.material.polygonOffsetFactor = -3;
      this.material.polygonOffsetUnits = -12;
    }
    // Triangles per tile key (sim x, y of the centroid), white then yellow.
    const buckets = new Map<string, { src: Float32Array; tri: number; colour: THREE.Color }[]>();
    for (const [pos, hex] of [
      [white, WHITE],
      [yellow, YELLOW],
    ] as const) {
      const colour = new THREE.Color(hex);
      for (let t = 0; t < pos.length / 9; t++) {
        const o = t * 9;
        const cx = (pos[o] + pos[o + 3] + pos[o + 6]) / 3;
        const cy = (pos[o + 1] + pos[o + 4] + pos[o + 7]) / 3;
        const key = `${Math.floor(cx / tileM)},${Math.floor(cy / tileM)}`;
        let b = buckets.get(key);
        if (!b) buckets.set(key, (b = []));
        b.push({ src: pos, tri: t, colour });
      }
    }
    // Deterministic order (sorted keys), as the city tiles.
    for (const key of [...buckets.keys()].sort()) {
      const tris = buckets.get(key)!;
      const position = new Float32Array(tris.length * 9);
      const color = new Float32Array(tris.length * 9);
      tris.forEach(({ src, tri, colour }, i) => {
        position.set(src.subarray(tri * 9, tri * 9 + 9), i * 9);
        for (let v = 0; v < 3; v++) {
          color[i * 9 + v * 3] = colour.r;
          color[i * 9 + v * 3 + 1] = colour.g;
          color[i * 9 + v * 3 + 2] = colour.b;
        }
      });
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.BufferAttribute(position, 3));
      geo.setAttribute("color", new THREE.BufferAttribute(color, 3));
      geo.computeVertexNormals();
      geo.computeBoundingSphere();
      const mesh = new THREE.Mesh(geo, this.material);
      mesh.name = `markings#${key}`;
      this.group.add(mesh);
      this.tiles.push(mesh);
      this.spheres.push(null);
    }
  }

  /** Paint tiles drawn this frame would be at most this many (frustum culling aside). */
  get tileCount(): number {
    return this.tiles.length;
  }

  /**
   * Hide tiles entirely beyond `far` m of the camera (Three world space: the city draw
   * distance). Redone only when the camera moved more than 10 m or the distance changed.
   */
  update(camPos: THREE.Vector3, far: number): void {
    const d = far;
    if (Math.abs(d - this.lastFar) < 1 && camPos.distanceToSquared(this.lastCam) < 100) return;
    this.lastFar = d;
    this.lastCam.copy(camPos);
    for (let i = 0; i < this.tiles.length; i++) {
      const m = this.tiles[i];
      let s = this.spheres[i];
      if (!s) {
        m.updateWorldMatrix(true, false);
        s = this.spheres[i] = m.geometry.boundingSphere!.clone().applyMatrix4(m.matrixWorld);
      }
      m.visible = s.center.distanceTo(camPos) - s.radius < d;
    }
  }
}
