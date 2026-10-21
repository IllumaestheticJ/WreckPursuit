// Instanced city features check (features_view.ts): free drive in a city (default vesper_v3),
// then, if the city has no box / cylinder features yet, inject a synthetic set around the car
// (containers, a wall, trees, lamps, parked cars, a parking deck with a pitched ramp) and take
// screenshots. Prints feature / chunk counts and page errors.
// Usage (after `npm run wasm`): node e2e/features_shots.mjs [outDir] [cityId]
// Env: CHROMIUM=/path/to/chrome. A missing greybox for the city is tolerated (warning only).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? "/tmp/wp-features-shots");
const cityId = process.argv[3] ?? "vesper_v3";
mkdirSync(out, { recursive: true });
const PORT = 5198;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1600, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => {
  if (m.type() === "warning" || m.type() === "error") console.log(`[${m.type()}]`, m.text().slice(0, 200));
});
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

const t0 = Date.now();
await page.evaluate((id) => {
  document.getElementById("city-map").value = id;
  document.getElementById("city").checked = true;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", true);
}, cityId);
await page.waitForFunction((id) => window.__wp.sim.cityId() === id, cityId, { timeout: 300000 });
console.log(`${cityId} session started in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
await page.waitForTimeout(3000);
await page.addStyleTag({ content: "#overlay { display: none !important; }" });

const info = await page.evaluate(() => {
  const w = window.__wp;
  const f = w.scene.features;
  const st = w.sim.states();
  const pos = [st[1], st[2], st[3]];
  const native = { boxes: f.boxes, cylinders: f.cylinders, meshes: f.meshes };
  let synthetic = false;
  if (f.boxes + f.cylinders === 0) {
    synthetic = true;
    // [shape, material, kind, cx, cy, cz, hx, hy, hz, yaw, pitch]
    const rows = [];
    const [x, y, z] = pos;
    const g = z - 0.5; // rough ground under the car
    // Containers (red / blue / green), stacked two high, 30 m east.
    for (let i = 0; i < 6; i++)
      for (let k = 0; k < 2; k++) rows.push([0, 17 + ((i + k) % 3), 28, x + 30 + i * 2.8, y + 10, g + 1.3 + k * 2.6, 1.22, 6.1, 1.3, 0, 0]);
    // A concrete wall 20 m north, running east-west.
    rows.push([0, 0, 0, x, y + 20, g + 1.25, 0.3, 15, 1.25, Math.PI / 2, 0]);
    // Trees west: trunk (wood cylinder) + canopy (foliage cylinder), and lamps (light).
    for (let i = 0; i < 8; i++) {
      rows.push([1, 5, 48, x - 12, y - 20 + i * 8, g + 1.5, 0.25, 0.25, 1.5, 0, 0]);
      rows.push([1, 9, 49, x - 12, y - 20 + i * 8, g + 5, 2.2, 2.2, 2, 0, 0]);
      rows.push([1, 23, 51, x - 6, y - 20 + i * 8, g + 4, 0.15, 0.15, 4, 0, 0]);
    }
    // Parked cars (car paint, per-instance colour) in a row east.
    for (let i = 0; i < 10; i++) rows.push([0, 22, 35, x + 10 + i * 2.6, y - 10, g + 0.7, 0.9, 2.2, 0.7, 0, 0]);
    // A parking deck 6 m up with a 1:7 ramp (pitched box) climbing north onto it.
    rows.push([0, 0, 32, x + 60, y + 60, g + 6 - 0.15, 8, 15, 0.15, 0, 0]);
    const pitch = Math.atan2(6, 42);
    const len = Math.hypot(6, 42);
    rows.push([0, 1, 33, x + 60, y + 24 - 0.15 * Math.sin(pitch), g + 3 - 0.15 * Math.cos(pitch), 3, len / 2, 0.15, 0, pitch]);
    // Orange safety barrier arms, yawed 30°.
    for (let i = 0; i < 4; i++) rows.push([0, 15, 7, x + 20 + i * 4, y + 30, g + 0.5, 0.1, 1.8, 0.5, Math.PI / 6, 0]);
    f.set(new Float32Array(rows.flat()), w.sim.featurePalette());
  }
  return { pos, native, synthetic, boxes: f.boxes, cylinders: f.cylinders, meshes: f.meshes, chunks: f.chunkStats() };
});
console.log(JSON.stringify(info));
const [x, y, z] = info.pos;
const views = [
  ["features_overview.png", [x - 40, y - 60, z + 45], [x + 25, y + 20, z]],
  ["features_street.png", [x - 2, y - 30, z + 3], [x + 15, y + 10, z + 2]],
  ["features_deck_ramp.png", [x + 30, y + 5, z + 15], [x + 60, y + 45, z + 4]],
];
for (const [name, from, to] of views) {
  await page.evaluate(([f, t]) => {
    window.__wp.setClip(0.1, 3000);
    window.__wp.lookAt(f, t);
  }, [from, to]);
  await page.waitForTimeout(1500);
  await page.screenshot({ path: join(out, name) });
  console.log("wrote", join(out, name));
}
// Draw distance: from 3 km up and away, nearby chunks only.
const far = await page.evaluate(() => window.__wp.scene.features.chunkStats());
console.log("chunks visible / total:", JSON.stringify(far));
console.log(errors.length ? `page errors:\n${errors.join("\n")}` : "no page errors");
await browser.close();
server.kill("SIGTERM");
process.exit(errors.length ? 1 : 0);
