// Junction hole finder + screenshots for docs/media/junctions/ (headless Chromium via Playwright).
// Loads the committed greybox (`data/city/<city>.greybox.glb`) in a bare Three.js page (no wasm
// needed) and, for every junction / bend / dead end listed in a JSON file
// (`[{node, kind, x, y, z, r}]`, written by `cargo test -p city-import --test junction_cover
// -- --ignored dump_junctions`):
//  - renders it top-down (orthographic) with only roads, sidewalks and kerbs drawn on a magenta
//    background, and counts magenta pixels inside the paved radius: each one is a hole;
//  - writes contact sheets (`<prefix>_<city>_sheet_<n>.png`) and, for the worst junctions (or
//    `--nodes a,b,c`), a full-scene top-down shot and a street-level shot.
// Usage: node e2e/junction_shots.mjs <city> <junctions.json> <outDir> [prefix] [--nodes 1,2]
// Env: CHROMIUM=/path/to/chrome; CITY_DIR=dir with the .greybox.glb (default data/city).
import { createServer } from "node:http";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = join(root, "../..");
const args = process.argv.slice(2);
const nodesArg = args.indexOf("--nodes");
const pick = nodesArg >= 0 ? args[nodesArg + 1].split(",").map(Number) : null;
const pos = nodesArg >= 0 ? args.filter((_, i) => i !== nodesArg && i !== nodesArg + 1) : args;
const [city, listPath, outArg, prefix = "before"] = pos;
const out = resolve(outArg ?? join(repo, "docs/media/junctions"));
mkdirSync(out, { recursive: true });
const junctions = JSON.parse(readFileSync(listPath, "utf8"));

const page_html = `<!doctype html><html><head><meta charset="utf-8">
<script type="importmap">{"imports":{"three":"/three/build/three.module.js","three/addons/":"/three/examples/jsm/"}}</script>
</head><body style="margin:0;background:#000">
<script type="module">
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
const renderer = new THREE.WebGLRenderer({ antialias: false, preserveDrawingBuffer: true });
document.body.appendChild(renderer.domElement);
const scene = new THREE.Scene();
scene.add(new THREE.HemisphereLight(0xffffff, 0x445544, 1.6));
const sun = new THREE.DirectionalLight(0xffffff, 1.4);
sun.position.set(300, -200, 600);
scene.add(sun);
const gltf = await new GLTFLoader().loadAsync("/city/${city}.greybox.glb");
const city = gltf.scene;
// glTF is +Y up (x, z, -y); back to the sim's +Z up.
city.rotation.x = Math.PI / 2;
scene.add(city);
const meshes = [];
city.traverse((o) => { if (o.isMesh) meshes.push(o); });
const roadish = new Set(["asphalt", "highway_concrete", "parking", "bus_lane", "sidewalk", "sidewalk_corner", "kerb"]);
function mode(m) {
  for (const o of meshes) {
    const name = o.material.name;
    o.visible = m === "full" || roadish.has(name);
    if (m === "holes" && roadish.has(name)) {
      o.material.side = THREE.FrontSide; // a triangle facing down is a hole from above
    } else {
      o.material.side = THREE.DoubleSide;
    }
  }
  renderer.setClearColor(m === "holes" ? 0xff00ff : 0x9ec4e8);
}
function topDown(j, px, half) {
  renderer.setSize(px, px);
  const cam = new THREE.OrthographicCamera(-half, half, half, -half, 0.1, 400);
  cam.up.set(0, 1, 0);
  cam.position.set(j.x, j.y, j.z + 150);
  cam.lookAt(j.x, j.y, j.z);
  renderer.render(scene, cam);
}
// Holes = magenta (nothing drawn) regions enclosed by road / sidewalk: connected components
// of magenta pixels that touch neither the tile border nor exceed 60 m² (a city block).
function countHoles(j, px, half) {
  const gl = renderer.getContext();
  const buf = new Uint8Array(px * px * 4);
  gl.readPixels(0, 0, px, px, gl.RGBA, gl.UNSIGNED_BYTE, buf);
  const mpp = (2 * half) / px;
  const magenta = new Uint8Array(px * px);
  for (let i = 0; i < px * px; i++) {
    magenta[i] = buf[4 * i] > 240 && buf[4 * i + 1] < 20 && buf[4 * i + 2] > 240 ? 1 : 0;
  }
  const seen = new Uint8Array(px * px);
  let holes = 0;
  const stack = [];
  for (let s = 0; s < px * px; s++) {
    if (!magenta[s] || seen[s]) continue;
    let size = 0, border = false;
    stack.push(s);
    seen[s] = 1;
    while (stack.length) {
      const i = stack.pop();
      size++;
      const x = i % px, y = (i / px) | 0;
      if (x === 0 || y === 0 || x === px - 1 || y === px - 1) border = true;
      for (const n of [i - 1, i + 1, i - px, i + px]) {
        if (n < 0 || n >= px * px || seen[n] || !magenta[n]) continue;
        if ((n % px === 0 && x === px - 1) || (x === 0 && n % px === px - 1)) continue;
        seen[n] = 1;
        stack.push(n);
      }
    }
    const area = size * mpp * mpp;
    if (!border && area < 60) holes += area;
  }
  return holes;
}
window.scan = (list, tile) => {
  mode("holes");
  const sheet = document.createElement("canvas");
  const cols = 8;
  const rows = Math.ceil(list.length / cols);
  sheet.width = cols * tile; sheet.height = rows * tile;
  const ctx = sheet.getContext("2d");
  ctx.font = "12px monospace";
  const res = [];
  list.forEach((j, k) => {
    const half = j.r + 8;
    topDown(j, 400, half);
    const holes = countHoles(j, 400, half);
    res.push({ node: j.node, kind: j.kind, holes_m2: holes });
    ctx.drawImage(renderer.domElement, (k % cols) * tile, Math.floor(k / cols) * tile, tile, tile);
    ctx.fillStyle = holes > 0.01 ? "#ff0" : "#fff";
    ctx.fillText(j.kind[0] + j.node + (holes > 0.01 ? " " + holes.toFixed(2) + "m2" : ""), (k % cols) * tile + 4, Math.floor(k / cols) * tile + 14);
  });
  return { res, png: sheet.toDataURL("image/png") };
};
window.shot = (j, kind) => {
  if (kind === "street") {
    mode("full");
    renderer.setSize(1280, 720);
    const cam = new THREE.PerspectiveCamera(60, 1280 / 720, 0.05, 2000);
    cam.up.set(0, 0, 1);
    const from = j.cam ?? [j.x + j.r + 10, j.y, j.z + 1.2];
    cam.position.set(from[0], from[1], from[2]);
    cam.lookAt(j.x, j.y, j.z);
    renderer.render(scene, cam);
  } else {
    mode(kind === "holes" ? "holes" : "full");
    topDown(j, 900, j.r + 12);
  }
  return renderer.domElement.toDataURL("image/png");
};
window.ready = true;
</script></body></html>`;

const types = { ".js": "text/javascript", ".glb": "model/gltf-binary", ".html": "text/html" };
const server = createServer((req, res) => {
  const url = decodeURIComponent(req.url.split("?")[0]);
  let file = null;
  if (url === "/") {
    res.writeHead(200, { "content-type": "text/html" });
    return res.end(page_html);
  }
  if (url.startsWith("/three/")) file = join(root, "node_modules/three", url.slice(7));
  if (url.startsWith("/city/")) file = join(process.env.CITY_DIR ?? join(repo, "data/city"), url.slice(6));
  try {
    const body = readFileSync(file);
    res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => server.listen(0, r));
const port = server.address().port;

const browser = await chromium.launch({
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
page.on("pageerror", (e) => console.error("page error:", String(e)));
await page.goto(`http://localhost:${port}/`);
await page.waitForFunction(() => window.ready === true, null, { timeout: 180000 });

const save = (name, dataUrl) => {
  writeFileSync(join(out, name), Buffer.from(dataUrl.split(",")[1], "base64"));
  console.log("wrote", name);
};
const all = [];
const per = 48;
for (let s = 0; s * per < junctions.length; s++) {
  const { res, png } = await page.evaluate(([l, t]) => window.scan(l, t), [junctions.slice(s * per, (s + 1) * per), 200]);
  all.push(...res);
  save(`${prefix}_${city}_sheet_${s}.png`, png);
}
const bad = all.filter((r) => r.holes_m2 > 0.01).sort((a, b) => b.holes_m2 - a.holes_m2);
const total = bad.reduce((a, r) => a + r.holes_m2, 0);
console.log(`${city}: ${all.length} junctions/bends/ends scanned, ${bad.length} with holes, ${total.toFixed(2)} m2 total`);
for (const r of bad.slice(0, 30)) console.log(`  ${r.kind} ${r.node}: ${r.holes_m2.toFixed(2)} m2`);
writeFileSync(join(out, `${prefix}_${city}_holes.json`), JSON.stringify(all, null, 1));
const detail = pick ?? bad.slice(0, 3).map((r) => r.node);
for (const n of detail) {
  const j = junctions.find((x) => x.node === n);
  if (!j) continue;
  for (const kind of ["top", "holes", "street"]) {
    save(`${prefix}_${city}_${j.kind}${n}_${kind}.png`, await page.evaluate(([j, k]) => window.shot(j, k), [j, kind]));
  }
}
await browser.close();
server.close();
