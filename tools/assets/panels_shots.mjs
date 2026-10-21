// Browser check + screenshots for workstream R (car models & panels), headless Chromium.
//   node tools/assets/panels_shots.mjs [outDir=docs/media/panels]
// Needs the testbed's node_modules and wasm pkg (`just wasm`). Starts Vite itself.
// Checks: panel meshes load for every car, the side-swipe scenario swings a door open on its
// hinge, the frontal hit leaves the bumper hanging; saves screenshots of each.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
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
  const parts = await page.evaluate(() => [...window.__wp.app.views.values()][0].panels.root.children.length);
  check(parts >= 25, `${car}: panel mesh loaded (${parts} parts)`);
  await page.screenshot({ path: join(out, `model_${car}.png`) });
}

// 2. Scenarios with panels: pause at a time, shoot, read panel states.
async function scenarioAt(file, tS, camera, offset) {
  await page.evaluate((f) => window.__wp.runScenario(f), file);
  await panelsReady();
  await page.evaluate(([c, o]) => window.__wp.setCamera(c, o), [camera, offset]);
  await page.waitForFunction((t) => window.__wp.sim.worldInfo()[0] >= t * 240, tS, { timeout: 90000 });
  await page.keyboard.press("KeyP"); // pause
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
await page.keyboard.press("Digit0"); // lattice view
await sleep(500);
await page.screenshot({ path: join(out, "side_swipe_lattice_view.png") });
await page.keyboard.press("Digit0");
await page.keyboard.press("KeyP");

st = await scenarioAt("panels/frontal_bumper_hang.ron", 4.25, "orbit", [4.5, 5.5, 1.6]);
const car = panelsOf(st[0]);
check(car.some((p) => p.state >= 2), `frontal: a panel hangs or fell (${JSON.stringify(car.filter((p) => p.state))})`);
await page.screenshot({ path: join(out, "frontal_bumper_hanging.png") });
await page.keyboard.press("KeyP");

// Browser cost of panel skinning (JS, per car per frame) and of the wasm panel_states call.
const perf = await page.evaluate(() => {
  const v = [...window.__wp.app.views.values()][0];
  const nodes = window.__wp.sim.latticeNodes(v.entity);
  const n = 300;
  let t0 = performance.now();
  for (let i = 0; i < n; i++) v.panels.update(nodes, 0);
  const skin = (performance.now() - t0) / n;
  t0 = performance.now();
  for (let i = 0; i < n; i++) v.panels.update(nodes, 0, undefined, false);
  const steady = (performance.now() - t0) / n;
  return { skinMsPerCar: skin, steadyMsPerCar: steady };
});
console.log(
  `browser: panel mesh skinning ${perf.skinMsPerCar.toFixed(3)} ms per car per frame while denting (re-skin + normals), ` +
    `${perf.steadyMsPerCar.toFixed(3)} ms steady (only swinging panels; includes panel_states)`,
);

check(errors.length === 0, `no page errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `${failures} check(s) failed` : `all panel checks passed; screenshots in ${out}`);
process.exit(failures ? 1 : 0);
