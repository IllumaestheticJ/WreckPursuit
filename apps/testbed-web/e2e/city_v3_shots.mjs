// Port Vesper v3 (in development) screenshots for docs/media/city-v3/ (headless Chromium via
// Playwright). Usage (after `npm run wasm`): node e2e/city_v3_shots.mjs [outDir]
// Env: CHROMIUM=/path/to/chrome. Shots: overview, downtown, the Westline Cloverleaf, the Skyway
// Stack, the container terminal, the airport, the rail yard, Hollins Knob, the Unfinished
// Exchange, the Calloway Tunnel portal, the Old Harbor piers and the Vesper Bowl.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/city-v3"));
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

// Free drive in Port Vesper at noon; wait for the greybox.
await page.evaluate(() => {
  document.getElementById("city-map").value = "vesper_v3";
  document.getElementById("city").checked = true;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", true);
});
await page.waitForFunction(() => (window.__wp.scene.cities.get("vesper_v3")?.children.length ?? 0) > 0, null, {
  timeout: 180000,
});
// Hide the panels so the city fills the frame.
await page.addStyleTag({ content: "#overlay { display: none !important; }" });

const views = [
  ["overview.png", [-3600, -5200, 3200], [0, 0, 0]],
  ["downtown.png", [700, -700, 140], [450, 250, 40]],
  ["westline_cloverleaf.png", [-1650, -150, 320], [-2000, 300, 10]],
  ["skyway_stack.png", [2250, 150, 220], [1800, 500, 15]],
  ["container_terminal.png", [1250, -1950, 160], [2400, -2750, 0]],
  ["airport.png", [-2900, -1000, 420], [-3900, -2200, 0]],
  ["rail_yard_roundhouse.png", [-2100, -350, 180], [-2500, 250, 0]],
  ["hollins_knob.png", [-1400, 1900, 320], [-2300, 2580, 120]],
  ["unfinished_exchange.png", [-300, 2480, 45], [-675, 2600, 10]],
  ["calloway_tunnel_portal.png", [1830, 160, 22], [1800, -150, 2]],
  ["old_harbor_piers.png", [1950, 250, 80], [2450, 450, 0]],
  ["vesper_bowl.png", [-650, -1620, 110], [-950, -1860, 0]],
];
for (const [name, from, to] of views) {
  const [near, far] = name === "overview.png" ? [20, 14000] : [0.5, 4000];
  await page.evaluate(([f, t, n, fa]) => {
    window.__wp.setClip(n, fa);
    window.__wp.lookAt(f, t);
  }, [from, to, near, far]);
  await page.waitForTimeout(2500);
  await page.screenshot({ path: join(out, name) });
  console.log("wrote", name);
}

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 3));
  process.exit(1);
}
process.exit(0);
