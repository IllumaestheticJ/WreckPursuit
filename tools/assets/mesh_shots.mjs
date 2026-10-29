// Quick look at procedural car meshes without the testbed or the wasm build: renders
// data/panels/meshes/<car>.json (rest pose, greybox materials + the mesh's tints) in headless
// Chromium and writes front / rear 3/4, side, top and greenhouse close-up (cabin, cabin_rear) PNGs
// per car; SHOT_CAM="x,y,z" SHOT_AT="x,y,z" (chassis space, m) adds <car>_custom.png;
// MESH_DIR=<dir> renders <dir>/<id>.json (ids `<car>__<variant>` use <car>'s def).
//   node tools/assets/mesh_shots.mjs <outDir> <car> [car ...]
//
// Painted cars (mesh format 5 with `detail`): the detail texture (data/panels/<detail.file>) is
// drawn through the parts' second UV set (`duvs` → `uv1`) with the shader rule of
// data/panels/README.md "Detail texture": albedo = tex.rgb · mix(1, paint, tex.a), lamps glow
// tex.rgb. SHOT_FLAT=1 keeps flat shading for them too (default: smooth, like the game).
// SHOT_NOTEX=1 renders them without the texture (shape checks).
//
// SHOT_CAMS="name:x,y,z:x,y,z;…" adds one <car>_<name>.png per camera (SHOT_FOV degrees);
// SHOT_BG / SHOT_GROUND set the background / ground colour (hex).
//
// Orthographic fit checks: SHOT_ORTHO=front,side,top,rear (any subset) adds <car>_ortho_<view>.png
// at a fixed scale (SHOT_PX_PER_M, default 200 px/m) with a 0.5 m grid (red lines every 1 m,
// origin = chassis origin; side view looks at the car's left, nose to the left);
// SHOT_REF="side=<png>,front=<png>,…" lays that reference image over the view (stretched to the
// frame; SHOT_REF_ALPHA, default 0.45), so a reference drawn at the same scale lines up.
// Reference images stay local (scratchpad), never in the repo.
//
// Needs three (apps/testbed-web/node_modules, or THREE_DIR=<dir of three.module.js>) and
// playwright-core (apps/testbed-web/node_modules, or PLAYWRIGHT_CORE=<package dir>; falls back
// to the globally installed `playwright`).
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const testbed = join(repo, "apps/testbed-web");
const pwCandidates = [
  process.env.PLAYWRIGHT_CORE && join(process.env.PLAYWRIGHT_CORE, "index.mjs"),
  join(testbed, "node_modules/playwright-core/index.mjs"),
  "/opt/node22/lib/node_modules/playwright/index.mjs",
].filter(Boolean);
const pwPath = pwCandidates.find((p) => existsSync(p));
if (!pwPath) {
  console.error("playwright-core not found (set PLAYWRIGHT_CORE)");
  process.exit(2);
}
const { chromium } = await import(pathToFileURL(pwPath).href);
const threeDir = process.env.THREE_DIR ?? join(testbed, "node_modules/three/build");
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
const PAINT = new Set(["paint", "livery"]);
const LAMP = new Set(["headlight", "taillight", "emissive_amber"]);
window.shoot = async (mesh, car, detailUrl, opts) => {
  const W = 1100, H = 620;
  const r = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
  r.setSize(W, H);
  r.outputColorSpace = THREE.SRGBColorSpace;
  document.body.innerHTML = "";
  document.body.appendChild(r.domElement);
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(opts.bg ?? 0x9aa4ad);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x404040, 1.6));
  const sun = new THREE.DirectionalLight(0xffffff, 2.2);
  sun.position.set(4, 6, 9);
  scene.add(sun);
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(40, 40), new THREE.MeshStandardMaterial({ color: opts.ground ?? 0x5d646b }));
  scene.add(ground);
  const root = new THREE.Group();
  scene.add(root);
  let detail = null;
  if (detailUrl) {
    detail = await new THREE.TextureLoader().loadAsync(detailUrl);
    detail.colorSpace = THREE.SRGBColorSpace;
    detail.channel = 1;
    detail.flipY = false;
    detail.magFilter = THREE.NearestFilter;
    detail.minFilter = THREE.LinearMipmapLinearFilter;
    detail.anisotropy = 4;
  }
  const mats = {};
  const mat = (slot, textured) => {
    const key = slot + (textured ? "#tex" : "");
    if (!mats[key]) {
      const t = TINT[slot] && mesh.tints && mesh.tints[TINT[slot]];
      let col = t ? new THREE.Color().setRGB(t[0] / 255, t[1] / 255, t[2] / 255, THREE.SRGBColorSpace) : new THREE.Color(SLOT[slot] ?? 0xff00ff);
      const glass = slot === "glass";
      const lamp = LAMP.has(slot);
      // Textured non-paint parts take their colour from the texture.
      if (textured && !PAINT.has(slot)) col = new THREE.Color(0xffffff);
      const m = new THREE.MeshStandardMaterial({ color: col, side: THREE.DoubleSide, flatShading: !textured || opts.flat,
        metalness: slot === "chrome" || slot === "rim" ? 0.6 : 0.1, roughness: glass ? 0.1 : textured && PAINT.has(slot) ? 0.42 : 0.55,
        transparent: glass, opacity: glass ? 0.75 : 1,
        emissive: lamp ? (textured ? new THREE.Color(0xffffff) : col) : /light|amber/.test(slot) ? col : 0x000000,
        emissiveIntensity: textured && lamp ? 0.25 : 0.35 });
      if (textured) {
        m.map = detail;
        if (lamp) m.emissiveMap = detail;
        // Detail rule: albedo = tex.rgb * mix(1, paint, tex.a) (non-paint parts: colour white).
        m.onBeforeCompile = (sh) => {
          sh.fragmentShader = sh.fragmentShader.replace("#include <map_fragment>",
            "vec4 dtex = texture2D( map, vMapUv ); diffuseColor.rgb = dtex.rgb * mix( vec3( 1.0 ), diffuseColor.rgb, dtex.a );");
        };
        m.customProgramCacheKey = () => "detail";
      }
      mats[key] = m;
    }
    return mats[key];
  };
  for (const p of mesh.parts) {
    const pos = new Float32Array(p.positions_mm.length);
    for (let i = 0; i < pos.length; i++) pos[i] = p.positions_mm[i] / 1000;
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    const textured = !!(detail && p.duvs && p.duvs.length);
    if (textured) {
      const uv = new Float32Array(p.duvs.length);
      for (let i = 0; i < uv.length; i++) uv[i] = p.duvs[i] / 65535;
      g.setAttribute("uv1", new THREE.BufferAttribute(uv, 2));
    }
    g.setIndex(p.indices);
    g.computeVertexNormals();
    const m = new THREE.Mesh(g, mat(p.material, textured));
    if (p.wheel != null && car) m.position.fromArray(car.wheels[p.wheel].position_m);
    root.add(m);
    // SHOT_WIRE=1: overlay the triangle edges (for facet / sliver checks).
    if (opts.wire) {
      const w = new THREE.LineSegments(new THREE.WireframeGeometry(g), new THREE.LineBasicMaterial({ color: 0x000000, transparent: true, opacity: 0.35 }));
      w.position.copy(m.position);
      root.add(w);
    }
  }
  // Chassis space: +X right, +Y forward, +Z up.
  const cam = new THREE.PerspectiveCamera(30, W / H, 0.1, 200);
  cam.up.set(0, 0, 1);
  const len = car ? car.body_size_m[1] : 4.5;
  const d = 2.15 * len;
  const shots = {};
  if (!opts.orthoOnly) {
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
  }
  // Optional extra view: SHOT_CAM="x,y,z" SHOT_AT="x,y,z" (chassis space, m) → <car>_custom.png.
  if (opts.customCam) {
    cam.position.fromArray(opts.customCam[0]);
    cam.lookAt(...opts.customCam[1]);
    r.render(scene, cam);
    shots.custom = r.domElement.toDataURL("image/png");
  }
  // SHOT_CAMS="name:x,y,z:x,y,z;…" → <car>_<name>.png per entry (SHOT_FOV, default 30°).
  for (const [name, at, to] of opts.cams) {
    cam.fov = opts.fov;
    cam.updateProjectionMatrix();
    cam.position.fromArray(at);
    cam.lookAt(...to);
    r.render(scene, cam);
    shots[name] = r.domElement.toDataURL("image/png");
  }
  // Orthographic views at a fixed metric scale with a grid (and an optional reference overlay).
  if (opts.ortho.length) {
    const s = opts.pxPerM;
    const hw = W / (2 * s), hh = H / (2 * s);
    const grid = new THREE.Group();
    scene.add(grid);
    ground.visible = false;
    for (const view of opts.ortho) {
      grid.clear();
      // Camera looks along -dir; (u, v) = the frame axes in chassis space; centre of the frame.
      const V = {
        side: { pos: [-20, 0, 0.7], up: [0, 0, 1], u: [0, 1, 0], v: [0, 0, 1], c: [0, 0, 0.7] },
        front: { pos: [0, 20, 0.7], up: [0, 0, 1], u: [1, 0, 0], v: [0, 0, 1], c: [0, 0, 0.7] },
        rear: { pos: [0, -20, 0.7], up: [0, 0, 1], u: [-1, 0, 0], v: [0, 0, 1], c: [0, 0, 0.7] },
        top: { pos: [0, 0, 20], up: [-1, 0, 0], u: [0, 1, 0], v: [-1, 0, 0], c: [0, 0, 0] },
      }[view];
      if (!V) continue;
      const oc = new THREE.OrthographicCamera(-hw, hw, hh, -hh, 0.1, 100);
      oc.up.fromArray(V.up);
      oc.position.fromArray(V.pos);
      oc.lookAt(...V.c);
      // Grid in the plane through the chassis origin facing the camera, 0.5 m steps.
      const pt = (a, b) => new THREE.Vector3(V.c[0] + V.u[0] * a + V.v[0] * b, V.c[1] + V.u[1] * a + V.v[1] * b, V.c[2] + V.u[2] * a + V.v[2] * b);
      for (let k = -12; k <= 12; k++) {
        const major = k % 2 === 0;
        const m = new THREE.LineBasicMaterial({ color: major ? 0xb00000 : 0x303030, transparent: true, opacity: major ? 0.55 : 0.25, depthTest: false });
        const a = k * 0.5;
        grid.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints([pt(a, -6), pt(a, 6)]), m));
        grid.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints([pt(-6, a - (view === "top" ? 0 : 0.7)), pt(6, a - (view === "top" ? 0 : 0.7))]), m));
      }
      r.render(scene, oc);
      let url = r.domElement.toDataURL("image/png");
      const ref = opts.refs[view];
      if (ref) {
        const c = document.createElement("canvas");
        c.width = W;
        c.height = H;
        const g2 = c.getContext("2d");
        const load = (src) => new Promise((ok, err) => { const im = new Image(); im.onload = () => ok(im); im.onerror = err; im.src = src; });
        g2.drawImage(await load(url), 0, 0);
        g2.globalAlpha = opts.refAlpha;
        g2.drawImage(await load(ref), 0, 0, W, H);
        url = c.toDataURL("image/png");
      }
      shots["ortho_" + view] = url;
    }
    grid.clear();
    ground.visible = true;
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
    return route.fulfill({ contentType: "text/javascript", body: readFileSync(join(threeDir, url.pathname.slice(1))) });
  }
  return route.fulfill({ contentType: "text/html", body: PAGE });
});
await page.goto("http://shots.local/");
await page.waitForFunction(() => window.ready === true);
const v = (e) => e.split(",").map(Number);
const dataUrl = (file) => "data:image/png;base64," + readFileSync(file).toString("base64");
const refs = {};
for (const kv of (process.env.SHOT_REF ?? "").split(",").filter(Boolean)) {
  const [k, f] = kv.split("=");
  refs[k] = dataUrl(resolve(f));
}
const opts = {
  wire: process.env.SHOT_WIRE === "1",
  flat: process.env.SHOT_FLAT === "1",
  customCam: process.env.SHOT_CAM && process.env.SHOT_AT ? [v(process.env.SHOT_CAM), v(process.env.SHOT_AT)] : null,
  ortho: (process.env.SHOT_ORTHO ?? "").split(",").filter(Boolean),
  cams: (process.env.SHOT_CAMS ?? "").split(";").filter(Boolean).map((e) => {
    const [n, a, b] = e.split(":");
    return [n, v(a), v(b)];
  }),
  fov: Number(process.env.SHOT_FOV ?? 30),
  // SHOT_BG / SHOT_GROUND: background and ground colours (hex, e.g. 0x202326).
  bg: process.env.SHOT_BG ? Number(process.env.SHOT_BG) : null,
  ground: process.env.SHOT_GROUND ? Number(process.env.SHOT_GROUND) : null,
  orthoOnly: process.env.SHOT_ORTHO_ONLY === "1",
  pxPerM: Number(process.env.SHOT_PX_PER_M ?? 200),
  refs,
  refAlpha: Number(process.env.SHOT_REF_ALPHA ?? 0.45),
};
for (const id of cars) {
  // MESH_DIR=<dir> reads <dir>/<id>.json instead (e.g. `phase3_demo` output); an id
  // `<car>__<variant>` takes the car def of <car>.
  const meshDir = process.env.MESH_DIR ? resolve(process.env.MESH_DIR) : join(repo, "data/panels/meshes");
  const mesh = JSON.parse(readFileSync(join(meshDir, `${id}.json`), "utf8"));
  let car = null;
  try {
    car = JSON.parse(readFileSync(join(repo, "data/cars", `${id.split("__")[0]}.json`), "utf8"));
  } catch {}
  // The detail texture sits next to the meshes' parent (data/panels/<file>).
  let detail = null;
  if (mesh.detail && process.env.SHOT_NOTEX !== "1") {
    const f = join(repo, "data/panels", mesh.detail.file);
    if (existsSync(f)) detail = dataUrl(f);
  }
  const shots = await page.evaluate(([m, c, d, o]) => window.shoot(m, c, d, o), [mesh, car, detail, opts]);
  for (const [name, url] of Object.entries(shots)) {
    const file = join(out, `${id}_${name}.png`);
    writeFileSync(file, Buffer.from(url.split(",")[1], "base64"));
    console.log(file);
  }
}
await browser.close();
