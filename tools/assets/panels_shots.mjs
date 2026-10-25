// Browser check + screenshots for workstream R (car models & panels), headless Chromium.
//   node tools/assets/panels_shots.mjs [outDir=docs/media/panels]
// Needs the testbed's node_modules and wasm pkg (`just wasm`). Starts Vite itself.
// Checks: panel meshes load for every car, the side-swipe scenario swings a door open on its
// hinge, the frontal hit leaves the bumper hanging, the 56 km/h barrier and 50 km/h car hits
// crumple the hood and fenders, a 50 km/h rear-end crumples the quarters and trunk; saves
// screenshots of each.
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const testbed = join(repo, "apps/testbed-web");
const { chromium } = await import(pathToFileURL(join(testbed, "node_modules/playwright-core/index.mjs")).href);
const out = resolve(process.argv[2] ?? join(repo, "docs/media/panels"));
mkdirSync(out, { recursive: true });
const PORT = 5198;
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: testbed, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 150; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const panelsReady = () =>
  page.waitForFunction(() => [...window.__wp.app.views.values()].every((v) => v.panels.ready), null, { timeout: 20000 });
// Pause / resume the sim directly: P opens the game menu while driving (round 12), which keeps the
// sim paused after a second P and covers the screenshot.
const setPaused = (paused) => page.evaluate((p) => (window.__wp.app.paused = p), paused);
// Triangles the browser draws for the player car's model (batched body parts, lenses, wheel and
// wheel-pivot parts), against the mesh file. Round 12 batches body parts per material, so the
// number of scene children no longer says whether every part loaded.
const drawnTris = () =>
  page.evaluate(() => {
    const v = [...window.__wp.app.views.values()][0];
    const meshes = new Set();
    v.panels.root.traverse((o) => o.isMesh && meshes.add(o));
    for (const group of [...v.panels.wheelParts, ...v.panels.pivotParts]) for (const m of group) meshes.add(m);
    let tris = 0;
    for (const m of meshes) {
      const g = m.geometry;
      tris += (g.index ? g.index.count : (g.attributes.position?.count ?? 0)) / 3;
    }
    return Math.round(tris);
  });
const meshTris = (car) => JSON.parse(readFileSync(join(repo, "data/panels/meshes", `${car}.json`), "utf8")).tri_count;
const hideHud = () =>
  page.evaluate(() => {
    for (const id of ["telemetry", "inputs", "components", "events", "tireForces", "cgVelocity", "contacts"])
      window.__wp.toggleLayer(id);
  });

// 1. A model of several cars in free drive (Arcade, flat world), orbit camera.
await hideHud();
for (const car of ["debug_sedan", "hauler_1500", "lucciola_350", "sentinel_cruiser"]) {
  await page.evaluate((c) => window.__wp.startFreeDrive(c, "Arcade", false), car);
  await panelsReady();
  await page.evaluate(() => window.__wp.setCamera("orbit", [-5.5, 6.5, 2.6]));
  await sleep(1200);
  const [drawn, file] = [await drawnTris(), meshTris(car)];
  check(drawn >= file, `${car}: panel mesh loaded (${drawn} triangles drawn, ${file} in the mesh file)`);
  await page.screenshot({ path: join(out, `model_${car}.png`) });
}

// 2. Scenarios with panels: pause at a time, shoot, read panel states.
async function scenarioAt(file, tS, camera, offset) {
  await page.evaluate((f) => window.__wp.runScenario(f), file);
  await panelsReady();
  await page.evaluate(([c, o]) => window.__wp.setCamera(c, o), [camera, offset]);
  await page.waitForFunction((t) => window.__wp.sim.worldInfo()[0] >= t * 240, tS, { timeout: 90000 });
  await setPaused(true);
  await sleep(600);
  return page.evaluate(() =>
    [...window.__wp.app.views.values()].map((v) => Array.from(v.panels.states)),
  );
}
const panelsOf = (flat) => {
  const out = [];
  for (let k = 0; k + 10 <= flat.length; k += 10) out.push({ region: flat[k], state: flat[k + 1], angle: flat[k + 2] });
  return out;
};

let st = await scenarioAt("panels/side_swipe_door.ron", 3.6, "orbit", [-6, 3, 3]);
let victim = panelsOf(st[0]);
check(victim.some((p) => p.state === 1 && p.angle > 0.8), `side swipe: an unlatched door is swinging (${JSON.stringify(victim.filter((p) => p.state))})`);
await page.screenshot({ path: join(out, "side_swipe_door_open.png") });
await page.evaluate(() => window.__wp.toggleLayer("panels")); // lattice view
await sleep(500);
await page.screenshot({ path: join(out, "side_swipe_lattice_view.png") });
await page.evaluate(() => window.__wp.toggleLayer("panels"));
await setPaused(false);

st = await scenarioAt("panels/frontal_bumper_hang.ron", 4.25, "orbit", [4.5, 5.5, 1.6]);
const car = panelsOf(st[0]);
check(car.some((p) => p.state >= 2), `frontal: a panel hangs or fell (${JSON.stringify(car.filter((p) => p.state))})`);
await page.screenshot({ path: join(out, "frontal_bumper_hanging.png") });
await setPaused(false);

// 56 km/h full-width frontal (playtest row 31): the hood buckles, the headlights crush and break.
st = await scenarioAt("panels/frontal_56_hood.ron", 2.0, "orbit", [-4.2, -1.2, 3.4]);
const hood = await page.evaluate(() => {
  const v = [...window.__wp.app.views.values()][0];
  return { hood: v.panels.partMotion?.("hood") ?? null, lights: v.panels.lights?.().map((l) => ({ name: l.name, broken: l.broken })) ?? [] };
});
check(!!hood.hood && hood.hood.disp >= 0.1 && hood.hood.fold >= 0.08, `frontal 56 km/h: hood buckles (${JSON.stringify(hood.hood)})`);
const heads = hood.lights.filter((l) => l.name.startsWith("headlight"));
check(heads.length === 2 && heads.every((l) => l.broken), `frontal 56 km/h: both headlights broken (${JSON.stringify(hood.lights)})`);
await page.screenshot({ path: join(out, "frontal_56_hood_buckled.png") });
await setPaused(false);

// Lead feedback round 12b: a 50 km/h head-on into a parked car in Arcade. The bumper crushes
// and lets go while the frame barely moves; the hood still buckles (two ridges, ≥ 12 cm), its
// nose comes back with the crushed bumper and both front fenders buckle.
st = await scenarioAt("panels/frontal_50_car_arcade.ron", 2.2, "orbit", [-5.6, 2.2, 5.0]);
const front = await page.evaluate(() => {
  const v = [...window.__wp.app.views.values()][0];
  const m = (n) => v.panels.partMotion?.(n) ?? null;
  return { hood: m("hood"), fl: m("fender_fl"), fr: m("fender_fr") };
});
check(
  !!front.hood && front.hood.fold >= 0.12 && front.hood.pull >= 0.05 && front.fl?.fold >= 0.04 && front.fr?.fold >= 0.04,
  `frontal 50 km/h into a car: hood buckles and follows the crushed nose, fenders buckle (${JSON.stringify(front)})`,
);
await page.screenshot({ path: join(out, "frontal_50_car_hood_crumpled.png") });
await setPaused(false);

// Rear-ended at 50 km/h (parked sedan): both rear quarters buckle and follow the crushed tail.
st = await scenarioAt("panels/rear_50_car_arcade.ron", 2.2, "orbit", [-5.6, -2.2, 5.0]);
const rear = await page.evaluate(() => {
  const v = [...window.__wp.app.views.values()][0];
  const m = (n) => v.panels.partMotion?.(n) ?? null;
  return { trunk: m("trunk"), rl: m("quarter_rl"), rr: m("quarter_rr") };
});
check(
  rear.rl?.fold >= 0.05 && rear.rr?.fold >= 0.05 && rear.rl?.pull >= 0.05 && rear.rr?.pull >= 0.05 && rear.trunk?.fold >= 0.08,
  `rear-ended at 50 km/h: quarters buckle and follow the crushed tail, trunk folds (${JSON.stringify(rear)})`,
);
await page.screenshot({ path: join(out, "rear_50_car_quarters_crumpled.png") });
await setPaused(false);

// Brake lights: the taillight lenses use the shared tail material, glow red while braking, and
// the old lamp markers are gone with panels on.
await page.evaluate(() => window.__wp.startFreeDrive("debug_sedan", "Arcade", false));
await panelsReady();
await page.evaluate(() => window.__wp.setCamera("orbit", [2.6, -6.2, 2.0]));
await sleep(800);
await page.keyboard.down("KeyS");
await sleep(700);
const brake = await page.evaluate(() => {
  const v = [...window.__wp.app.views.values()][0];
  const lights = v.panels.lights?.() ?? [];
  const tails = lights.filter((l) => l.name.startsWith("taillight"));
  let markers = 0;
  v.root.traverse((o) => {
    if (o.isMesh && o.visible && !v.panels.root.getObjectById(o.id) && o.material?.emissive && o.geometry?.type === "BoxGeometry" && o.parent === v.root)
      markers += o.material === tails[0]?.material ? 1 : 0;
  });
  const e = tails[0]?.material?.emissive;
  return { tails: tails.map((l) => ({ name: l.name, visible: l.visible, broken: l.broken })), red: e ? [e.r, e.g, e.b] : null, markers };
});
await page.screenshot({ path: join(out, "brake_lights_on.png") });
await page.keyboard.up("KeyS");
check(
  brake.tails.length === 2 && brake.tails.every((l) => l.visible && !l.broken) && !!brake.red && brake.red[0] > 0.5 && brake.red[1] < 0.2 && brake.markers === 0,
  `brake lights: taillight lenses glow red, no lamp markers (${JSON.stringify(brake)})`,
);

// Browser cost of panel skinning (JS, per car per frame) and of the wasm panel_states call.
const perf = await page.evaluate(() => {
  const v = [...window.__wp.app.views.values()][0];
  const nodes = window.__wp.sim.latticeNodes(v.entity);
  const n = 300;
  let t0 = performance.now();
  for (let i = 0; i < n; i++) v.panels.update(nodes, 0);
  const skin = (performance.now() - t0) / n;
  return { skinMsPerCar: skin };
});
console.log(`browser: panel mesh skinning ${perf.skinMsPerCar.toFixed(3)} ms per car per frame (includes panel_states)`);

check(errors.length === 0, `no page errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `${failures} check(s) failed` : `all panel checks passed; screenshots in ${out}`);
process.exit(failures ? 1 : 0);
