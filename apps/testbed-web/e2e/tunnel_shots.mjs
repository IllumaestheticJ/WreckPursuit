// Hollins Tunnel (Loop 9) deck and backfill screenshots (headless Chromium): the north side,
// where the deck used to stand 7-9 m proud of the ground, and the hill side.
// Usage (after `npm run wasm`): node e2e/tunnel_shots.mjs [outDir] [prefix]  (default docs/media/tunnel)
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/tunnel"));
const prefix = process.argv[3] ?? "";
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
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

await page.evaluate(() => {
  document.getElementById("city-map").value = "ashpeak";
  document.getElementById("city").checked = true;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", true);
});
await page.waitForFunction(() => (window.__wp.scene.cities.get("ashpeak")?.children.length ?? 0) > 0, null, {
  timeout: 180000,
});
await page.addStyleTag({ content: "#overlay { display: none !important; }" });

// Portals at (-720, 1377) (east, road 41 m) and (-1110, 1399) (west, 57.5 m); deck top 7.3 m
// above the road. North = +y.
const views = [
  ["north_side.png", [-900, 1475, 50], [-900, 1395, 52]],
  ["north_edge_along.png", [-770, 1440, 58], [-960, 1412, 52]],
  ["east_portal.png", [-640, 1350, 60], [-760, 1385, 45]],
  ["hill_side.png", [-940, 1300, 85], [-900, 1395, 58]],
];
for (const [name, from, to] of views) {
  await page.evaluate(([f, t]) => {
    window.__wp.setClip(0.1, 4000);
    window.__wp.lookAt(f, t);
  }, [from, to]);
  await page.waitForTimeout(2500);
  await page.screenshot({ path: join(out, prefix + name) });
  console.log("wrote", prefix + name);
}
if (errors.length) console.log("page errors:", errors);
await browser.close();
process.exit(0);
