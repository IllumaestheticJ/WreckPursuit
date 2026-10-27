/**
 * Crease-angle split vertex normals for the batched car body (car-shapes phase 3b: smooth
 * shading). The skinned batch keeps its own vertex layout (`src`, written by skinning); a crease
 * map turns it into a render layout where a vertex is duplicated once per smoothing group of its
 * faces, so faces meeting at less than the crease angle shade smoothly and sharper edges (panel
 * edges, the nose face, sills, lamp recesses) stay crisp.
 *
 * Built once per batch from the rest pose (`buildCrease`): vertices are welded by rest position
 * (UV-band seams and separately built faces join), each face corner gets the set of faces around
 * its welded vertex whose rest normal lies within the crease angle of its own, and corners with
 * the same source vertex and set share a render vertex. `updateCrease` copies the skinned
 * positions through the map and recomputes the normals from each render vertex's fixed face set
 * (area weighted, from the current index, so hidden parts' collapsed faces drop out). Call it
 * only when the batch was re-skinned.
 */

/** Default crease angle, degrees (edges sharper than this stay hard). */
export const CREASE_DEG = 38;

/** A batch's crease map (see the module docs). */
export interface CreaseMap {
  /** Render vertex → source vertex. */
  map: Uint32Array;
  /** Render index: the source index's faces in the same order, remapped. */
  index: Uint16Array | Uint32Array;
  /** Render vertex → its smoothing faces: `faces[offsets[v] .. offsets[v + 1]]`. */
  offsets: Uint32Array;
  faces: Uint32Array;
}

/** Build the crease map of an indexed triangle list at rest (`rest`: xyz per source vertex). */
export function buildCrease(rest: ArrayLike<number>, index: ArrayLike<number>, creaseDeg = CREASE_DEG, weldM = 1e-4): CreaseMap {
  const nv = Math.floor(rest.length / 3);
  const nf = Math.floor(index.length / 3);
  const cosC = Math.cos((creaseDeg * Math.PI) / 180);
  // Weld by rest position.
  const weld = new Uint32Array(nv);
  const byPos = new Map<string, number>();
  const q = (x: number) => Math.round(x / weldM);
  let nw = 0;
  for (let v = 0; v < nv; v++) {
    const key = `${q(rest[v * 3])},${q(rest[v * 3 + 1])},${q(rest[v * 3 + 2])}`;
    let w = byPos.get(key);
    if (w === undefined) byPos.set(key, (w = nw++));
    weld[v] = w;
  }
  // Rest face normals (unit; zero for degenerate faces).
  const fn = new Float32Array(nf * 3);
  for (let f = 0; f < nf; f++) faceNormal(rest, index[f * 3], index[f * 3 + 1], index[f * 3 + 2], fn, f, true);
  // Faces around each welded vertex (CSR).
  const cnt = new Uint32Array(nw + 1);
  for (let i = 0; i < nf * 3; i++) cnt[weld[index[i]] + 1]++;
  for (let w = 0; w < nw; w++) cnt[w + 1] += cnt[w];
  const around = new Uint32Array(nf * 3);
  const fill = cnt.slice(0, nw);
  for (let i = 0; i < nf * 3; i++) {
    const w = weld[index[i]];
    const f = (i / 3) | 0;
    // A face touching the same welded vertex twice (degenerate) is listed once.
    if (fill[w] > cnt[w] && around[fill[w] - 1] === f) continue;
    around[fill[w]++] = f;
  }
  // Render vertices: one per (source vertex, smoothing set).
  const renderOf = new Map<string, number>();
  const map: number[] = [];
  const sets: number[][] = [];
  const out = new Uint32Array(nf * 3);
  const set: number[] = [];
  for (let i = 0; i < nf * 3; i++) {
    const v = index[i];
    const f = (i / 3) | 0;
    const w = weld[v];
    set.length = 0;
    const nx = fn[f * 3];
    const ny = fn[f * 3 + 1];
    const nz = fn[f * 3 + 2];
    const flat = nx === 0 && ny === 0 && nz === 0;
    for (let k = cnt[w]; k < fill[w]; k++) {
      const g = around[k];
      if (g === f) {
        set.push(g);
        continue;
      }
      if (flat) continue;
      const d = nx * fn[g * 3] + ny * fn[g * 3 + 1] + nz * fn[g * 3 + 2];
      if (d >= cosC) set.push(g);
    }
    if (set.length === 0) set.push(f);
    set.sort((a, b) => a - b);
    const key = `${v}:${set.join(",")}`;
    let r = renderOf.get(key);
    if (r === undefined) {
      r = map.length;
      renderOf.set(key, r);
      map.push(v);
      sets.push(set.slice());
    }
    out[i] = r;
  }
  const nr = map.length;
  const offsets = new Uint32Array(nr + 1);
  for (let r = 0; r < nr; r++) offsets[r + 1] = offsets[r] + sets[r].length;
  const faces = new Uint32Array(offsets[nr]);
  for (let r = 0; r < nr; r++) faces.set(sets[r], offsets[r]);
  const index16 = nr <= 65535 ? Uint16Array.from(out) : out;
  return { map: Uint32Array.from(map), index: index16, offsets, faces };
}

/** Normal of face (a, b, c) of `p` into `out[f]` (area weighted, or unit when `unit`). */
function faceNormal(p: ArrayLike<number>, a: number, b: number, c: number, out: Float32Array, f: number, unit: boolean): void {
  const ax = p[a * 3];
  const ay = p[a * 3 + 1];
  const az = p[a * 3 + 2];
  const ux = p[b * 3] - ax;
  const uy = p[b * 3 + 1] - ay;
  const uz = p[b * 3 + 2] - az;
  const vx = p[c * 3] - ax;
  const vy = p[c * 3 + 1] - ay;
  const vz = p[c * 3 + 2] - az;
  let nx = uy * vz - uz * vy;
  let ny = uz * vx - ux * vz;
  let nz = ux * vy - uy * vx;
  if (unit) {
    const l = Math.hypot(nx, ny, nz);
    if (l < 1e-12) {
      nx = ny = nz = 0;
    } else {
      nx /= l;
      ny /= l;
      nz /= l;
    }
  }
  out[f * 3] = nx;
  out[f * 3 + 1] = ny;
  out[f * 3 + 2] = nz;
}

/** Scratch face normals, grown as needed. */
let faceScratch = new Float32Array(0);

/**
 * Copy the skinned source positions `src` into the render `pos` through the map and recompute
 * the render normals `normal` from the current render `index` (collapsed faces count nothing).
 */
export function updateCrease(c: CreaseMap, src: Float32Array, index: ArrayLike<number>, pos: Float32Array, normal: Float32Array): void {
  const nr = c.map.length;
  for (let r = 0; r < nr; r++) {
    const s = c.map[r] * 3;
    pos[r * 3] = src[s];
    pos[r * 3 + 1] = src[s + 1];
    pos[r * 3 + 2] = src[s + 2];
  }
  const nf = Math.floor(index.length / 3);
  if (faceScratch.length < nf * 3) faceScratch = new Float32Array(nf * 3);
  const fn = faceScratch;
  for (let f = 0; f < nf; f++) faceNormal(pos, index[f * 3], index[f * 3 + 1], index[f * 3 + 2], fn, f, false);
  for (let r = 0; r < nr; r++) {
    let x = 0;
    let y = 0;
    let z = 0;
    for (let k = c.offsets[r]; k < c.offsets[r + 1]; k++) {
      const f = c.faces[k] * 3;
      x += fn[f];
      y += fn[f + 1];
      z += fn[f + 2];
    }
    const l = Math.hypot(x, y, z);
    if (l < 1e-12) {
      normal[r * 3] = 0;
      normal[r * 3 + 1] = 0;
      normal[r * 3 + 2] = 1;
    } else {
      normal[r * 3] = x / l;
      normal[r * 3 + 1] = y / l;
      normal[r * 3 + 2] = z / l;
    }
  }
}
