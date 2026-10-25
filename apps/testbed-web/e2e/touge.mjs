// End-to-end check of touge battles in the browser testbed (lead/touge-port) in headless Chromium.
// Usage: npm run e2e:touge (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome, SHOTS=dir for screenshots.
//
// Checks: the "touge" button opens the picker with the four passes; at rep 0 every battle is
// locked; the testbed rep buttons open bosses, side bets and pink slips; a boss battle with a
// side bet starts a two-car session on the pass (NPC on pole, no traffic, no police) and takes
// the stake from the session's cash ledger; guard rails are drawn; a car put through the wall is
// totaled and loses at once, and the side bet settles as lost; no page errors.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5207;
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
// Plain sessions only: no career (a career run would read the career's rep).
await page.evaluate(() => {
  const c = document.getElementById("career");
  if (c) c.checked = false;
});

// 1. The picker lists the four passes; at rep 0 every battle is locked.
await page.click("#sc-touge");
await page.waitForSelector('[data-testid="touge-picker"]', { timeout: 10000 });
const passes = await page.$$eval("[data-touge]", (els) => els.map((e) => e.getAttribute("data-touge")));
check(passes.length === 4, `touge picker lists ${passes.length} passes (${passes.join(", ")})`);
const menu = (choice) => page.evaluate((c) => window.__wp.sim.tougeMenu(c, false), choice);
const boss = { map: passes[0], direction: "down", daylight: "night", surface: "dry", opponent: "boss" };
await page.click('[data-testid="touge-set-rep"] button >> text="0"');
let m = await menu(boss);
check(!m.race.unlocked && !m.side_bet.unlocked && !m.pink_slip.unlocked, `rep 0: boss, side bet and pink slip locked (${m.race.locked_reason})`);
check(await page.$eval('[data-testid="touge-picker"] button:has-text("Battle")', (b) => b.disabled), "Battle button disabled while locked");

// 2. Testbed rep 10000 opens everything.
await page.click('[data-testid="touge-set-rep"] button >> text="10000"');
m = await menu(boss);
check(m.race.unlocked && m.side_bet.unlocked && m.pink_slip.unlocked, `rep ${m.rep}: boss, side bet and pink slip open (${m.tier})`);
check(m.max_stake > 0 && m.max_stake <= m.cash, `side bet capped by cash (max ${m.max_stake}, cash ${m.cash})`);
await shot("touge_picker.png");

// 3. Boss battle, downhill at night, with a side bet: a two-car session on the pass.
await page.click('[data-testid="touge-opponent"] button:has-text("Boss")');
await page.click('[data-testid="touge-direction"] button:has-text("Downhill")');
await page.click('[data-testid="touge-daylight"] button:has-text("Night")');
await page.click('[data-testid="touge-surface"] button:has-text("Dry")');
await page.check('input[name="touge-wager"] >> nth=1');
await page.waitForSelector('[data-testid="touge-stake"]', { timeout: 5000 });
const stake = await page.$eval('[data-testid="touge-stake"]', (e) => Number(e.value));
const cash0 = m.cash;
await page.click('[data-testid="touge-picker"] button:has-text("Battle")');
await page.waitForFunction(() => window.__wp.sim.scenario().name.startsWith("race_touge:"), null, { timeout: 30000 });
const scName = await page.evaluate(() => window.__wp.sim.scenario().name);
check(scName === `race_touge:${passes[0]}:down:night:dry:boss`, `picker started the battle session (${scName})`);
const entities = await page.evaluate(() => window.__wp.sim.entities());
check(entities.length === 2, `player + the boss only, no traffic or police (${entities.length} entities)`);
// The wager is taken at the first tick.
await page.evaluate(() => {
  for (let k = 0; k < 4; k++) window.__wp.sim.step(1 / 60);
});
await page.waitForSelector('[data-testid="touge-battle"]', { timeout: 10000 });
let r = await page.evaluate(() => window.__wp.sim.tougeResult());
check(r !== null && r.wager.kind === "side_bet" && r.wager_taken, `side bet of ${stake} taken (${r?.wager_error ?? "ok"})`);
const cash1 = await page.evaluate(() => window.__wp.sim.runAccount()?.cash ?? null);
check(cash1 === cash0 - stake, `stake left the session's cash (${cash0} → ${cash1})`);
const rails = await page.evaluate(() => {
  const s = window.__wp.sim.propsStatic();
  const stride = 11; // wp_wasm::props::PROP_STRIDE
  let n = 0;
  for (let o = 0; o < s.length; o += stride) if (s[o] === 3) n++; // guard rail (PR #75; K-rails are gone)
  return n;
});
check(rails > 10, `guard rail sections on the pass (${rails})`);
await shot("touge_grid.png");

// 4. Put the player through the wall: totaled, battle lost at once, side bet lost.
const [player] = entities;
await page.evaluate((p) => {
  const s = window.__wp.sim.states();
  for (let o = 0; o < s.length; o += 50) {
    if (s[o] !== p) continue;
    const x = s[o + 1];
    const y = s[o + 2];
    // 40 m to one side of the grid slot: far past the wall line.
    window.__wp.sim.debugPlace(p, x + 40, y, 0, 0);
  }
}, player);
for (let i = 0; i < 20; i++) {
  const done = await page.evaluate(() => {
    for (let k = 0; k < 30; k++) window.__wp.sim.step(1 / 60);
    const t = window.__wp.sim.tougeResult();
    return t?.settlement != null || t?.settle_error != null;
  });
  if (done) break;
}
r = await page.evaluate(() => window.__wp.sim.tougeResult());
check(r.outcome?.result === "Lost" && r.outcome?.totaled === player, `through the wall: totaled and lost (${JSON.stringify(r.outcome)})`);
check(r.settle_error === null, `settled without error (${r.settle_error})`);
check(r.settlement?.bet_net === -stake && r.settlement?.bet_payout === 0, `side bet lost (net ${r.settlement?.bet_net})`);
check((r.settlement?.rep ?? 0) < 0, `rep lost for the defeat and the wreck (${r.settlement?.rep})`);
await page.waitForSelector('[data-testid="touge-settlement"]', { timeout: 10000 });
await shot("touge_lost.png");

// Playtest row 167: the career garage's "Touge battles" opens the touge picker on the career's
// account (a fresh career: everything locked, with the tier it needs); closing it goes back to
// the garage.
{
  const p3 = await context.newPage();
  p3.on("pageerror", (e) => errors.push(String(e)));
  await p3.goto(URL);
  await p3.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
  await p3.evaluate(() => {
    const wp = window.__wp;
    wp.careerApi.newCareer("Arcade");
    wp.careerApi.buy("junkyard_beater", "Worn");
    wp.openGarage();
  });
  await p3.waitForSelector('[data-testid="career-touge"]', { timeout: 10000 });
  await p3.click('[data-testid="career-touge"]');
  await p3.waitForSelector('[data-testid="touge-picker"]', { timeout: 10000 });
  const cm = await p3.evaluate(() => {
    const wp = window.__wp;
    const map = wp.sim.tougeList()[0].id;
    return { garage: wp.garage.open, m: wp.sim.tougeMenu({ map, direction: "down", daylight: "night", surface: "dry", opponent: "challenger" }, true) };
  });
  check(
    !cm.garage && cm.m.tier === "unknown" && !cm.m.race.unlocked && /Known/.test(cm.m.race.locked_reason ?? ""),
    `career garage: Touge battles opens the picker on the career (tier ${cm.m.tier}; ${cm.m.race.locked_reason})`,
  );
  check(await p3.$eval('[data-testid="touge-picker"] button:has-text("Battle")', (b) => b.disabled), "career touge: Battle disabled while locked");
  await p3.click('[data-testid="touge-picker"] button:has-text("close")');
  check(await p3.evaluate(() => window.__wp.garage.open), "closing the career touge picker returns to the garage");
  await p3.close();
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
stopServer();
console.log(failures ? `${failures} check(s) failed` : "all touge checks passed");
process.exit(failures ? 1 : 0);
