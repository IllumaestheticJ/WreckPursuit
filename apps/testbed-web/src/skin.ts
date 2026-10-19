// Skinned deformable body: every visual vertex is a linear blend of up to four lattice nodes
// (wp_contracts::SkinBind). Positions are refreshed from lattice_nodes() each frame; normals
// every 4th frame.
//
// Debug cars ship with an empty skin_binding and no visual mesh, so for regular grid lattices
// (like the generated 4×10×3 box lattices) we synthesise a hull binding: the outer faces of the
// grid, one vertex per face corner bound 100 % to its node. It exercises the same code path.
import * as THREE from "three";

export interface SkinData {
  vertexCount: number;
  /** 4 node ids per vertex. */
  nodes: Uint32Array;
  /** 4 weights per vertex. */
  weights: Float32Array;
  /** Triangle indices, or null to render the vertices as points (binding without a mesh). */
  index: Uint32Array | null;
  /** "lattice" for data-driven bindings, "auto-hull" for the synthesised one. */
  source: "lattice" | "auto-hull";
}

/** Build SkinData from skin_binding_indices / skin_binding_weights. */
export function skinFromBinding(indices: Uint32Array, weights: Float32Array): SkinData | null {
  const binds = indices.length / 5;
  if (binds === 0) return null;
  let vertexCount = 0;
  for (let b = 0; b < binds; b++) vertexCount = Math.max(vertexCount, indices[b * 5] + 1);
  const nodes = new Uint32Array(vertexCount * 4);
  const w = new Float32Array(vertexCount * 4);
  for (let b = 0; b < binds; b++) {
    const v = indices[b * 5];
    for (let k = 0; k < 4; k++) {
      nodes[v * 4 + k] = indices[b * 5 + 1 + k];
      w[v * 4 + k] = weights[b * 4 + k];
    }
  }
  return { vertexCount, nodes, weights: w, index: null, source: "lattice" };
}

/** Synthesise a hull skin for a regular grid lattice; null when the lattice is not a grid. */
export function autoHullSkin(rest: Float32Array): SkinData | null {
  const n = rest.length / 3;
  const key = (v: number) => Math.round(v * 1000);
  const axes = [0, 1, 2].map((a) => {
    const vals = new Set<number>();
    for (let i = 0; i < n; i++) vals.add(key(rest[i * 3 + a]));
    return [...vals].sort((x, y) => x - y);
  });
  const [nx, ny, nz] = axes.map((a) => a.length);
  if (nx * ny * nz !== n || nx < 2 || ny < 2 || nz < 2) return null;
  const grid = new Int32Array(n).fill(-1);
  const at = (i: number, j: number, k: number) => grid[(k * ny + j) * nx + i];
  for (let p = 0; p < n; p++) {
    const i = axes[0].indexOf(key(rest[p * 3]));
    const j = axes[1].indexOf(key(rest[p * 3 + 1]));
    const k = axes[2].indexOf(key(rest[p * 3 + 2]));
    grid[(k * ny + j) * nx + i] = p;
  }
  if (grid.some((g) => g < 0)) return null;

  const vertNodes: number[] = [];
  const tris: number[] = [];
  // quad(a,b,c,d) in counter-clockwise order seen from outside.
  const quad = (a: number, b: number, c: number, d: number) => {
    const base = vertNodes.length;
    vertNodes.push(a, b, c, d);
    tris.push(base, base + 1, base + 2, base, base + 2, base + 3);
  };
  for (let j = 0; j < ny - 1; j++)
    for (let i = 0; i < nx - 1; i++) {
      quad(at(i, j, nz - 1), at(i + 1, j, nz - 1), at(i + 1, j + 1, nz - 1), at(i, j + 1, nz - 1)); // top
      quad(at(i, j, 0), at(i, j + 1, 0), at(i + 1, j + 1, 0), at(i + 1, j, 0)); // bottom
    }
  for (let k = 0; k < nz - 1; k++)
    for (let j = 0; j < ny - 1; j++) {
      quad(at(nx - 1, j, k), at(nx - 1, j + 1, k), at(nx - 1, j + 1, k + 1), at(nx - 1, j, k + 1)); // right
      quad(at(0, j, k), at(0, j, k + 1), at(0, j + 1, k + 1), at(0, j + 1, k)); // left
    }
  for (let k = 0; k < nz - 1; k++)
    for (let i = 0; i < nx - 1; i++) {
      quad(at(i, ny - 1, k), at(i, ny - 1, k + 1), at(i + 1, ny - 1, k + 1), at(i + 1, ny - 1, k)); // front
      quad(at(i, 0, k), at(i + 1, 0, k), at(i + 1, 0, k + 1), at(i, 0, k + 1)); // rear
    }
  const vertexCount = vertNodes.length;
  const nodes = new Uint32Array(vertexCount * 4);
  const weights = new Float32Array(vertexCount * 4);
  vertNodes.forEach((node, v) => {
    nodes[v * 4] = node;
    weights[v * 4] = 1;
  });
  return { vertexCount, nodes, weights, index: new Uint32Array(tris), source: "auto-hull" };
}

/** A Three.js object that follows the lattice through a SkinData binding. */
export class SkinnedBody {
  readonly object: THREE.Mesh | THREE.Points;
  private readonly geom = new THREE.BufferGeometry();
  private readonly pos: Float32Array;
  private frame = 0;

  constructor(readonly skin: SkinData, material: THREE.Material) {
    this.pos = new Float32Array(skin.vertexCount * 3);
    this.geom.setAttribute("position", new THREE.BufferAttribute(this.pos, 3));
    if (skin.index) {
      this.geom.setIndex(new THREE.BufferAttribute(skin.index, 1));
      this.object = new THREE.Mesh(this.geom, material);
    } else {
      this.object = new THREE.Points(this.geom, new THREE.PointsMaterial({ size: 0.04, color: 0xd04a2a }));
    }
  }

  /** Blend vertex positions from current lattice nodes (chassis space, xyz per node). */
  update(nodes: Float32Array): void {
    const { nodes: ids, weights, vertexCount } = this.skin;
    for (let v = 0; v < vertexCount; v++) {
      let x = 0;
      let y = 0;
      let z = 0;
      for (let k = 0; k < 4; k++) {
        const w = weights[v * 4 + k];
        if (w === 0) continue;
        const n = ids[v * 4 + k] * 3;
        x += w * nodes[n];
        y += w * nodes[n + 1];
        z += w * nodes[n + 2];
      }
      this.pos[v * 3] = x;
      this.pos[v * 3 + 1] = y;
      this.pos[v * 3 + 2] = z;
    }
    const attr = this.geom.getAttribute("position") as THREE.BufferAttribute;
    attr.needsUpdate = true;
    if (this.skin.index && this.frame++ % 4 === 0) this.geom.computeVertexNormals();
    this.geom.computeBoundingSphere();
  }
}
