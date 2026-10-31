// Police tools in the browser testbed (headless Chromium, Playwright): the roadblock and spike
// strip goldens run live, their results match simcli, the barriers and the strip are drawn,
// and the spiked car's tyres are drawn flat. Usage: npm run e2e:police (needs `npm run wasm`).
// Env: CHROMIUM=/path/to/chrome, SHOTS=dir to write screenshots.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5197;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const untilTick = (t) => page.waitForFunction((t) => window.__wp.sim.worldInfo()[0] >= t, t, { timeout: 120000 });

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

// Roadblock (roadblocks-v2, owner 2026-10-31): two cruisers parked at an angle across the
// road, one wide gap; the car threads the gap untouched. Cones and flares are drawn; no
// barrier / barricade records any more.
await page.evaluate(() => window.__wp.runScenario("golden/roadblock_gap.ron", 0));
await page.evaluate(() => window.__wp.setCamera("chase"));
await untilTick(240 * 1.5);
const before = await page.evaluate(() => Array.from(window.__wp.sim.policeTools()));
const kinds = (recs) => { const k = []; for (let o = 0; o < recs.length; o += 11) k.push(recs[o]); return k; };
check(kinds(before).filter((k) => k === 4).length >= 4, `roadblock cones drawn (${kinds(before).filter((k) => k === 4).length})`);
check(kinds(before).filter((k) => k === 5).length >= 2, "road flares on the approach");
check(!kinds(before).some((k) => k === 0 || k === 2 || k === 3), "no retired barrier records");
await shot("police_roadblock_before.png");
await untilTick(240 * 3.4);
await shot("police_roadblock_through.png");
await page.waitForFunction(() => window.__wp.sim.scenarioResult()?.done === true, null, { timeout: 120000 });
let res = await page.evaluate(() => window.__wp.sim.scenarioResult());
check(res.pass && res.matches_simcli === true, `golden_roadblock_gap passes live and matches simcli (${res.matches_simcli})`);

// Spike strip: drawn on the road, then the tyres go flat.
await page.evaluate(() => window.__wp.runScenario("golden/spike_strip.ron", 0));
await page.evaluate(() => window.__wp.setCamera("chase"));
await untilTick(240 * 1.2);
const strip = await page.evaluate(() => Array.from(window.__wp.sim.policeTools()));
check(strip.length === 11 && strip[0] === 1, "spike strip drawn");
await shot("police_spikes_before.png");
await untilTick(240 * 7.5); // tyres deflate over 5 s after the hit at ~2 s
const flat = await page.evaluate(() => Array.from(window.__wp.sim.tirePunctures()).slice(0, 4));
check(flat.every((p) => p > 0.5), `all four tyres flat after the strip (${flat.map((p) => p.toFixed(2)).join(" ")})`);
await shot("police_spikes_flat.png");
await page.waitForFunction(() => window.__wp.sim.scenarioResult()?.done === true, null, { timeout: 120000 });
res = await page.evaluate(() => window.__wp.sim.scenarioResult());
check(res.pass && res.matches_simcli === true, `golden_spike_strip passes live and matches simcli (${res.matches_simcli})`);

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
process.exit(failures ? 1 : 0);
