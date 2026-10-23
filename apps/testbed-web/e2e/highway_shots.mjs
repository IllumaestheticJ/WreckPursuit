// Loop 9 interchanges, median barrier, road markings and ramp gores screenshots (headless
// Chromium).
// Usage (after `npm run wasm`): node e2e/highway_shots.mjs [outDir]  (default docs/media/highway)
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/highway"));
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
  executablePath: chromiumPath(),
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
console.log("marking triangles", await page.evaluate(() => window.__wp.scene.markings.triangles));
await shoot([
  ["interchange_harbor_west.png", [-1250, -800, 160], [-1490, -480, 5]],
  ["interchange_bluff_road.png", [1250, 650, 140], [1510, 300, 10]],
  ["off_ramp_gore.png", [-1513.5, -95, 9.5], [-1506, -280, 4]],
  ["on_ramp_merge.png", [1524, -270, 4.5], [1531, -100, 2]],
  ["off_ramp_chevrons.png", [1535.5, 82, 9.5], [1540, 108, 5.5]],
  ["on_ramp_chevrons.png", [1526.5, -283, 5.5], [1531, -252, 1.5]],
  ["crossroad_bridge.png", [-1380, -470, 9], [-1600, -485, 8]],
  ["median_barrier.png", [36, 1373.5, 25.2], [200, 1386.5, 23.6]],
  ["port_half_diamond.png", [700, -880, 90], [920, -1040, 2]],
  ["downtown_markings.png", [-160, -45, 14], [120, -48, 8]],
]);
await city("procedural_12x12");
await shoot([
  ["grid_markings.png", [-40, -40, 30], [60, 60, 0]],
  ["grid_interchange.png", [160, -620, 95], [0, -385, 0]],
  ["grid_viaduct.png", [48, -455, 10], [2, -320, 7]],
  ["grid_on_viaduct.png", [9.6, -300, 9.5], [9.6, 0, 8.6]],
  ["grid_taper_dotted.png", [12.5, -700, 5], [15, -610, 0]],
  ["grid_gore_chevrons.png", [11.5, -572, 7.5], [14.5, -530, 3]],
]);
if (errors.length) console.log("page errors:", errors);
await browser.close();
process.exit(0);
