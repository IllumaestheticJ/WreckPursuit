// Static city features (road format v4: walls, fences, trees, lamps, parked cars, containers,
// cranes, decks, ramps, ...) drawn instanced from city_features() (wasm). Prism features are in
// the greybox; boxes and cylinders come here, one THREE.InstancedMesh per (spatial chunk,
// shape, material). Chunks are FEATURE_CHUNK_M squares so frustum culling works on big maps,
// and chunks farther than `drawDistanceM` from the camera are hidden.
//
// Box rotation matches the physics colliders (wp_sim::physics::box_rotation): pitch about the
// local +X axis first (positive = the +Y end up), then yaw about +Z (q = qYaw · qPitch).
// Colours come from feature_palette() (the greybox palette, wp_world::palette); CarPaint
// instances get a deterministic per-instance colour, Light is emissive.
import * as THREE from "three";
import { FEATURE_CHUNK_M, FEATURE_MATERIAL, FEATURE_SHAPE, FEATURE_STRIDE, FT } from "./wasm/layout";
import { layers } from "./settings";

/** Default draw distance for feature chunks, m (`?featureDist=` overrides). */
export const FEATURE_DRAW_DISTANCE_M = 1500;

/** Car paint colours for parked cars (picked by feature index, deterministic). */
const CAR_PAINTS = [0xb8bcc4, 0x23262b, 0xe9ebf0, 0x8a1c1c, 0x1f3f7a, 0x3d5a3a, 0x7a7d82, 0xc9a227, 0x5a3a22, 0x2f6f8f];

interface Chunk {
  group: THREE.Group;
  /** Chunk centre in sim space, m. */
  cx: number;
  cy: number;
}

export class FeaturesView {
  /** Parent in sim space (+Z up); add to SceneView.world. */
  readonly group = new THREE.Group();
  /** Draw distance, m (chunks whose nearest point is farther from the camera are hidden). */
  drawDistanceM = FEATURE_DRAW_DISTANCE_M;
  /** Instances drawn (for automation): boxes, cylinders. */
  boxes = 0;
  cylinders = 0;
  /** InstancedMesh count (draw calls before culling). */
  meshes = 0;
  private chunks: Chunk[] = [];
  private readonly boxGeo = new THREE.BoxGeometry(2, 2, 2);
  private readonly cylGeo = new THREE.CylinderGeometry(1, 1, 2, 10, 1).rotateX(Math.PI / 2);
  private materials: THREE.MeshStandardMaterial[] = [];

  constructor() {
    const d = Number(new URLSearchParams(location.search).get("featureDist"));
    if (d > 0) this.drawDistanceM = d;
  }

  /** Rebuild from city_features() and feature_palette(). Fetch once per session. */
  set(data: Float32Array, palette: Float32Array): void {
    this.clear();
    const n = Math.floor(data.length / FEATURE_STRIDE);
    if (n === 0) return;
    // Materials from the palette (linear RGBA, like the greybox's glTF baseColorFactor).
    const nMat = Math.floor(palette.length / 4);
    for (let m = 0; m < nMat; m++) {
      const c = new THREE.Color().setRGB(palette[4 * m], palette[4 * m + 1], palette[4 * m + 2], THREE.LinearSRGBColorSpace);
      const mat = new THREE.MeshStandardMaterial({ color: m === FEATURE_MATERIAL.carPaint ? 0xffffff : c, roughness: 0.85 });
      mat.name = `feature_${m}`;
      if (m === FEATURE_MATERIAL.light) {
        mat.emissive = c;
        mat.emissiveIntensity = 1.2;
      }
      this.materials.push(mat);
    }
    // Bucket feature indices by chunk, then by (shape, material).
    const buckets = new Map<string, Map<number, number[]>>();
    for (let i = 0; i < n; i++) {
      const o = i * FEATURE_STRIDE;
      const key = `${Math.floor(data[o + FT.cx] / FEATURE_CHUNK_M)},${Math.floor(data[o + FT.cy] / FEATURE_CHUNK_M)}`;
      let chunk = buckets.get(key);
      if (!chunk) buckets.set(key, (chunk = new Map()));
      const sm = data[o + FT.shape] * 256 + Math.min(data[o + FT.material], nMat - 1);
      let list = chunk.get(sm);
      if (!list) chunk.set(sm, (list = []));
      list.push(i);
    }
    const m4 = new THREE.Matrix4();
    const pos = new THREE.Vector3();
    const scale = new THREE.Vector3();
    const qYaw = new THREE.Quaternion();
    const qPitch = new THREE.Quaternion();
    const zAxis = new THREE.Vector3(0, 0, 1);
    const xAxis = new THREE.Vector3(1, 0, 0);
    const colour = new THREE.Color();
    for (const [key, chunk] of buckets) {
      const [ci, cj] = key.split(",").map(Number);
      const group = new THREE.Group();
      group.name = `features_${key}`;
      for (const [sm, list] of chunk) {
        const shape = Math.floor(sm / 256);
        const matIdx = sm % 256;
        const cyl = shape === FEATURE_SHAPE.cylinder;
        const mesh = new THREE.InstancedMesh(cyl ? this.cylGeo : this.boxGeo, this.materials[matIdx], list.length);
        list.forEach((fi, k) => {
          const o = fi * FEATURE_STRIDE;
          pos.set(data[o + FT.cx], data[o + FT.cy], data[o + FT.cz]);
          scale.set(Math.max(data[o + FT.hx], 1e-3), Math.max(data[o + FT.hy], 1e-3), Math.max(data[o + FT.hz], 1e-3));
          qYaw.setFromAxisAngle(zAxis, data[o + FT.yaw]);
          qPitch.setFromAxisAngle(xAxis, data[o + FT.pitch]);
          qYaw.multiply(qPitch);
          m4.compose(pos, qYaw, scale);
          mesh.setMatrixAt(k, m4);
          if (matIdx === FEATURE_MATERIAL.carPaint) {
            // Deterministic colour per feature index (Knuth multiplicative hash).
            const h = Math.imul(fi + 1, 2654435761) >>> 0;
            mesh.setColorAt(k, colour.setHex(CAR_PAINTS[h % CAR_PAINTS.length]));
          }
        });
        mesh.instanceMatrix.needsUpdate = true;
        if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
        mesh.computeBoundingSphere();
        mesh.name = `features_${cyl ? "cyl" : "box"}_${matIdx}`;
        group.add(mesh);
        this.meshes++;
        if (cyl) this.cylinders += list.length;
        else this.boxes += list.length;
      }
      this.group.add(group);
      this.chunks.push({ group, cx: (ci + 0.5) * FEATURE_CHUNK_M, cy: (cj + 0.5) * FEATURE_CHUNK_M });
    }
  }

  /** Per frame: layer toggle + draw distance. `camera` is in Three space (+Y up, sim y = -z). */
  update(camera: THREE.Camera): void {
    this.group.visible = layers.on("features");
    if (!this.group.visible || this.chunks.length === 0) return;
    const p = camera.getWorldPosition(new THREE.Vector3());
    const sx = p.x;
    const sy = -p.z;
    // Distance from the camera to the chunk square (0 inside it).
    const half = FEATURE_CHUNK_M / 2;
    for (const c of this.chunks) {
      const dx = Math.max(Math.abs(sx - c.cx) - half, 0);
      const dy = Math.max(Math.abs(sy - c.cy) - half, 0);
      c.group.visible = dx * dx + dy * dy <= this.drawDistanceM * this.drawDistanceM;
    }
  }

  /** Visible chunks / total (for automation). */
  chunkStats(): { visible: number; total: number } {
    return { visible: this.chunks.filter((c) => c.group.visible).length, total: this.chunks.length };
  }

  private clear(): void {
    for (const c of this.chunks) {
      for (const m of c.group.children) (m as THREE.InstancedMesh).dispose();
      c.group.removeFromParent();
    }
    for (const m of this.materials) m.dispose();
    this.chunks = [];
    this.materials = [];
    this.boxes = 0;
    this.cylinders = 0;
    this.meshes = 0;
  }
}
