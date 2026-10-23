// Port Vesper v2 screenshots for docs/media/city-v2/ (headless Chromium via Playwright).
// Usage (after `npm run wasm`): node e2e/city_v2_shots.mjs [outDir]
// Env: CHROMIUM=/path/to/chrome. Shots: overview, Hollins Drive switchbacks, the Route 40 West
// overpass and Route 40 North underpass, a banked Loop sweeper, Bluff Road, downtown at street
// level, and the night run from the chase camera.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/city-v2"));
mkdirSync(out, { recursive: true });
const PORT = 5197;
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

// Free drive in Port Vesper at noon; wait for the greybox.
await page.evaluate(() => {
  document.getElementById("city-map").value = "vesper_v2";
  document.getElementById("city").checked = true;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", true);
});
await page.waitForFunction(() => (window.__wp.scene.cities.get("vesper_v2")?.children.length ?? 0) > 0, null, {
  timeout: 60000,
});
// Hide the panels so the city fills the frame.
await page.addStyleTag({ content: "#overlay { display: none !important; }" });

const views = [
  ["overview.png", [-1500, -2100, 1400], [-100, 300, 0]],
  ["hollins_switchbacks.png", [-560, 820, 140], [-780, 1060, 70]],
  ["route40w_overpass.png", [-1430, 30, 26], [-1520, 100, 10]],
  ["route40n_underpass.png", [30, 1250, 26], [0, 1380, 4]],
  ["loop_banked_sweeper.png", [1300, 1120, 70], [1500, 1250, 30]],
  ["bluff_road.png", [700, 760, 75], [1150, 1000, 40]],
  ["downtown_street.png", [-160, -45, 14], [120, -48, 8]],
  ["hill_from_downtown.png", [-250, 300, 40], [-1000, 1080, 90]],
];
for (const [name, from, to] of views) {
  const [near, far] = name === "overview.png" ? [20, 9000] : [0.1, 3000];
  await page.evaluate(([f, t, n, fa]) => {
    window.__wp.setClip(n, fa);
    window.__wp.lookAt(f, t);
  }, [from, to, near, far]);
  await page.waitForTimeout(1500);
  await page.screenshot({ path: join(out, name) });
  console.log("wrote", name);
}

// Night run: night, police, traffic, from the chase camera.
await page.evaluate(() => {
  window.__wp.setClip(0.1, 3000);
  document.getElementById("city").checked = false;
  window.__wp.startNightRun("debug_sedan", "Arcade");
});
await page.keyboard.down("KeyW");
await page.waitForTimeout(4000);
await page.keyboard.up("KeyW");
await page.screenshot({ path: join(out, "night_run.png") });
console.log("wrote night_run.png");

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 3));
  process.exit(1);
}
process.exit(0);
