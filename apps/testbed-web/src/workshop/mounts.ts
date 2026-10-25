// Garage workshop: what each small mesh part is bolted to, so it leaves with that panel.
//
// The car meshes give every part a body region, but lamps and trim often carry a frame or
// quarter region (they ride with the shell in a crash) while they sit on a bolt-on panel: the
// tail lamps of a coupe are set into its rear bumper. Taking that bumper off in the workshop must
// take the lamps with it, or they hang in the air. So each lamp / trim / mirror part gets a
// **host**: the removable panel whose surface it touches, found from the mesh geometry once per
// model. A part is drawn only while its own slot and its host are on the car.
//
// Rules (distances are the 25th percentile over a sample of the part's vertices of the distance
// to the nearest triangle of a panel, so a lens set into a bumper reads as touching it):
// - only panels within CONTACT_M touch the part;
// - when the welded shell (body, sills, quarters, roof) touches it about as closely as the
//   nearest bolt-on panel, it stays on the shell (no host);
// - else the host is the nearest touching bolt-on panel; near-ties go to a bumper, where lamps and
//   grilles live on most cars.
import type { MeshPartJson } from "../render/panels";
import { PANEL_SLOTS, REGION_SLOT } from "./slotmap";

/** A part closer than this to a panel surface is bolted to it, m. */
const CONTACT_M = 0.03;
/** Distances within this of each other count as a tie, m. */
const TIE_M = 0.005;
/** Grid cell for the triangle lookup, m. */
const CELL_M = 0.1;
/** Vertices sampled per part. */
const SAMPLES = 48;

/** Part kinds that are small bolt-ons riding on a panel (the rest are panels, shell or mechanicals). */
const ATTACHED_KINDS = new Set(["light", "detail", "mirror", "lightbar", "decal_carrier"]);

/** One candidate support: a panel's triangles in a uniform grid. */
interface Support {
  /** Bolt-on panel slot, or null for the welded shell. */
  slot: string | null;
  tri: Float32Array;
  grid: Map<string, number[]>;
}

const cellKey = (x: number, y: number, z: number) => `${x},${y},${z}`;

function support(def: MeshPartJson, slot: string | null): Support {
  const p = def.positions_mm;
  const idx = def.indices;
  const tri = new Float32Array(idx.length * 3);
  for (let i = 0; i < idx.length; i++) {
    tri[i * 3] = p[idx[i] * 3] * 1e-3;
    tri[i * 3 + 1] = p[idx[i] * 3 + 1] * 1e-3;
    tri[i * 3 + 2] = p[idx[i] * 3 + 2] * 1e-3;
  }
  const grid = new Map<string, number[]>();
  for (let t = 0; t < idx.length / 3; t++) {
    const o = t * 9;
    const lo = [Infinity, Infinity, Infinity];
    const hi = [-Infinity, -Infinity, -Infinity];
    for (let k = 0; k < 3; k++)
      for (let a = 0; a < 3; a++) {
        lo[a] = Math.min(lo[a], tri[o + k * 3 + a]);
        hi[a] = Math.max(hi[a], tri[o + k * 3 + a]);
      }
    for (let x = Math.floor(lo[0] / CELL_M); x <= Math.floor(hi[0] / CELL_M); x++)
      for (let y = Math.floor(lo[1] / CELL_M); y <= Math.floor(hi[1] / CELL_M); y++)
        for (let z = Math.floor(lo[2] / CELL_M); z <= Math.floor(hi[2] / CELL_M); z++) {
          const key = cellKey(x, y, z);
          const list = grid.get(key);
          if (list) list.push(t);
          else grid.set(key, [t]);
        }
  }
  return { slot, tri, grid };
}

/** Squared distance from point p to triangle t of `tri` (closest point, Ericson 5.1.5). */
function distSq(px: number, py: number, pz: number, tri: Float32Array, t: number): number {
  const o = t * 9;
  const ax = tri[o], ay = tri[o + 1], az = tri[o + 2];
  const abx = tri[o + 3] - ax, aby = tri[o + 4] - ay, abz = tri[o + 5] - az;
  const acx = tri[o + 6] - ax, acy = tri[o + 7] - ay, acz = tri[o + 8] - az;
  const apx = px - ax, apy = py - ay, apz = pz - az;
  const d1 = abx * apx + aby * apy + abz * apz;
  const d2 = acx * apx + acy * apy + acz * apz;
  let qx: number, qy: number, qz: number;
  const at = (u: number, v: number) => {
    qx = ax + abx * u + acx * v;
    qy = ay + aby * u + acy * v;
    qz = az + abz * u + acz * v;
  };
  if (d1 <= 0 && d2 <= 0) at(0, 0);
  else {
    const bpx = px - tri[o + 3], bpy = py - tri[o + 4], bpz = pz - tri[o + 5];
    const d3 = abx * bpx + aby * bpy + abz * bpz;
    const d4 = acx * bpx + acy * bpy + acz * bpz;
    const cpx = px - tri[o + 6], cpy = py - tri[o + 7], cpz = pz - tri[o + 8];
    const d5 = abx * cpx + aby * cpy + abz * cpz;
    const d6 = acx * cpx + acy * cpy + acz * cpz;
    const vc = d1 * d4 - d3 * d2;
    const vb = d5 * d2 - d1 * d6;
    const va = d3 * d6 - d5 * d4;
    if (d3 >= 0 && d4 <= d3) at(1, 0);
    else if (d6 >= 0 && d5 <= d6) at(0, 1);
    else if (vc <= 0 && d1 >= 0 && d3 <= 0) at(d1 / (d1 - d3), 0);
    else if (vb <= 0 && d2 >= 0 && d6 <= 0) at(0, d2 / (d2 - d6));
    else if (va <= 0 && d4 - d3 >= 0 && d5 - d6 >= 0) {
      const w = (d4 - d3) / (d4 - d3 + (d5 - d6));
      at(1 - w, w);
    } else {
      const den = 1 / (va + vb + vc);
      at(vb * den, vc * den);
    }
  }
  const dx = px - qx!, dy = py - qy!, dz = pz - qz!;
  return dx * dx + dy * dy + dz * dz;
}

/** Distance from a point to the support's surface, or Infinity beyond CONTACT_M. */
function nearest(s: Support, x: number, y: number, z: number): number {
  let best = CONTACT_M * CONTACT_M;
  let hit = false;
  const r = CONTACT_M;
  const seen = new Set<number>();
  for (let cx = Math.floor((x - r) / CELL_M); cx <= Math.floor((x + r) / CELL_M); cx++)
    for (let cy = Math.floor((y - r) / CELL_M); cy <= Math.floor((y + r) / CELL_M); cy++)
      for (let cz = Math.floor((z - r) / CELL_M); cz <= Math.floor((z + r) / CELL_M); cz++) {
        const list = s.grid.get(cellKey(cx, cy, cz));
        if (!list) continue;
        for (const t of list) {
          if (seen.has(t)) continue;
          seen.add(t);
          const d = distSq(x, y, z, s.tri, t);
          if (d <= best) {
            best = d;
            hit = true;
          }
        }
      }
  return hit ? Math.sqrt(best) : Infinity;
}

/** 25th-percentile distance of `def`'s sampled vertices to the support (Infinity: not touching). */
function touch(def: MeshPartJson, s: Support): number {
  const p = def.positions_mm;
  const n = p.length / 3;
  const step = Math.max(1, Math.floor(n / SAMPLES));
  const d: number[] = [];
  for (let i = 0; i < n; i += step) d.push(nearest(s, p[i * 3] * 1e-3, p[i * 3 + 1] * 1e-3, p[i * 3 + 2] * 1e-3));
  d.sort((a, b) => a - b);
  return d[Math.floor(d.length / 4)] ?? Infinity;
}

const cache = new Map<string, Map<MeshPartJson, string>>();

/**
 * Host panel slot of every bolt-on mesh part of car model `carId` that has one (parts not in the
 * map stay on the shell). `parts` is the model's mesh part list (data/panels/meshes/<car>.json
 * `parts`, the same objects every time); the result is cached per model.
 */
export function partHosts(carId: string, parts: readonly MeshPartJson[]): Map<MeshPartJson, string> {
  const hit = cache.get(carId);
  if (hit) return hit;
  const supports: Support[] = [];
  for (const def of parts) {
    if (def.wheel != null || def.positions_mm.length === 0) continue;
    if (def.kind === "body") supports.push(support(def, null));
    else if (def.kind === "panel") {
      const slot = def.region ? (REGION_SLOT[def.region] ?? null) : null;
      supports.push(support(def, slot && PANEL_SLOTS.has(slot) ? slot : null));
    }
  }
  const out = new Map<MeshPartJson, string>();
  for (const def of parts) {
    if (def.wheel != null || !ATTACHED_KINDS.has(def.kind) || def.positions_mm.length === 0) continue;
    let shell = Infinity;
    const panels: { slot: string; d: number }[] = [];
    for (const s of supports) {
      const d = touch(def, s);
      if (!Number.isFinite(d)) continue;
      if (s.slot === null) shell = Math.min(shell, d);
      else panels.push({ slot: s.slot, d });
    }
    if (!panels.length) continue;
    panels.sort((a, b) => a.d - b.d);
    const best = panels[0].d;
    if (shell <= best + TIE_M) continue;
    const ties = panels.filter((p) => p.d <= best + TIE_M);
    const host = ties.find((p) => p.slot.endsWith("_bumper")) ?? ties[0];
    out.set(def, host.slot);
  }
  cache.set(carId, out);
  return out;
}
