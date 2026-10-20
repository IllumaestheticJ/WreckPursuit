// City tiles (browser perf pass, docs/integration/browser-perf.md). The greybox glTF holds one
// mesh per material spanning the whole map (Port Vesper v2: ~4 km, 370k triangles), so frustum
// culling never removed anything and the GPU shaded every triangle every frame. `tileCity`
// splits each of those meshes into square tiles (by triangle centroid) that share the original
// material; each tile gets its own bounding sphere, so Three's frustum culling drops what is
// behind / beside the camera and opaque tiles sort front to back (less overdraw).
// `CityTiles.update` additionally hides tiles past a draw distance (fog / setting).
// Geometry only: materials, their names and shader patches (wet roads) are untouched.
import * as THREE from "three";

/** Tile edge, m. ~10 materials × tiles in view stays around a hundred draw calls. */
export const CITY_TILE_M = 320;

/**
 * Tile edge for instanced meshes (trees, driveways, hedges, fences), m: their unit meshes are
 * tiny, so fewer, bigger tiles cost less (draw calls) than the culling saves.
 */
export const INSTANCE_TILE_M = 640;

/** Split one indexed or non-indexed geometry into tiles keyed by the triangle centroid (x, z). */
function splitGeometry(geo: THREE.BufferGeometry, tileM: number): THREE.BufferGeometry[] {
  const pos = geo.getAttribute("position");
  const index = geo.getIndex();
  const triCount = (index ? index.count : pos.count) / 3;
  const vtx = (t: number, k: number) => (index ? index.getX(t * 3 + k) : t * 3 + k);
  // Triangles per tile key.
  const buckets = new Map<string, number[]>();
  for (let t = 0; t < triCount; t++) {
    let cx = 0;
    let cz = 0;
    for (let k = 0; k < 3; k++) {
      const v = vtx(t, k);
      cx += pos.getX(v);
      cz += pos.getZ(v);
    }
    const key = `${Math.floor(cx / 3 / tileM)},${Math.floor(cz / 3 / tileM)}`;
    let b = buckets.get(key);
    if (!b) buckets.set(key, (b = []));
    b.push(t);
  }
  if (buckets.size <= 1) return [geo];
  const names = Object.keys(geo.attributes);
  const out: THREE.BufferGeometry[] = [];
  // Deterministic order (sorted keys) so tile meshes come out the same on every load.
  for (const key of [...buckets.keys()].sort()) {
    const tris = buckets.get(key)!;
    const remap = new Map<number, number>();
    const newIndex: number[] = [];
    for (const t of tris) {
      for (let k = 0; k < 3; k++) {
        const v = vtx(t, k);
        let n = remap.get(v);
        if (n === undefined) remap.set(v, (n = remap.size));
        newIndex.push(n);
      }
    }
    const g = new THREE.BufferGeometry();
    for (const name of names) {
      const src = geo.getAttribute(name) as THREE.BufferAttribute;
      const size = src.itemSize;
      const Arr = (src.array as Float32Array).constructor as new (n: number) => Float32Array;
      const arr = new Arr(remap.size * size);
      for (const [o, n] of remap) for (let c = 0; c < size; c++) arr[n * size + c] = src.array[o * size + c];
      g.setAttribute(name, new THREE.BufferAttribute(arr, size, src.normalized));
    }
    g.setIndex(remap.size > 65535 ? new THREE.Uint32BufferAttribute(newIndex, 1) : new THREE.Uint16BufferAttribute(newIndex, 1));
    g.computeBoundingSphere();
    g.computeBoundingBox();
    out.push(g);
  }
  return out;
}

/** Tiled city: the tile meshes and their world bounding spheres (for distance culling). */
export class CityTiles {
  readonly meshes: THREE.Mesh[] = [];
  private readonly spheres: THREE.Sphere[] = [];
  /** Tiles hidden by the draw distance at the last update. */
  hidden = 0;
  private lastFar = -1;
  private readonly lastCam = new THREE.Vector3(Infinity, 0, 0);

  /** Replace every big mesh under `root` by its tiles (same parent, transform and material). */
  constructor(root: THREE.Object3D, tileM = CITY_TILE_M) {
    const meshes: THREE.Mesh[] = [];
    root.traverse((o) => {
      if ((o as THREE.Mesh).isMesh) meshes.push(o as THREE.Mesh);
    });
    root.updateMatrixWorld(true);
    for (const m of meshes) {
      // Instanced meshes (trees, driveways, hedges, fences: glTF EXT_mesh_gpu_instancing)
      // are split by instance position instead.
      if ((m as THREE.InstancedMesh).isInstancedMesh) {
        this.splitInstances(m as THREE.InstancedMesh, tileM * (INSTANCE_TILE_M / CITY_TILE_M));
        continue;
      }
      const parts = splitGeometry(m.geometry, tileM);
      if (parts.length === 1) {
        this.add(m);
        continue;
      }
      const parent = m.parent!;
      parts.forEach((g, i) => {
        const t = new THREE.Mesh(g, m.material);
        t.name = `${m.name}#${i}`;
        t.position.copy(m.position);
        t.quaternion.copy(m.quaternion);
        t.scale.copy(m.scale);
        t.renderOrder = m.renderOrder;
        t.userData = m.userData;
        parent.add(t);
        t.updateMatrixWorld(true);
        this.add(t);
      });
      m.geometry.dispose();
      m.removeFromParent();
    }
  }

  /**
   * Replace an instanced mesh by one instanced mesh per tile (same geometry and material),
   * each with its instances' bounding sphere, so frustum and distance culling work per tile.
   */
  private splitInstances(m: THREE.InstancedMesh, tileM: number): void {
    const buckets = new Map<string, number[]>();
    const mat = new THREE.Matrix4();
    const pos = new THREE.Vector3();
    for (let i = 0; i < m.count; i++) {
      m.getMatrixAt(i, mat);
      pos.setFromMatrixPosition(mat).applyMatrix4(m.matrixWorld);
      const key = `${Math.floor(pos.x / tileM)},${Math.floor(pos.z / tileM)}`;
      let b = buckets.get(key);
      if (!b) buckets.set(key, (b = []));
      b.push(i);
    }
    const parent = m.parent!;
    [...buckets.keys()].sort().forEach((key, k) => {
      const ids = buckets.get(key)!;
      const t = new THREE.InstancedMesh(m.geometry, m.material, ids.length);
      ids.forEach((src, dst) => {
        m.getMatrixAt(src, mat);
        t.setMatrixAt(dst, mat);
      });
      t.instanceMatrix.needsUpdate = true;
      t.name = `${m.name}#${k}`;
      t.position.copy(m.position);
      t.quaternion.copy(m.quaternion);
      t.scale.copy(m.scale);
      t.userData = m.userData;
      parent.add(t);
      t.updateMatrixWorld(true);
      t.computeBoundingSphere();
      this.meshes.push(t);
      this.spheres.push(t.boundingSphere!.clone().applyMatrix4(t.matrixWorld));
    });
    m.removeFromParent();
  }

  private add(m: THREE.Mesh): void {
    if (!m.geometry.boundingSphere) m.geometry.computeBoundingSphere();
    this.meshes.push(m);
    this.spheres.push(m.geometry.boundingSphere!.clone().applyMatrix4(m.matrixWorld));
  }

  /** Tiles that pass the draw distance and the camera frustum (what the GPU draws). */
  drawn(camera: THREE.Camera): number {
    const f = new THREE.Frustum().setFromProjectionMatrix(
      new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse),
    );
    let n = 0;
    for (let i = 0; i < this.meshes.length; i++) if (this.meshes[i].visible && f.intersectsSphere(this.spheres[i])) n++;
    return n;
  }

  /**
   * Hide tiles entirely beyond `far` m of the camera (Three world space). Cheap; only redone
   * when the camera moved more than 10 m or the distance changed.
   */
  update(camPos: THREE.Vector3, far: number): void {
    if (Math.abs(far - this.lastFar) < 1 && camPos.distanceToSquared(this.lastCam) < 100) return;
    this.lastFar = far;
    this.lastCam.copy(camPos);
    let hidden = 0;
    for (let i = 0; i < this.meshes.length; i++) {
      const s = this.spheres[i];
      const vis = s.center.distanceTo(camPos) - s.radius < far;
      this.meshes[i].visible = vis;
      if (!vis) hidden++;
    }
    this.hidden = hidden;
  }
}
