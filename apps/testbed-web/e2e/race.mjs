// End-to-end check of street races in the browser testbed (workstream 15) in headless Chromium.
// Usage: npm run e2e:race (needs `npm run wasm` first; `npm run e2e` runs it after run.mjs).
// Env: CHROMIUM=/path/to/chrome, SHOTS=dir for screenshots.
//
// Checks: the "races" button opens the picker with every race; picking the grid highway
// sprint starts a race session (countdown, HUD, player held on the grid, gate marker in the
// world); after the green light the AI racers move and show on the city minimap; on autopilot the race runs to a results
// table with a payout from the economy; no page errors.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5198;
const URL = `http://localhost:${PORT}/`;
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
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1400, height: 860 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

// 1. The race menu lists every race.
await page.click("#sc-races");
await page.waitForSelector('[data-testid="race-picker"]', { timeout: 10000 });
const cards = await page.$$eval("[data-race]", (els) => els.map((e) => e.getAttribute("data-race")));
check(cards.length >= 4, `race picker lists ${cards.length} races (${cards.join(", ")})`);
for (const id of ["hollins_sprint", "loop9_circuit", "lantern_checkpoint", "grid_highway_sprint"]) check(cards.includes(id), `picker offers ${id}`);
for (const id of ["harbor_dash", "tunnel_run", "hollins_descent", "offramp_sprint", "grid_arterial_sprint"]) check(cards.includes(id), `picker offers ${id}`);
// Betting: every card prices the player (odds on yourself); a fresh account below Street rep
// sees them with the lock reason, an unlocked one gets the bet slip.
const betRows = await page.$$eval('[data-testid="bet-locked"], [data-testid="bet-slip"]', (els) => els.map((e) => e.textContent ?? ""));
check(betRows.length === cards.length && betRows.every((t) => /\d\.\d\d/.test(t)), `every race card shows odds on the player (${betRows[0] ?? "none"})`);
await shot("race_picker.png");

// 2. Pick the grid highway sprint from the menu: a race session with countdown and HUD.
await page.click('[data-race="grid_highway_sprint"] button');
await page.waitForFunction(() => window.__wp.raceView.raceId === "grid_highway_sprint", null, { timeout: 30000 });
const scName = await page.evaluate(() => window.__wp.sim.scenario().name);
check(scName === "race_grid_highway_sprint", `menu started the race session (${scName})`);
check((await page.$('[data-testid="race-picker"]')) === null, "picker closed");
await page.waitForSelector('[data-testid="race-hud"]', { timeout: 10000 });
check((await page.$('[data-testid="race-countdown"]')) !== null, "countdown shows on the grid");
const entities = await page.evaluate(() => window.__wp.sim.entities());
check(entities.length === 4, `player + 3 AI racers (${entities.length} entities)`);
const posOf = (id) =>
  page.evaluate((e) => {
    const s = window.__wp.sim.states();
    for (let o = 0; o < s.length; o += 50) if (s[o] === e) return [s[o + 1], s[o + 2]];
    return null;
  }, id);
const gridPos = {};
for (const e of entities) gridPos[e] = await posOf(e);
// Full throttle during the countdown is refused: the player stays on the grid.
await page.keyboard.down("KeyW");
await page.waitForTimeout(1200);
const info0 = await page.evaluate(() => Array.from(window.__wp.sim.raceInfo()));
const p0 = await posOf(entities[0]);
const moved = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);
if (info0[0] === 0) check(moved(p0, gridPos[entities[0]]) < 0.5, `player held on the grid during the countdown (${moved(p0, gridPos[entities[0]]).toFixed(2)} m)`);
check((await page.evaluate(() => window.__wp.raceMarker.shown)) >= 0, "next-gate marker is placed in the world");
await shot("race_grid.png");

// 3. After green the AI racers drive off.
await page.waitForFunction(() => window.__wp.sim.raceInfo()[0] >= 1, null, { timeout: 60000 });
const t0 = await page.evaluate(() => window.__wp.sim.worldInfo()[0]);
await page.waitForFunction((t) => window.__wp.sim.worldInfo()[0] > t + 240 * 5, t0, { timeout: 90000 });
await page.keyboard.up("KeyW");
let aiMoved = 0;
for (const e of entities.slice(1)) if (moved(await posOf(e), gridPos[e]) > 15) aiMoved++;
check(aiMoved >= 2, `AI racers move after the green light (${aiMoved}/3 moved > 15 m)`);
const st = await page.evaluate(() => Array.from(window.__wp.sim.raceStandings()));
check(st.length === 4 * 10, `live standings for 4 racers (${st.length / 10})`);
check((await page.$('[data-testid="race-minimap"]')) !== null, "race minimap (UI kit) shows the route");
// The city minimap draws the AI racers (owner row 100): map_dynamic v2 racer block.
const mapRacers = await page.evaluate(() => ({ n: window.__wp.sim.mapDynamic(window.__wp.app.player)[20], drawn: window.__wp.map.hud.stats().minimap.racers }));
check(mapRacers.n === 4 && mapRacers.drawn === 3, `city minimap shows the 3 AI racers (map racers ${mapRacers.n}, drawn ${mapRacers.drawn})`);
await shot("race_hud.png");

// 4. A fresh page races the sprint on autopilot (the AI drives the player too), fast-forwarded
//    (SwiftShader renders at ~0.3x real time): a result table with a payout from the economy.
await page.close();
const page2 = await context.newPage();
page2.on("pageerror", (e) => errors.push(String(e) + (process.env.E2E_STACK ? `\n${e.stack}` : "")));
page2.on("console", (m) => (m.type() === "error" && errors.push(m.text())) || (m.text().startsWith("THROW") && console.log(m.text())));
await page2.goto(`${URL}?race=grid_highway_sprint&autopilot=1`);
await page2.waitForFunction(() => window.__wp?.ready === true && window.__wp.raceView.raceId !== "", null, { timeout: 30000 });
if (process.env.E2E_STACK)
  await page2.evaluate(() => {
    const s = window.__wp.sim;
    for (const k of Object.keys(s)) {
      const f = s[k];
      if (typeof f !== "function") continue;
      s[k] = (...a) => {
        try {
          return f(...a);
        } catch (e) {
          console.log(`THROW ${k}(${a.join(",")}): ${e}\n${new Error().stack}`);
          throw e;
        }
      };
    }
  });
for (let i = 0; i < 60; i++) {
  // Events are drained (and race events counted) per step: a 120-step chunk would otherwise
  // hand the page events about traffic cars that have already been despawned.
  const done = await page2.evaluate(() => {
    window.__raceEvents ??= 0;
    for (let k = 0; k < 120; k++) {
      window.__wp.sim.step(1 / 60);
      for (const e of window.__wp.sim.eventsDrain()) if ("Race" in e) window.__raceEvents++;
    }
    return window.__wp.sim.raceInfo()[0] === 2;
  });
  if (done) break;
}
await page2.waitForFunction(() => window.__wp.raceView.report?.final_results === true, null, { timeout: 60000 });
await page2.waitForSelector('[data-testid="race-results"]', { timeout: 10000 });
const rep = await page2.evaluate(() => window.__wp.raceView.report);
check(rep.rows.length === 4, `results table has ${rep.rows.length} rows`);
const finishers = rep.rows.filter((r) => r.status === "Finished").length;
check(finishers >= 2, `${finishers} racers finished (winner ${rep.rows[0].name} in ${rep.rows[0].time_s?.toFixed(1)} s)`);
check(rep.payout !== null && rep.payout.entry_fee > 0, `payout settled by the economy (buy-in ${rep.payout?.entry_fee}, purse ${rep.payout?.purse}, rep ${rep.payout?.rep})`);
await page2.waitForSelector('[data-testid="race-payout"]', { timeout: 5000 });
const raceEvents = await page2.evaluate(() => (window.__wp.app.eventCounts.Race ?? 0) + (window.__raceEvents ?? 0));
check(raceEvents > 3, `SimEvent::Race events reach the page (${raceEvents})`);
if (shots) await page2.screenshot({ path: join(shots, "race_results.png") });

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
stopServer();
console.log(failures ? `${failures} check(s) failed` : "all race checks passed");
process.exit(failures ? 1 : 0);
