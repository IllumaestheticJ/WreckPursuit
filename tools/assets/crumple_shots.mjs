// Hood / front crumple shots for workstream R (lead feedback round 12b), headless Chromium.
//   node tools/assets/crumple_shots.mjs [prefix=after] [outDir=docs/media/panels]
// writes crumple_<prefix>_<scenario>_<8|30>m.png. The "before" set was shot from a worktree
// of the commit before round 12b (old renderer + format-2 meshes) with prefix "before".
// Runs the 50 km/h Arcade frontals (barrier, parked car) and a 70 km/h barrier hit, pauses
// after the hit and shoots each from the front-left, high (over the barrier, onto the hood) at
// 8 m and 30 m, cropped to the car. Prints the
// hood / fender / bumper motion the renderer reports (`PanelBody.partMotion`) as JSON.
// Needs the testbed's node_modules and wasm pkg (`just wasm`). Starts Vite itself.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const testbed = join(repo, "apps/testbed-web");
const { chromium } = await import(pathToFileURL(join(testbed, "node_modules/playwright-core/index.mjs")).href);
const prefix = process.argv[2] ?? "after";
const out = resolve(process.argv[3] ?? join(repo, "docs/media/panels"));
mkdirSync(out, { recursive: true });
const PORT = Number(process.env.PORT ?? 5199);
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: testbed, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 150; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}
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
// Hide the HUD panels and debug overlays so the shots show the car (this browser context only).
await page.addStyleTag({ content: "#overlay, #audio-panel, #police-hud { display: none !important; }" });
await page.evaluate(() => {
  for (const id of ["telemetry", "inputs", "components", "events", "tireForces", "cgVelocity", "contacts"]) window.__wp.toggleLayer(id);
});

// Left of the car, a little ahead and high (so the hood shows over the fender and the barrier
// does not hide it), at `d` metres.
const viewAt = (d) => {
  const v = [-0.72, 0.28, 0.64];
  const l = Math.hypot(...v);
  return v.map((x) => (x / l) * d);
};
const results = {};
for (const [name, file, tS, speed] of [
  ["frontal_50_barrier", "panels/frontal_50_barrier_arcade.ron", 2.2, 0],
  ["frontal_50_car", "panels/frontal_50_car_arcade.ron", 2.2, 0],
  ["frontal_70_barrier", "panels/frontal_50_barrier_arcade.ron", 2.0, 70],
]) {
  await page.evaluate(([f, s]) => window.__wp.runScenario(f, s), [file, speed]);
  await page.waitForFunction(() => [...window.__wp.app.views.values()].every((v) => v.panels.ready), null, { timeout: 20000 });
  await page.evaluate(() => window.__wp.setCamera("orbit", [-2, -7, 3]));
  await page.waitForFunction((t) => window.__wp.sim.worldInfo()[0] >= t * 240, tS, { timeout: 90000 });
  await page.keyboard.press("KeyP"); // pause
  await sleep(500);
  results[name] = await page.evaluate(() => {
    const v = [...window.__wp.app.views.values()][0];
    const m = (n) => v.panels.partMotion?.(n) ?? null;
    return { hood: m("hood"), fender_fl: m("fender_fl"), fender_fr: m("fender_fr"), bumper_front: m("bumper_front"), states: Array.from(v.panels.states) };
  });
  for (const d of [8, 30]) {
    await page.evaluate((o) => window.__wp.setCamera("orbit", o), viewAt(d));
    await sleep(600);
    // Crop to the middle of the frame (the car), away from the HUD panels.
    await page.screenshot({ path: join(out, `crumple_${prefix}_${name}_${d}m.png`), clip: { x: 340, y: 150, width: 600, height: 420 } });
  }
  await page.keyboard.press("KeyP");
}
console.log(JSON.stringify(results, (k, v) => (k === "states" ? undefined : typeof v === "number" ? Math.round(v * 1000) / 1000 : v)));
if (errors.length) console.log(`page errors: ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
process.exit(errors.length ? 1 : 0);
