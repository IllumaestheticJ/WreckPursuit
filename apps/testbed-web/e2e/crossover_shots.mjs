// Loop 9 median crossovers (openings in the median barrier, coned at both ends) screenshots
// (headless Chromium).
// Usage (after `npm run wasm`): node e2e/crossover_shots.mjs [outDir]  (default docs/media/crossover)
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/crossover"));
mkdirSync(out, { recursive: true });
const PORT = 5199;
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
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

async function city(id) {
  await page.evaluate((cid) => {
    document.getElementById("city-map").value = cid;
    document.getElementById("city").checked = true;
    window.__wp.startFreeDrive("debug_sedan", "Arcade", true);
  }, id);
  await page.waitForFunction((cid) => (window.__wp.scene.cities.get(cid)?.children.length ?? 0) > 0, id, {
    timeout: 90000,
  });
  await page.addStyleTag({ content: "#overlay { display: none !important; }" });
}

async function shoot(views) {
  for (const [name, from, to] of views) {
    await page.evaluate(([f, t]) => {
      window.__wp.setClip(0.1, 4000);
      window.__wp.lookAt(f, t);
    }, [from, to]);
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(out, name) });
    console.log("wrote", name);
  }
}

await city("vesper_v2");
// The four crossovers (wp_world::crossover; POIs "Loop 9 crossover 1..4"), each from above the
// carriageway a little way back.
await shoot([
  ["crossover_south.png", [105, -1080, 14], [150, -1060, 2.5]],
  ["crossover_east.png", [1505, 420, 31], [1524, 468, 18.8]],
  ["crossover_north.png", [930, 1352, 48], [882, 1374, 35.4]],
  ["crossover_west.png", [-1472, -448, 17], [-1491, -400, 4]],
  ["crossover_south_low.png", [128, -1063, 3.2], [175, -1060, 2.2]],
]);
if (errors.length) console.log("page errors:", errors);
await browser.close();
process.exit(0);
