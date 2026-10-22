// Sidewalk / lot / building-base screenshots for docs/media/sidewalks/ (playtest #52), headless
// Chromium via Playwright. Loads a greybox (`<CITY_DIR>/<city>.greybox.glb`, default
// data/city) in a bare Three.js page (no wasm needed) and renders fixed street-level and
// top-down views, so the same views can be shot from the old greybox ("before") and the new
// one ("after"). Camera heights are relative to the greybox surface under each point (found by
// a downward ray), so a view stays at eye level when the ground moves.
// Usage: node e2e/sidewalk_shots.mjs <city> <outDir> <prefix>
// Env: CHROMIUM=/path/to/chrome; CITY_DIR=dir with the .greybox.glb (default data/city).
import { createServer } from "node:http";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = join(root, "../..");
const [city = "vesper_v2", outArg, prefix = "after"] = process.argv.slice(2);
const out = resolve(outArg ?? join(repo, "docs/media/sidewalks"));
mkdirSync(out, { recursive: true });

// [name, from [x, y, metres above the surface there], to [x, y, metres above], top-down?]
const VIEWS = {
  vesper_v2: [
    // Lantern Row downtown, south side of a street: kerb, sidewalk, lot, facade (1.5 m back).
    ["downtown_frontage", [-268, -146.5, 1.3], [-256, -160.5, 0.4]],
    ["downtown_frontage_low", [-244, -157.6, 0.45], [-268, -160.8, 0.15]],
    // Lantern Row, a street side without buildings: sidewalk, lot (verge), skirt, ground.
    ["downtown_verge", [-196.5, -232, 1.1], [-187.5, -200, 0.1]],
    ["downtown_verge_low", [-191.6, -230, 0.45], [-187.6, -208, 0.05]],
    // Hollins Drive (11 % grade): a frontage on the slope; the base goes down the hill as a
    // plinth instead of the sidewalk stepping down.
    ["hollins_frontage", [-615.1, 1024.4, 1.4], [-601.8, 1029.4, 0.4]],
    ["hollins_frontage_low", [-600.3, 1017.1, 0.5], [-605.9, 1038.5, 0.2]],
    // Bluff Road where the lot ends at a deck approach: the sidewalk's back edge gets a rail.
    ["bluff_deck_approach_top", [1336.1, 729.4, 45], [1336.1, 729.5, 0], true],
    // Larkspur node 81 from above (the cul-de-sac that left 6° off a street).
    ["larkspur_node81_top", [-1064.6, -108.9, 160], [-1064.6, -108.8, 0], true],
    // A downtown corner from above (corner fill, lots, building line).
    ["downtown_corner_top", [-200, -150, 90], [-200, -149.9, 0], true],
  ],
  procedural_12x12: [
    // A core block: facades 6 m behind the sidewalk, the lot filled flush to them.
    ["grid_frontage", [70, 60, 1.2], [82, 40, 0.4]],
    ["grid_frontage_low", [62, 47.6, 0.45], [88, 41.5, 0.05]],
    ["grid_corner_top", [100, 50, 110], [100, 50.1, 0], true],
  ],
};

const page_html = `<!doctype html><html><head><meta charset="utf-8">
<script type="importmap">{"imports":{"three":"/three/build/three.module.js","three/addons/":"/three/examples/jsm/"}}</script>
</head><body style="margin:0;background:#000">
<script type="module">
import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
renderer.setSize(1280, 720);
document.body.appendChild(renderer.domElement);
const scene = new THREE.Scene();
scene.background = new THREE.Color(0x9fb8cc);
scene.add(new THREE.HemisphereLight(0xffffff, 0x445544, 1.5));
const sun = new THREE.DirectionalLight(0xffffff, 1.6);
sun.position.set(300, -200, 600);
scene.add(sun);
const gltf = await new GLTFLoader().loadAsync("/city/${city}.greybox.glb");
const city = gltf.scene;
// glTF is +Y up (x, z, -y); back to the sim's +Z up.
city.rotation.x = Math.PI / 2;
scene.add(city);
city.updateMatrixWorld(true);
const meshes = [];
city.traverse((o) => {
  if (o.isMesh) {
    meshes.push(o);
    o.material.side = THREE.DoubleSide;
  }
});
const ray = new THREE.Raycaster();
// Height of the drawn surface under (x, y), ignoring roofs (the lowest hit within 3 m of the
// first is ground; buildings stand far taller).
const ground = (x, y) => {
  ray.set(new THREE.Vector3(x, y, 1000), new THREE.Vector3(0, 0, -1));
  const hits = ray.intersectObjects(meshes, false).filter((h) => h.object.material.name !== "building");
  return hits.length ? hits[0].point.z : 0;
};
window.shot = (from, to, top) => {
  const cam = new THREE.PerspectiveCamera(top ? 40 : 62, 1280 / 720, 0.05, 3000);
  cam.up.set(0, top ? 1 : 0, top ? 0 : 1);
  const fz = top ? from[2] + ground(from[0], from[1]) : ground(from[0], from[1]) + from[2];
  const tz = ground(to[0], to[1]) + to[2];
  cam.position.set(from[0], from[1], fz);
  cam.lookAt(to[0], to[1], tz);
  renderer.render(scene, cam);
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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
page.on("pageerror", (e) => console.error("page error:", String(e)));
await page.goto(`http://localhost:${port}/`);
await page.waitForFunction(() => window.ready === true, null, { timeout: 180000 });
for (const [name, from, to, top] of VIEWS[city] ?? []) {
  const png = await page.evaluate(([f, t, tp]) => window.shot(f, t, tp), [from, to, !!top]);
  const file = `${prefix}_${name}.png`;
  writeFileSync(join(out, file), Buffer.from(png.split(",")[1], "base64"));
  console.log("wrote", file);
}
await browser.close();
server.close();
