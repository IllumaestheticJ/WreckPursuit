// Browser check for the mixed fleets (data/fleets/*.toml): city chase in Port Vesper with the
// player made wanted, then counts which police models were dispatched and which traffic models
// are drawn as real cars, and saves screenshots.
//   node e2e/fleets_shots.mjs [outDir]   (from apps/testbed-web; needs the wasm pkg)
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const testbed = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { chromium } = await import(pathToFileURL(join(testbed, "node_modules/playwright-core/index.mjs")).href);
const out = resolve(process.argv[2] ?? join(tmpdir(), "wp-fleets"));
mkdirSync(out, { recursive: true });
const PORT = 5199;
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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await page.evaluate(() => {
  for (const id of ["telemetry", "inputs", "components", "events", "tireForces", "cgVelocity", "contacts"]) window.__wp.toggleLayer(id);
});
await page.evaluate(() => window.__wp.startCityChase());
await page.waitForFunction(() => window.__wp.scene.traffic.active, null, { timeout: 30000 });
// Wanted level 4 for the player: reinforcements and roadblocks draw from the heavy end.
await page.evaluate(() => window.__wp.sim.debugSetHeat(window.__wp.app.player, 80));

const seenPolice = new Set();
const seenTraffic = new Set();
for (let i = 0; i < 45; i++) {
  await sleep(1000);
  const s = await page.evaluate(() => {
    const wp = window.__wp;
    const police = [...wp.app.views.values()].filter((v) => v.info.kind === "Police").map((v) => v.info.car_id);
    const cars = wp.sim.trafficFleetCars();
    const flags = wp.sim.trafficAgents().filter((_, k) => k % 5 === 4);
    const drawn = [...new Set([...flags].map((f) => cars[Math.floor(f / 65536)]))];
    return { police, drawn };
  });
  s.police.forEach((c) => seenPolice.add(c));
  s.drawn.forEach((c) => seenTraffic.add(c));
}
console.log("police models:", [...seenPolice].sort().join(", "));
console.log("traffic models:", [...seenTraffic].sort().join(", "));
check(seenPolice.size >= 2, `several police models dispatched (${seenPolice.size})`);
check(seenTraffic.size >= 8, `many traffic models on the road (${seenTraffic.size})`);
check(
  [...seenTraffic].some((c) => ["gantry_box", "longhaul_tractor", "quarry_tipper"].includes(c)),
  "heavy trucks in traffic",
);
const models = await page.evaluate(() => window.__wp.scene.traffic.group.children.length);
check(models >= 9, `traffic meshes built (${models} instanced meshes)`);

await page.evaluate(() => window.__wp.setCamera("orbit", [-14, 18, 9]));
await sleep(1500);
await page.screenshot({ path: join(out, "fleets_chase_orbit.png") });
await page.evaluate(() => {
  const v = window.__wp.app.views.get(window.__wp.app.player);
  const p = v.root.position;
  window.__wp.lookFrom([p.x - 60, p.y - 60, 45], [p.x, p.y, 0]);
});
await sleep(1500);
await page.screenshot({ path: join(out, "fleets_chase_overview.png") });
check(errors.length === 0, `no page errors ${errors.join(" | ")}`);
await browser.close();
console.log(failures ? `${failures} failure(s)` : `all fleet checks passed; screenshots in ${out}`);
process.exit(failures ? 1 : 0);
