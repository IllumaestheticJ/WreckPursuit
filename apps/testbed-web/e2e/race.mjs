// End-to-end check of street races in the browser testbed (workstream 15) in headless Chromium.
// Usage: npm run e2e:race (needs `npm run wasm` first; `npm run e2e` runs it after run.mjs).
// Env: CHROMIUM=/path/to/chrome, SHOTS=dir for screenshots.
//
// Checks: the "races" button opens the picker with every race the city offers (Loop 9 night
// and day since 2026-10-24); picking Loop 9 night starts a race session (countdown, HUD, player held on the grid, gate marker in the
// world); after the green light the AI racers move and show on the city minimap; on autopilot the race runs to a results
// table with a payout from the economy; no page errors.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
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
// Owner, 2026-10-24: Port Vesper offers Loop 9 night and day only (the highway run needs the
// Ashpeak map; every older race is retired).
check(cards.join(",") === "loop9_circuit,loop9_day", `race picker lists the Loop 9 races (${cards.join(", ")})`);
for (const id of ["hollins_sprint", "lantern_checkpoint", "grid_highway_sprint", "harbor_dash"]) check(!cards.includes(id), `picker no longer offers ${id}`);
// Betting: every card prices the player (odds on yourself); a fresh account below Street rep
// sees them with the lock reason, an unlocked one gets the bet slip.
const betRows = await page.$$eval('[data-testid="bet-locked"], [data-testid="bet-slip"]', (els) => els.map((e) => e.textContent ?? ""));
check(betRows.length === cards.length && betRows.every((t) => /\d\.\d\d/.test(t)), `every race card shows odds on the player (${betRows[0] ?? "none"})`);
await shot("race_picker.png");

// 2. Pick Loop 9 night from the menu: a race session with countdown and HUD.
await page.click('[data-race="loop9_circuit"] button');
await page.waitForFunction(() => window.__wp.raceView.raceId === "loop9_circuit", null, { timeout: 30000 });
const scName = await page.evaluate(() => window.__wp.sim.scenario().name);
check(scName === "race_loop9_circuit", `menu started the race session (${scName})`);
check((await page.$('[data-testid="race-picker"]')) === null, "picker closed");
await page.waitForSelector('[data-testid="race-hud"]', { timeout: 10000 });
check((await page.$('[data-testid="race-countdown"]')) !== null, "countdown shows on the grid");
const entities = await page.evaluate(() => window.__wp.sim.entities());
check(entities.length === 6, `player + 5 AI racers (${entities.length} entities)`);
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
check(aiMoved >= 3, `AI racers move after the green light (${aiMoved}/5 moved > 15 m)`);
const st = await page.evaluate(() => Array.from(window.__wp.sim.raceStandings()));
check(st.length === 6 * 10, `live standings for 6 racers (${st.length / 10})`);
check((await page.$('[data-testid="race-minimap"]')) !== null, "race minimap (UI kit) shows the route");
// The city minimap draws the AI racers (owner row 100): map_dynamic v2 racer block.
const mapRacers = await page.evaluate(() => ({ n: window.__wp.sim.mapDynamic(window.__wp.app.player)[20], drawn: window.__wp.map.hud.stats().minimap.racers }));
check(mapRacers.n === 6 && mapRacers.drawn === 5, `city minimap shows the 5 AI racers (map racers ${mapRacers.n}, drawn ${mapRacers.drawn})`);
await shot("race_hud.png");

// 4. A fresh page races the grid highway sprint on autopilot (the AI drives the player too),
//    fast-forwarded (SwiftShader renders at ~0.3x real time): a result table with a payout
//    from the economy. The sprint is retired from the picker (2026-10-24), but the
//    `?race=<id>` debug link still starts it, and it is short.
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

// Playtest row 167: the career garage's "Street races" opens the picker as a career run: the
// career car races, the buy-in comes off the career, and the results lead back to the garage.
{
  const page3 = await context.newPage();
  page3.on("pageerror", (e) => errors.push(String(e)));
  await page3.goto(`${URL}?autopilot=1`);
  await page3.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
  const G = '[data-testid="career-garage"]';
  const before = await page3.evaluate(() => {
    const wp = window.__wp;
    wp.careerApi.newCareer("Arcade");
    const v = wp.careerApi.buy("junkyard_beater", "Worn");
    wp.openGarage();
    return v;
  });
  await page3.waitForSelector(G, { timeout: 10000 });
  await page3.click('[data-testid="career-races"]');
  await page3.waitForSelector('[data-race="loop9_circuit"]', { timeout: 10000 });
  const pickerOpen = await page3.evaluate(() => !window.__wp.garage.open && (window.__wp.raceView.cards?.length ?? 0) >= 2);
  check(pickerOpen, "career garage: Street races opens the race picker (garage closed)");
  await page3.click('[data-race="loop9_circuit"] button:has-text("Race")');
  const started = await page3.evaluate(() => {
    const wp = window.__wp;
    return { active: wp.sim.careerRunActive(), name: wp.sim.scenario().name, car: wp.sim.scenario().entities[0].car, cash: wp.sim.runAccount()?.cash ?? null };
  });
  check(
    started.active && started.name.startsWith("race_loop9_circuit") && started.car === "junkyard_beater" && started.cash !== null && started.cash < before.cash,
    `career race: a career run with the career car (${started.car}), buy-in off the career ($${before.cash} → $${started.cash})`,
  );
  // A 10 km lap: up to ~10 min of race time in 2 s chunks.
  for (let i = 0; i < 300; i++) {
    const done = await page3.evaluate(() => {
      for (let k = 0; k < 120; k++) window.__wp.sim.step(1 / 60);
      return window.__wp.sim.raceInfo()[0] === 2;
    });
    if (done) break;
  }
  await page3.waitForFunction(() => window.__wp.raceView.report?.final_results === true, null, { timeout: 60000 });
  const back = await page3.waitForSelector('[data-testid="race-to-garage"]', { timeout: 10000 }).then(() => true).catch(() => false);
  check(back, "career race results offer Back to garage");
  if (back) await page3.click('[data-testid="race-to-garage"]');
  const after = await page3.evaluate(() => ({ open: window.__wp.garage.open, active: window.__wp.sim.careerRunActive(), view: window.__wp.careerApi.view() }));
  check(
    after.open && !after.active && after.view.day === before.day + 1 && after.view.cash !== before.cash,
    `career race → garage: run settled on the career (night ${before.day} → ${after.view.day}, $${before.cash} → $${after.view.cash})`,
  );
  // Closing a career picker without racing lands back in the garage too.
  await page3.click('[data-testid="career-races"]');
  await page3.waitForSelector('[data-race]', { timeout: 10000 });
  await page3.click('[data-testid="race-picker"] button:has-text("close")');
  const closed = await page3.evaluate(() => window.__wp.garage.open && !window.__wp.raceView.cards);
  check(closed, "closing the career race picker returns to the garage");
  await page3.close();
}

// Betting on a career race: the stake slider moves (and keeps its value through the picker's
// redraws), the bet is settled on the results panel as soon as the player is home, and the
// stake and any payout land on the career account.
{
  const page4 = await context.newPage();
  page4.on("pageerror", (e) => errors.push(String(e)));
  await page4.goto(`${URL}?autopilot=1`);
  await page4.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
  await page4.evaluate(() => {
    const wp = window.__wp;
    wp.careerApi.newCareer("Arcade");
    wp.careerApi.buy("junkyard_beater", "Worn");
    // Street rep, so city betting is open.
    const save = JSON.parse(wp.sim.careerSave());
    save.account.rep = 2000;
    wp.sim.careerLoad(JSON.stringify(save));
    wp.openGarage();
  });
  await page4.waitForSelector('[data-testid="career-garage"]', { timeout: 10000 });
  await page4.click('[data-testid="career-races"]');
  const card = '[data-race="loop9_circuit"]';
  await page4.waitForSelector(`${card} [data-testid="bet-pick"]`, { timeout: 10000 });
  const winLabel = await page4.$eval(`${card} [data-testid="bet-pick"]`, (el) => [...el.options].map((o) => o.value).find((v) => v.startsWith("win:")));
  await page4.selectOption(`${card} [data-testid="bet-pick"]`, winLabel);
  const cardOdds = await page4.evaluate((l) => window.__wp.raceView.cards.find((c) => c.id === "loop9_circuit").odds.find((o) => o.label === l).odds, winLabel);
  const stakeText = () => page4.$eval(`${card} [data-testid="bet-stake"]`, (el) => el.textContent);
  const before = await stakeText();
  await page4.focus(`${card} [data-testid="bet-range"]`);
  for (let i = 0; i < 3; i++) await page4.keyboard.press("ArrowRight");
  // The picker redraws while a race session is up: the stake must survive it.
  await page4.evaluate(() => window.__wp.raceView.draw());
  const moved = await stakeText();
  check(before === "$500" && moved === "$800", `bet slip stake slider moves (${before} → ${moved})`);
  await page4.click(`${card} button:has-text("Race")`);
  await page4.waitForFunction(() => window.__wp.sim.careerRunActive(), null, { timeout: 30000 });
  for (let i = 0; i < 300; i++) {
    const done = await page4.evaluate(() => {
      for (let k = 0; k < 120; k++) window.__wp.sim.step(1 / 60);
      return window.__wp.raceView.report !== null;
    });
    if (done) break;
  }
  const bet = await page4.evaluate(() => window.__wp.raceView.report?.bets?.[0] ?? null);
  check(bet !== null && bet.stake === 800 && bet.outcome !== "open", `bet placed at $800 and settled at the player's finish (${JSON.stringify(bet)})`);
  check(bet !== null && Math.abs(bet.odds - cardOdds) < 1e-3, `ticket keeps the card's odds (${cardOdds} → ${bet?.odds})`);
  const paidEarly = await page4.evaluate(() => window.__wp.raceView.report?.payout != null);
  check(paidEarly, "purse settled with the player's result");
  await page4.waitForSelector('[data-testid="race-to-garage"]', { timeout: 60000 });
  await page4.click('[data-testid="race-to-garage"]');
  const ledger = await page4.evaluate(() => window.__wp.careerApi.view().ledger.map((l) => l.cash_delta));
  const paid = bet?.outcome === "won" ? bet.paid : null;
  check(ledger.includes(-800) && (paid === null || ledger.includes(paid)), `bet stake${paid ? " and payout" : ""} on the career ledger`);
  await page4.close();
}

// Quick play, second race: after a race the picker opened from the results ("Races…") redraws
// ten times a second; the stake slider must still move and hold its value.
{
  // A fresh context: no career saved by the blocks above, so this is quick play.
  const ctx5 = await browser.newContext({ viewport: { width: 1400, height: 860 } });
  const page5 = await ctx5.newPage();
  page5.on("pageerror", (e) => errors.push(String(e)));
  await page5.goto(`${URL}?autopilot=1`);
  await page5.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
  await page5.click("#sc-races");
  await page5.waitForSelector('[data-race="loop9_circuit"]', { timeout: 10000 });
  await page5.click('[data-race="loop9_circuit"] button:has-text("Race")');
  await page5.waitForFunction(() => window.__wp.raceView.raceId === "loop9_circuit", null, { timeout: 30000 });
  for (let i = 0; i < 300; i++) {
    const done = await page5.evaluate(() => {
      for (let k = 0; k < 120; k++) window.__wp.sim.step(1 / 60);
      return window.__wp.raceView.report !== null;
    });
    if (done) break;
  }
  await page5.evaluate(() => window.__wp.sim.tougeDebugSetRep(2000));
  await page5.click('button:has-text("Races…")');
  const card = '[data-race="loop9_day"]';
  await page5.waitForSelector(`${card} [data-testid="bet-pick"]`, { timeout: 10000 });
  const label = await page5.$eval(`${card} [data-testid="bet-pick"]`, (el) => [...el.options].map((o) => o.value).find((v) => v.startsWith("win:")));
  await page5.selectOption(`${card} [data-testid="bet-pick"]`, label);
  await page5.focus(`${card} [data-testid="bet-range"]`);
  for (let i = 0; i < 4; i++) await page5.keyboard.press("ArrowRight");
  await page5.waitForTimeout(600);
  const held = await page5.$eval(`${card} [data-testid="bet-stake"]`, (el) => el.textContent);
  check(held === "$900", `stake moves and holds through the picker's redraws after a race (${held})`);
  await ctx5.close();
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
stopServer();
console.log(failures ? `${failures} check(s) failed` : "all race checks passed");
process.exit(failures ? 1 : 0);
