// End-to-end check of honk-to-start race spots (src/race_spots.ts, wp_wasm `race_spots()`) in
// headless Chromium. Usage: npm run e2e:race-spots (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome, SHOTS=dir for screenshots, CITY=vesper_v2 (default) | ashpeak.
//
// Checks: a free drive on the city lists a spot for every Port Vesper race and draws them;
// rolling into the Hollins sprint spot shows the "stop" prompt, stopping shows "Honk … to
// start"; honking outside / while moving does nothing; the horn key while stopped starts the
// Hollins sprint on the same map with the player's car; spots are hidden during the race.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5199;
const CITY = process.env.CITY ?? "vesper_v2";
const URL = `http://localhost:${PORT}/?city=${CITY}`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
const stopServer = () => server.kill("SIGTERM");
process.on("exit", stopServer);

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(URL);
      if (r.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("vite did not start");
}

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

await waitForServer();
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

// 1. Free drive on the city at night (the spots glow): every Port Vesper race has a spot.
await page.evaluate(() => window.__wp.startFreeDrive(undefined, undefined, true, 22));
await page.waitForFunction((c) => window.__wp.sim.cityId() === c, CITY, { timeout: 120000 });
const spots = await page.evaluate(() => window.__wp.sim.raceSpots());
const ids = spots.map((s) => s.id);
for (const id of ["hollins_sprint", "loop9_circuit", "lantern_checkpoint"]) check(ids.includes(id), `spot for ${id} on ${CITY}`);
check(!ids.includes("grid_highway_sprint"), "no spot for the grid city's race");
const drawn = await page.evaluate(() => window.__wp.raceSpots.group.children.length);
check(drawn === spots.length, `${drawn} spots drawn for ${spots.length} listed`);

// 2. Roll into the Hollins spot from 20 m short of it at 8 m/s; honk on the way (nothing).
const h = spots.find((s) => s.id === "hollins_sprint");
const player = await page.evaluate(() => window.__wp.app.player);
const back = 20;
await page.evaluate(
  ([p, x, y, yaw]) => window.__wp.sim.debugPlace(p, x, y, yaw, 8),
  [player, h.x + Math.sin(h.heading) * back, h.y - Math.cos(h.heading) * back, h.heading],
);
await page.keyboard.down("KeyQ");
await page.waitForTimeout(300);
await page.keyboard.up("KeyQ");
await page.waitForFunction(() => window.__wp.raceSpots.inside !== null, null, { timeout: 30000 });
const movingPrompt = await page.$eval("#race-spot-prompt", (e) => e.textContent);
check(/Stop here/.test(movingPrompt ?? ""), `prompt while rolling in: "${movingPrompt}"`);
check((await page.evaluate(() => window.__wp.sim.scenario().name)) === "free_drive", "honking outside / while moving started nothing");

// 3. Brake to a stop inside the spot: the honk prompt.
await page.keyboard.down("KeyS");
await page.waitForFunction(
  (p) => {
    const s = window.__wp.sim.states();
    for (let o = 0; o < s.length; o += 50) if (s[o] === p) return s[o + 14] < 0.3;
    return false;
  },
  player,
  { timeout: 30000 },
);
await page.keyboard.up("KeyS");
await page.waitForTimeout(400);
const prompt = await page.$eval("#race-spot-prompt", (e) => e.textContent);
check(/^Honk \(.+\) to start: Hollins Drive hill sprint — Sprint, /.test(prompt ?? ""), `stopped prompt: "${prompt}"`);
check((await page.evaluate(() => window.__wp.raceSpots.inside?.id)) === "hollins_sprint", "inside the Hollins spot");
await shot("race_spot_prompt.png");

// 4. Honk: the race starts on the same map with the player's car; spots are hidden.
const car = await page.evaluate(() => window.__wp.sim.scenario().entities[0].car);
await page.keyboard.down("KeyQ");
await page.waitForFunction(() => window.__wp.raceView.raceId === "hollins_sprint", null, { timeout: 120000 });
await page.keyboard.up("KeyQ");
const sc = await page.evaluate(() => window.__wp.sim.scenario());
check(sc.name === "race_hollins_sprint", `honk started the race session (${sc.name})`);
check((await page.evaluate(() => window.__wp.sim.cityId())) === CITY, `race runs on the same map (${CITY})`);
check(sc.entities[0].car === car, `with the player's car (${sc.entities[0].car})`);
await page.waitForTimeout(500);
check((await page.evaluate(() => window.__wp.sim.raceSpots().length)) === 0, "no spots listed during the race");
check(!(await page.evaluate(() => window.__wp.raceSpots.group.visible)), "spots hidden during the race");
check((await page.$eval("#race-spot-prompt", (e) => e.style.display)) === "none", "prompt hidden during the race");
await shot("race_spot_started.png");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
stopServer();
console.log(failures ? `${failures} check(s) failed` : "all race spot checks passed");
process.exit(failures ? 1 : 0);
