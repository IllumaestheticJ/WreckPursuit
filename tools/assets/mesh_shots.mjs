// Quick look at procedural car meshes without the testbed or the wasm build: renders
// data/panels/meshes/<car>.json (rest pose, greybox materials + the mesh's tints) in headless
// Chromium and writes front / rear 3/4, side, top and greenhouse close-up (cabin, cabin_rear) PNGs
// per car; SHOT_CAM="x,y,z" SHOT_AT="x,y,z" (chassis space, m) adds <car>_custom.png.
//   node tools/assets/mesh_shots.mjs <outDir> <car> [car ...]
// Needs apps/testbed-web/node_modules (three, playwright-core).
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const testbed = join(repo, "apps/testbed-web");
const { chromium } = await import(pathToFileURL(join(testbed, "node_modules/playwright-core/index.mjs")).href);
const [outArg, ...cars] = process.argv.slice(2);
if (!outArg || cars.length === 0) {
  console.error("usage: node tools/assets/mesh_shots.mjs <outDir> <car> [car ...]");
  process.exit(2);
}
const out = resolve(outArg);
mkdirSync(out, { recursive: true });

// Greybox colours per material slot (sRGB hex); tinted slots take the mesh's `tints`.
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
window.shoot = async (mesh, car) => {
  const r = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
  r.setSize(1100, 620);
  r.outputColorSpace = THREE.SRGBColorSpace;
  document.body.innerHTML = "";
  document.body.appendChild(r.domElement);
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x9aa4ad);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x404040, 1.6));
  const sun = new THREE.DirectionalLight(0xffffff, 2.2);
  sun.position.set(4, 6, 9);
  scene.add(sun);
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(40, 40), new THREE.MeshStandardMaterial({ color: 0x5d646b }));
  scene.add(ground);
  const root = new THREE.Group();
  scene.add(root);
  const mats = {};
  const mat = (slot) => {
    if (!mats[slot]) {
      const t = TINT[slot] && mesh.tints && mesh.tints[TINT[slot]];
      const col = t ? new THREE.Color().setRGB(t[0] / 255, t[1] / 255, t[2] / 255, THREE.SRGBColorSpace) : new THREE.Color(SLOT[slot] ?? 0xff00ff);
      const glass = slot === "glass";
      mats[slot] = new THREE.MeshStandardMaterial({ color: col, side: THREE.DoubleSide, flatShading: true,
        metalness: slot === "chrome" || slot === "rim" ? 0.6 : 0.1, roughness: glass ? 0.1 : 0.55,
        transparent: glass, opacity: glass ? 0.75 : 1,
        emissive: /light|amber/.test(slot) ? col : 0x000000, emissiveIntensity: 0.35 });
    }
    return mats[slot];
  };
  for (const p of mesh.parts) {
    const pos = new Float32Array(p.positions_mm.length);
    for (let i = 0; i < pos.length; i++) pos[i] = p.positions_mm[i] / 1000;
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    g.setIndex(p.indices);
    g.computeVertexNormals();
    const m = new THREE.Mesh(g, mat(p.material));
    if (p.wheel != null && car) m.position.fromArray(car.wheels[p.wheel].position_m);
    root.add(m);
  }
  // Chassis space: +X right, +Y forward, +Z up.
  const cam = new THREE.PerspectiveCamera(30, 1100 / 620, 0.1, 200);
  cam.up.set(0, 0, 1);
  const len = car ? car.body_size_m[1] : 4.5;
  const d = 2.15 * len;
  const shots = {};
  for (const [name, dir] of Object.entries({ front: [0.62, 0.78, 0.30], rear: [-0.62, -0.78, 0.30], side: [1, 0, 0.12], top: [0.05, 0.02, 1] })) {
    cam.position.set(dir[0] * d, dir[1] * d, dir[2] * d + 0.6);
    cam.lookAt(0, 0, 0.55);
    r.render(scene, cam);
    shots[name] = r.domElement.toDataURL("image/png");
  }
  // Close-ups of the greenhouse (pillars, glass frames, cowl, mirrors) and the rear quarter.
  const zTop = car ? car.body_size_m[2] : 1.4;
  for (const [name, dir, at] of [["cabin", [0.55, 0.62, 0.42], [0, 0.35 * len * 0.25, 0.7 * zTop]], ["cabin_rear", [-0.62, -0.5, 0.38], [0, -0.1 * len, 0.65 * zTop]]]) {
    const dc = 0.95 * len;
    cam.position.set(at[0] + dir[0] * dc, at[1] + dir[1] * dc, at[2] + dir[2] * dc);
    cam.lookAt(at[0], at[1], at[2]);
    r.render(scene, cam);
    shots[name] = r.domElement.toDataURL("image/png");
  }
  // Optional extra view: SHOT_CAM="x,y,z" SHOT_AT="x,y,z" (chassis space, m) → <car>_custom.png.
  if (window.customCam) {
    cam.position.fromArray(window.customCam[0]);
    cam.lookAt(...window.customCam[1]);
    r.render(scene, cam);
    shots.custom = r.domElement.toDataURL("image/png");
  }
  return shots;
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
if (process.env.SHOT_CAM && process.env.SHOT_AT) {
  const v = (e) => e.split(",").map(Number);
  await page.evaluate((c) => (window.customCam = c), [v(process.env.SHOT_CAM), v(process.env.SHOT_AT)]);
}
for (const id of cars) {
  const mesh = JSON.parse(readFileSync(join(repo, "data/panels/meshes", `${id}.json`), "utf8"));
  let car = null;
  try {
    car = JSON.parse(readFileSync(join(repo, "data/cars", `${id}.json`), "utf8"));
  } catch {}
  const shots = await page.evaluate(([m, c]) => window.shoot(m, c), [mesh, car]);
  for (const [name, url] of Object.entries(shots)) {
    const file = join(out, `${id}_${name}.png`);
    writeFileSync(file, Buffer.from(url.split(",")[1], "base64"));
    console.log(file);
  }
}
await browser.close();
