// Unit check of render/crease.ts (crease-angle split normals of the smooth car bodies): a cube
// splits every corner (90° edges are hard), a 24-sided cylinder is smooth round its side with
// hard cap edges, and normals follow a moved vertex. Usage: node e2e/crease_check.mjs
// (bundles the module with Vite's esbuild; no browser). Exits 1 on a failure.
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build } from "esbuild";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dir = mkdtempSync(join(tmpdir(), "crease-"));
const out = join(dir, "crease.mjs");
await build({ entryPoints: [join(root, "src/render/crease.ts")], format: "esm", outfile: out, logLevel: "error" });
const { buildCrease, updateCrease } = await import(pathToFileURL(out).href);
rmSync(dir, { recursive: true, force: true });
let failed = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${msg}`);
  if (!ok) failed++;
};

const cubeP = [0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1];
const cubeI = [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
check(buildCrease(cubeP, cubeI).map.length === 24, "cube: 8 corners × 3 hard faces = 24 render vertices");

const n = 24;
const pos = [];
const idx = [];
for (let i = 0; i < n; i++) {
  const a = (2 * Math.PI * i) / n;
  pos.push(Math.cos(a), Math.sin(a), 0, Math.cos(a), Math.sin(a), 1);
}
pos.push(0, 0, 0, 0, 0, 1);
for (let i = 0; i < n; i++) {
  const j = (i + 1) % n;
  idx.push(2 * i, 2 * j, 2 * j + 1, 2 * i, 2 * j + 1, 2 * i + 1, 2 * n, 2 * j, 2 * i, 2 * n + 1, 2 * i + 1, 2 * j + 1);
}
const c = buildCrease(pos, idx);
const rp = new Float32Array(c.map.length * 3);
const rn = new Float32Array(c.map.length * 3);
const src = Float32Array.from(pos);
updateCrease(c, src, c.index, rp, rn);
let side = 0;
let cap = 0;
let bad = 0;
for (let r = 0; r < c.map.length; r++) {
  const z = Math.abs(rn[r * 3 + 2]);
  if (z < 0.05) side++;
  else if (z > 0.95) cap++;
  else bad++;
}
check(side === 2 * n && cap === 2 * n + 2 && bad === 0, `cylinder: ${side} smooth side, ${cap} cap, ${bad} mixed render vertices`);
// Positions follow the source through the map; normals follow a deformation.
check(rp.every((v, k) => v === src[c.map[(k / 3) | 0] * 3 + (k % 3)]), "render positions copy the source");
const before = rn.slice();
src[2 * n * 3 + 5] = 3; // pull the top cap's centre up
updateCrease(c, src, c.index, rp, rn);
check(rn.some((v, k) => Math.abs(v - before[k]) > 0.1), "normals change when a vertex moves");
process.exit(failed ? 1 : 0);
