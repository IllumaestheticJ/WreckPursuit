// Contact sheets of the procedural car meshes (car-realism pass): renders each car from several
// views in headless Chromium with the game's crease-angle smooth shading on the painted body and
// writes ONE compressed JPG per car (or per group), so a whole roster can be reviewed quickly.
//
//   node tools/assets/sheet_shots.mjs <outDir> <car> [car ...]
//
// Env:
//   MESH_DIR=<dir>     meshes to draw (default data/panels/meshes)
//   BEFORE_DIR=<dir>   also draw the same cars from this dir; tiles come in before / after pairs
//   GROUP=<name>       one sheet <name>.jpg with a row per car instead of one sheet per car
//   VIEWS=a,b,...      views (default front34,side,rear34,top,front,rear; groups: front34,side,top)
//   TILE=WxH           tile size in px (default 600x340; groups 420x236)
//   QUALITY=0..1       JPG quality (default 0.8)
//   FLAT=1             flat-shade everything (facet checks)
//   CAM="x,y,z;ax,ay,az[;fov]"  camera position and target (chassis metres) of the view `custom`
// Needs apps/testbed-web/node_modules (three, playwright-core).
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const testbed = join(repo, "apps/testbed-web");
const { chromium } = await import(pathToFileURL(join(testbed, "node_modules/playwright-core/index.mjs")).href);
const [outArg, ...cars] = process.argv.slice(2);
if (!outArg || cars.length === 0) {
  console.error("usage: node tools/assets/sheet_shots.mjs <outDir> <car> [car ...]");
  process.exit(2);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const group = process.env.GROUP || null;
const beforeDir = process.env.BEFORE_DIR ? resolve(process.env.BEFORE_DIR) : null;
const meshDir = process.env.MESH_DIR ? resolve(process.env.MESH_DIR) : join(repo, "data/panels/meshes");
const views = (process.env.VIEWS || (group ? "front34,side,top" : "front34,side,rear34,top,front,rear")).split(",");
const [tw, th] = (process.env.TILE || (group ? "420x236" : "600x340")).split("x").map(Number);
const quality = Number(process.env.QUALITY || 0.8);

const PAGE = `<!doctype html><html><body style="margin:0;background:#9aa4ad">
<script type="importmap">{"imports":{"three":"/three.module.js"}}</script>
<script type="module">
import * as THREE from "three";
const SLOT = { paint: 0x8a8f96, livery: 0x8a8f96, glass: 0x1d2a33, trim: 0x1a1a1c, headlight: 0xf2f4ff,
  taillight: 0xd0141c, rubber: 0x111111, metal: 0x9ea3a8, interior: 0x2a2826, chrome: 0xd8dadc,
  carbon: 0x202226, caliper: 0xc0262d, engine: 0x55585c, lightbar_red: 0xd0141c, lightbar_blue: 0x1446d0,
  lightbar_clear: 0xeeeeee, emissive_amber: 0xff9a1a, grille: 0x0d0d0e, exhaust: 0x6b6158, rim: 0xb5b8bb,
  spring: 0xc0262d, accent: 0xe0e0e0, underglow: 0x00ffff };
const TINT = { paint: "paint", livery: "paint", caliper: "caliper", spring: "spring", rim: "rim", accent: "accent" };
const VIEWS = { front34: [0.62, 0.78, 0.30], rear34: [-0.62, -0.78, 0.30], side: [1, 0, 0.10], top: [0, 0, 1],
  front: [0, 1, 0.12], rear: [0, -1, 0.12], low34: [0.55, 0.82, 0.10], high34: [0.5, 0.6, 0.62], highrear: [-0.5, -0.6, 0.62] };
const CREASE = Math.cos(38 * Math.PI / 180);
// Crease-angle normals over all the given parts together (welded by position), like render/crease.ts.
function creased(parts) {
  const pos = [], fn = [], faceVerts = [];
  const weld = new Map();
  const around = [];
  for (const p of parts) {
    const P = p.positions_mm, I = p.indices;
    for (let f = 0; f < I.length; f += 3) {
      const tri = [];
      for (let k = 0; k < 3; k++) {
        const i = I[f + k] * 3;
        const x = P[i] / 1000 + p.off[0], y = P[i + 1] / 1000 + p.off[1], z = P[i + 2] / 1000 + p.off[2];
        tri.push(x, y, z);
      }
      const ax = tri[3] - tri[0], ay = tri[4] - tri[1], az = tri[5] - tri[2];
      const bx = tri[6] - tri[0], by = tri[7] - tri[1], bz = tri[8] - tri[2];
      let nx = ay * bz - az * by, ny = az * bx - ax * bz, nz = ax * by - ay * bx;
      const l = Math.hypot(nx, ny, nz);
      if (l < 1e-12) continue;
      const fi = fn.length / 4;
      fn.push(nx / l, ny / l, nz / l, l);
      for (let k = 0; k < 3; k++) {
        const key = Math.round(tri[k * 3] * 1e4) + "," + Math.round(tri[k * 3 + 1] * 1e4) + "," + Math.round(tri[k * 3 + 2] * 1e4);
        let w = weld.get(key);
        if (w === undefined) { w = around.length; weld.set(key, w); around.push([]); }
        around[w].push(fi);
        faceVerts.push(w);
      }
      pos.push(...tri);
    }
  }
  const nor = new Float32Array(pos.length);
  for (let c = 0; c < faceVerts.length; c++) {
    const f = (c / 3) | 0;
    const nx = fn[f * 4], ny = fn[f * 4 + 1], nz = fn[f * 4 + 2];
    let sx = 0, sy = 0, sz = 0;
    for (const g of around[faceVerts[c]]) {
      const gx = fn[g * 4], gy = fn[g * 4 + 1], gz = fn[g * 4 + 2];
      if (gx * nx + gy * ny + gz * nz >= CREASE) { const a = fn[g * 4 + 3]; sx += gx * a; sy += gy * a; sz += gz * a; }
    }
    const l = Math.hypot(sx, sy, sz) || 1;
    nor[c * 3] = sx / l; nor[c * 3 + 1] = sy / l; nor[c * 3 + 2] = sz / l;
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute("position", new THREE.BufferAttribute(new Float32Array(pos), 3));
  g.setAttribute("normal", new THREE.BufferAttribute(nor, 3));
  return g;
}
let r = null;
window.shoot = async (mesh, car, views, tw, th, flat) => {
  if (!r) {
    r = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    r.outputColorSpace = THREE.SRGBColorSpace;
  }
  r.setSize(tw, th);
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x9aa4ad);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x50555a, 1.5));
  const sun = new THREE.DirectionalLight(0xffffff, 2.4);
  sun.position.set(4, 6, 9);
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0xdde6ff, 0.7);
  fill.position.set(-6, -3, 2);
  scene.add(fill);
  scene.add(new THREE.Mesh(new THREE.PlaneGeometry(40, 40), new THREE.MeshStandardMaterial({ color: 0x5d646b })));
  const bySlot = {};
  for (const p of mesh.parts) {
    p.off = p.wheel != null && car ? car.wheels[p.wheel].position_m : [0, 0, 0];
    (bySlot[p.material] ||= []).push(p);
  }
  const made = [];
  for (const [slot, parts] of Object.entries(bySlot)) {
    const t = TINT[slot] && mesh.tints && mesh.tints[TINT[slot]];
    const col = t ? new THREE.Color().setRGB(t[0] / 255, t[1] / 255, t[2] / 255, THREE.SRGBColorSpace) : new THREE.Color(SLOT[slot] ?? 0xff00ff);
    const glass = slot === "glass";
    const smooth = !flat && (slot === "paint" || slot === "livery");
    const m = new THREE.MeshStandardMaterial({ color: col, side: THREE.DoubleSide, flatShading: !smooth,
      metalness: slot === "chrome" || slot === "rim" ? 0.6 : (smooth ? 0.35 : 0.1), roughness: glass ? 0.1 : (smooth ? 0.42 : 0.55),
      transparent: glass, opacity: glass ? 0.75 : 1,
      emissive: /light|amber/.test(slot) ? col : 0x000000, emissiveIntensity: 0.35 });
    const g = creased(parts);
    made.push(g, m);
    scene.add(new THREE.Mesh(g, m));
  }
  // Long lens (little perspective), so proportions read like a side elevation / studio shot.
  const cam = new THREE.PerspectiveCamera(11, tw / th, 0.5, 400);
  const len = car ? car.body_size_m[1] : 4.5;
  const zc = 0.5 * (car ? car.body_size_m[2] : 1.4);
  const shots = [];
  for (const v of views) {
    if (v === "custom" && window.customCam) {
      // CAM="x,y,z;ax,ay,az[;fov]" (chassis metres): a free close-up.
      const [pos, at, fov] = window.customCam;
      cam.fov = fov ? fov[0] : 30;
      cam.updateProjectionMatrix();
      cam.up.set(0, 0, 1);
      cam.position.fromArray(pos);
      cam.lookAt(...at);
      r.render(scene, cam);
      shots.push(r.domElement.toDataURL("image/png"));
      cam.fov = 11;
      cam.updateProjectionMatrix();
      continue;
    }
    const dir = VIEWS[v];
    const n = Math.hypot(dir[0], dir[1], dir[2]);
    const d = (v === "front" || v === "rear" ? 3.0 : v === "top" || v === "side" ? 3.35 : 3.3) * Math.max(len, 3.9);
    // Top view: nose to the right.
    cam.up.set(v === "top" ? -1 : 0, 0, v === "top" ? 0 : 1);
    cam.position.set((dir[0] / n) * d, (dir[1] / n) * d, (dir[2] / n) * d + zc);
    cam.lookAt(0, 0, zc);
    r.render(scene, cam);
    shots.push(r.domElement.toDataURL("image/png"));
  }
  for (const x of made) x.dispose();
  return shots;
};
// tiles: [{url, label}] row-major; cols per row.
window.compose = async (tiles, cols, tw, th, quality) => {
  const rows = Math.ceil(tiles.length / cols);
  const c = document.createElement("canvas");
  c.width = cols * tw; c.height = rows * th;
  const ctx = c.getContext("2d");
  for (let i = 0; i < tiles.length; i++) {
    const img = new Image();
    await new Promise((ok) => { img.onload = ok; img.src = tiles[i].url; });
    const x = (i % cols) * tw, y = ((i / cols) | 0) * th;
    ctx.drawImage(img, x, y);
    if (tiles[i].label) {
      ctx.font = "bold 15px sans-serif";
      const w = ctx.measureText(tiles[i].label).width;
      ctx.fillStyle = "rgba(0,0,0,0.55)"; ctx.fillRect(x + 4, y + 4, w + 10, 21);
      ctx.fillStyle = "#fff"; ctx.fillText(tiles[i].label, x + 9, y + 20);
    }
  }
  return c.toDataURL("image/jpeg", quality);
};
window.ready = true;
</script></body></html>`;

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1100, height: 620 } })).newPage();
page.on("pageerror", (e) => console.error(String(e)));
await page.route("**/*", (route) => {
  const url = new URL(route.request().url());
  if (/^\/three\.[a-z.]+\.js$/.test(url.pathname)) {
    return route.fulfill({ contentType: "text/javascript", body: readFileSync(join(testbed, "node_modules/three/build", url.pathname.slice(1))) });
  }
  return route.fulfill({ contentType: "text/html", body: PAGE });
});
await page.goto("http://shots.local/");
await page.waitForFunction(() => window.ready === true);

if (process.env.CAM) {
  const c = process.env.CAM.split(";").map((e) => e.split(",").map(Number));
  await page.evaluate((c) => (window.customCam = c), c);
}
const flat = process.env.FLAT === "1";
const render = async (dir, id) => {
  const mesh = JSON.parse(readFileSync(join(dir, `${id}.json`), "utf8"));
  let car = null;
  for (const d of ["data/cars", "data/cars/parity"]) {
    try {
      car = JSON.parse(readFileSync(join(repo, d, `${id.split("__")[0]}.json`), "utf8"));
      break;
    } catch {}
  }
  return page.evaluate(([m, c, v, w, h, f]) => window.shoot(m, c, v, w, h, f), [mesh, car, views, tw, th, flat]);
};
const save = async (name, tiles, cols) => {
  const url = await page.evaluate(([t, c, w, h, q]) => window.compose(t, c, w, h, q), [tiles, cols, tw, th, quality]);
  const file = join(out, `${name}.jpg`);
  writeFileSync(file, Buffer.from(url.split(",")[1], "base64"));
  console.log(file);
};
const groupTiles = [];
for (const id of cars) {
  const after = await render(meshDir, id);
  const before = beforeDir ? await render(beforeDir, id) : null;
  const tiles = [];
  views.forEach((v, i) => {
    if (before) tiles.push({ url: before[i], label: `${id} · before` });
    tiles.push({ url: after[i], label: before ? `${id} · after` : `${id} · ${v}` });
  });
  if (group) groupTiles.push(...tiles);
  else await save(id, tiles, before ? 2 : 3);
}
if (group) await save(group, groupTiles, views.length * (beforeDir ? 2 : 1));
await browser.close();
