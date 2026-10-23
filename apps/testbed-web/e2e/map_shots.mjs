// City map HUD screenshots for docs/media/map/ (workstream 16; headless Chromium via Playwright).
// Usage (after `npm run wasm`): node e2e/map_shots.mjs [outDir]   Env: CHROMIUM=/path/to/chrome.
// Shots: the minimap in a real Port Vesper pursuit (player given L2 heat next to a patrol car,
// then driving away), the full city map (whole city and zoomed), a staged SEARCHING frame (the
// real frame with the status, search zone and escape progress overridden: a real search needs
// minutes of evasion), and the old grid city.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/map"));
mkdirSync(out, { recursive: true });
const PORT = 5196;
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
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(`${URL}?chase=1`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

const minimapClip = () =>
  page.evaluate(() => {
    const r = document.getElementById("wp-minimap").getBoundingClientRect();
    return { x: r.x - 12, y: r.y - 12, width: r.width + 24, height: r.height + 24 };
  });
const save = async (name, clip) => {
  await page.screenshot({ path: join(out, name), ...(clip ? { clip } : {}) });
  console.log("wrote", name);
};
const status = () => page.evaluate(() => window.__wp.sim.mapDynamic(window.__wp.app.player)[3]);

// Police on the map, then a real pursuit: L2 heat in front of a patrol car, drive away.
await page.waitForFunction(() => window.__wp.sim.mapDynamic(window.__wp.app.player)[1] > 0, null, { timeout: 120000, polling: 200 });
await page.waitForFunction(
  () => {
    try {
      window.__wp.sim.debugParkNearPolice(window.__wp.app.player, 25);
      window.__wp.sim.debugSetHeat(window.__wp.app.player, 30);
      return true;
    } catch {
      return false;
    }
  },
  null,
  { timeout: 120000, polling: 100 },
);
// Parked in view: the patrol car recognises the wanted car and starts a pursuit (screenshot
// before the stop turns into a capture).
const pursued = await page
  .waitForFunction(() => window.__wp.sim.mapDynamic(window.__wp.app.player)[3] === 2, null, { timeout: 90000, polling: 100 })
  .then(() => true)
  .catch(() => false);
console.log("pursuit reached:", pursued, "status", await status());
await save("minimap_pursuit.png", await minimapClip());
await save("hud_pursuit.png");

// Full map: whole city, then zoomed in on the player.
await page.keyboard.press("KeyM");
await page.waitForTimeout(500);
await save("full_map.png");
for (let i = 0; i < 4; i++) await page.keyboard.press("Equal");
await page.keyboard.press("KeyF");
await page.waitForTimeout(500);
await save("full_map_zoomed.png");
await page.keyboard.press("KeyM");
// A few seconds later the real sim has usually lost sight and is searching (or has captured
// the parked car): whatever it is, that is the real state.
await page.waitForTimeout(3500);
console.log("later status", await status());
await save("minimap_later_real.png", await minimapClip());

// Staged SEARCHING frame (real units and player; status / zone / escape overridden).
await page.evaluate(() => {
  const sim = window.__wp.sim;
  const real = sim.mapDynamic;
  window.__realMapDynamic = real;
  sim.mapDynamic = (p) => {
    const a = real(p);
    a[3] = 3; // searching
    a[4] = Math.max(2, a[4]);
    a[5] = a[15] - 90; // last known spot behind-left of the player
    a[6] = a[16] - 60;
    a[7] = 170;
    a[8] = 0.45;
    a[12] = a[5];
    a[13] = a[6];
    a[14] = 1;
    return a;
  };
});
await page.waitForTimeout(600);
await save("minimap_searching_staged.png", await minimapClip());
await page.keyboard.press("Tab");
await page.waitForTimeout(400);
for (let i = 0; i < 3; i++) await page.keyboard.press("Equal");
await page.keyboard.press("KeyF");
await page.waitForTimeout(500);
await save("full_map_searching_staged.png");
await page.keyboard.press("Escape");
await page.evaluate(() => (window.__wp.sim.mapDynamic = window.__realMapDynamic));

// The old grid city.
await page.evaluate(() => {
  const sel = document.getElementById("city-map");
  sel.value = "procedural_12x12";
  sel.dispatchEvent(new Event("change"));
});
await page.waitForFunction(() => window.__wp.map.hud.stats().city === "procedural_12x12", null, { timeout: 60000 });
await page.keyboard.down("KeyW");
await page.waitForTimeout(3000);
await page.keyboard.up("KeyW");
await save("grid_minimap.png", await minimapClip());
await page.keyboard.press("KeyM");
await page.waitForTimeout(500);
await save("grid_full_map.png");

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 3));
  process.exit(1);
}
process.exit(0);
