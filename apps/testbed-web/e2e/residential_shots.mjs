// Residential / arterial street shots for docs/art/city/ (lead helper `lead/city-residential`,
// owner playtest rows 94–95), low-poly dark style, scripted camera.
// Usage (after `npm run wasm`): node e2e/residential_shots.mjs <city> [outDir] [prefix]
//   city = vesper_v2 | procedural_12x12, prefix = before | after (file name prefix).
// Env: CHROMIUM=/path/to/chrome, HOUR (default 15).
// Each view is a lane point and heading: street level = 14 m behind, 4.5 m up, looking 40 m
// ahead; `top` views look down from 160 m behind / 130 m up.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const [city = "vesper_v2", outArg, prefix = "after"] = process.argv.slice(2);
const out = resolve(outArg ?? join(root, "../../docs/art/city"));
mkdirSync(out, { recursive: true });
const HOUR = Number(process.env.HOUR ?? 15);

// [name, lane point (x, y, z), heading (x, y), kind]
const VIEWS = {
  vesper_v2: [
    ["vesper_mercer_local", [211.1, 798.6, 22.3], [-0.03, -1], "street"],
    ["vesper_mercer_top", [60, 640, 18], [0.6, 0.8], "top"],
    ["vesper_eastgate_local", [1090.7, -142.3, 3.3], [0.03, -1], "street"],
    ["vesper_larkspur_local", [-1236.1, -202.5, 7.0], [-0.08, 1], "street"],
    ["vesper_larkspur_top", [-1000, -120, 8], [0.6, 0.8], "top"],
    ["vesper_harbor_arterial", [-100.1, -572.5, 2.7], [1, -0.07], "street"],
    ["vesper_downtown_arterial", [-247.1, -251.8, 4.6], [1, 0], "street"],
    ["vesper_loop9_highway", [-459.5, -1072.6, 2.5], [1, -0.05], "street"],
  ],
  procedural_12x12: [
    ["grid_residential_local", [-273.2, 297.8, 0], [0, 1], "street"],
    ["grid_residential_top", [-440, 440, 0], [0.6, 0.8], "top"],
    ["grid_arterial", [-64.0, -383.2, 0], [-1, 0], "street"],
  ],
};

const PORT = 5199;
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
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
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await page.evaluate(([c, h]) => {
  window.__wp.setStyle("lowpoly");
  document.getElementById("city-map").value = c;
  document.getElementById("city").checked = true;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", true, h);
}, [city, HOUR]);
await page.waitForFunction((c) => (window.__wp.scene.cities.get(c)?.children.length ?? 0) > 0, city, { timeout: 120000 });
await page.addStyleTag({ content: "#overlay, #hud, .hud, #minimap { display: none !important; }" });
await page.waitForTimeout(3000);

for (const [name, p, h, kind] of VIEWS[city] ?? []) {
  const n = Math.hypot(h[0], h[1]);
  const d = [h[0] / n, h[1] / n];
  const [from, to] =
    kind === "top"
      ? [[p[0] - d[0] * 160, p[1] - d[1] * 160, p[2] + 130], [p[0], p[1], p[2]]]
      : [[p[0] - d[0] * 14, p[1] - d[1] * 14, p[2] + 4.5], [p[0] + d[0] * 40, p[1] + d[1] * 40, p[2] + 1]];
  await page.evaluate(([f, t]) => {
    window.__wp.setClip(0.1, 3000);
    window.__wp.lookAt(f, t);
  }, [from, to]);
  await page.waitForTimeout(2500);
  await page.screenshot({ path: join(out, `${prefix}_${name}.png`) });
  console.log("wrote", `${prefix}_${name}.png`);
}

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 3));
  process.exit(1);
}
process.exit(0);
