// e2e: street racers (wp_wasm::street, src/ui/street.ts) in headless Chromium.
// Usage: node e2e/street.mjs (needs the wasm package). Env: SHOTS=dir for screenshots,
// CITY=vesper_v2|procedural_12x12 (default vesper_v2).
// Checks: the roster spawns with level-coloured minimap icons; honking next to a racer opens the
// map picker; clicking the map previews a route; Race! pays the stake and counts down; reaching
// B wins even money; the sim cost with and without street racers.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5201;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
const CITY = process.env.CITY ?? "vesper_v2";
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
const wait = (ms) => page.waitForTimeout(ms);

await page.goto(`${URL}?city=${CITY}`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
// Sim cost without street racers, then with.
const simMs = async () => {
  await wait(6000);
  return page.evaluate(() => window.__wp.app.perf.simMs);
};
await page.evaluate(() => {
  window.__wp.sim.setStreetRacers(false);
  window.__wp.startFreeDrive("lindwurm_4t", "Arcade", true);
});
const off = await simMs();
await page.evaluate(() => {
  window.__wp.sim.setStreetRacers(true);
  window.__wp.startFreeDrive("lindwurm_4t", "Arcade", true);
});
const on = await simMs();
const frameMs = await page.evaluate(() => window.__wp.app.perf.frameMs);
console.log(`sim ms per frame: without street racers ${off.toFixed(2)}, with ${on.toFixed(2)} (frame ${frameMs.toFixed(0)} ms)`);

const st = await page.evaluate(() => window.__wp.street.status);
check(st && st.roster.length === 7, `7 street racers (${st?.roster.length})`);
check(st && new Set(st.levels.map((l) => l.colour)).size === 5, "5 level colours");
check(await page.isVisible("#street-mini"), "minimap visible");
check(await page.isVisible("#street-legend"), "level legend visible");
await shot("street_1_minimap.png");

// Drive up behind racer 5 (level 4) and honk.
const target = st.roster[5];
const placeBehind = () =>
  page.evaluate((e) => {
    const wp = window.__wp;
    const r = wp.sim.streetRacers();
    for (let o = 0; o < r.length; o += 6) {
      if (r[o] !== e) continue;
      const [x, y, yaw] = [r[o + 2], r[o + 3], r[o + 4]];
      wp.sim.debugPlaceVehicle(wp.app.player, x + Math.sin(yaw) * 10, y - Math.cos(yaw) * 10, yaw);
    }
  }, target.entity);
// Headless frames are slow here: re-place behind the (moving) racer while the horn is held.
await page.keyboard.down("KeyQ");
for (let i = 0; i < 30; i++) {
  await placeBehind();
  await wait(250);
  if (await page.evaluate(() => window.__wp.sim.streetStatus()?.offer)) break;
  if (process.env.DEBUG)
    console.log(
      await page.evaluate((e) => {
        const wp = window.__wp;
        const meta = wp.sim.vehicleMeta();
        let flags = -1;
        for (let o = 0; o < meta.length; o += 3) if (meta[o] === wp.app.player) flags = meta[o + 2];
        const st = wp.sim.states();
        const r = wp.sim.streetRacers();
        let d = -1;
        for (let o = 0; o < r.length; o += 6) if (r[o] === e) d = Math.hypot(r[o + 2] - st[1], r[o + 3] - st[2]);
        return JSON.stringify({ flags, d, hint: wp.sim.streetStatus()?.hint?.name, horn: wp.app.input.horn, player: wp.app.player, st0: st[0], held: wp.sim.runResult()?.run_over, active: document.activeElement?.tagName + "#" + document.activeElement?.id, src: wp.app.source });
      }, target.entity),
    );
}
await page.keyboard.up("KeyQ");
await page.waitForFunction(() => window.__wp.sim.streetStatus()?.offer, null, { timeout: 30000 }).catch(() => {});
await page.waitForSelector("#street-map:not(.hidden)", { timeout: 20000 }).catch(() => {});
const offer = await page.evaluate(() => window.__wp.sim.streetStatus()?.offer);
check(offer?.entity === target.entity, `honk opened a challenge from ${target.name} (Lv ${target.level})`);
check(await page.isVisible("#street-map"), "map picker opened");
await wait(500);
await shot("street_2_offer.png");

// Click the map 500–900 m away along some road until the preview is raceable.
const box = await page.locator("#street-map canvas").boundingBox();
let ok = false;
for (const [fx, fy] of [[0.5, 0.25], [0.75, 0.5], [0.25, 0.5], [0.5, 0.75], [0.3, 0.3], [0.7, 0.7]]) {
  await page.mouse.click(box.x + box.width * fx, box.y + box.height * fy);
  await wait(150);
  ok = await page.evaluate(() => !document.querySelector("#st-go")?.disabled);
  if (ok) break;
}
check(ok, "map click previews a raceable route");
await shot("street_3_route.png");
const cash0 = await page.evaluate(() => window.__wp.sim.runAccount().cash);
await page.click("#st-go");
await wait(300);
const race = await page.evaluate(() => window.__wp.sim.streetStatus()?.race);
check(race && race.countdown_s > 0, `race counting down (${race?.countdown_s.toFixed(1)} s)`);
const cash1 = await page.evaluate(() => window.__wp.sim.runAccount().cash);
check(cash1 === cash0 - race.stake, `stake ${race?.stake} paid (${cash0} → ${cash1})`);
await page.waitForSelector("#street-hud:not(.hidden)", { timeout: 20000 }).catch(() => {});
check(await page.isVisible("#street-hud"), "race HUD visible");
await shot("street_4_countdown.png");
// Slow headless frames: wait on sim time, not wall time (green light + 4 s).
await page.waitForFunction(() => (window.__wp.sim.streetStatus()?.race?.elapsed_s ?? 0) > 4, null, { timeout: 120000 }).catch(() => {});
await shot("street_5_racing.png");
// Teleport to B: win.
await page.evaluate(([x, y]) => window.__wp.sim.debugPlaceVehicle(window.__wp.app.player, x, y, 0), race.finish);
await page.waitForFunction(() => window.__wp.sim.streetStatus()?.result, null, { timeout: 30000 }).catch(() => {});
const res = await page.evaluate(() => window.__wp.sim.streetStatus()?.result);
check(res?.won === true, `won (${res?.reason})`);
const cash2 = await page.evaluate(() => window.__wp.sim.runAccount().cash);
check(cash2 === cash0 + race.stake, `even money: ${cash0} → ${cash2}`);
await page.waitForSelector("#street-result:not(.hidden)", { timeout: 20000 }).catch(() => {});
check(await page.isVisible("#street-result"), "result banner visible");
await shot("street_6_result.png");
// Full map with the roster (backquote).
await page.keyboard.press("Backquote");
await page.waitForSelector("#street-map:not(.hidden)", { timeout: 20000 }).catch(() => {});
await wait(3000);
check(await page.isVisible("#street-map"), "` opens the city map");
await shot("street_7_citymap.png");
check(errors.length === 0, `no page errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
process.exit(failures ? 1 : 0);
