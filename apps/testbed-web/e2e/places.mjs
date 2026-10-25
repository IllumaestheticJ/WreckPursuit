// Career places end to end (headless Chromium): on a career night run, stopping inside a garage
// ring opens the garage panel (workshop / garage / back on the road) and stopping in a safehouse
// ends the night there. Usage: node e2e/places.mjs (needs `npm run wasm` first). SHOTS=dir.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5211;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
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
const screen = () => page.evaluate(() => window.__wp.workshop.current());
const slot = async (id) => (await screen()).slots.find((s) => s.id === id);
const settle = (ms = 900) => page.waitForTimeout(ms);
/** Wait until the car is built and the bay placed. */
const placed = () =>
  page.waitForFunction(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return w.workshop.active && v?.panels.ready && w.scene.world.getObjectByName("workshop-room");
  }, null, { timeout: 30000 });
/** Click the middle of a slot's part on screen. */
async function clickSlot(id) {
  const pt = await page.evaluate((s) => window.__wp.workshop.pointOfSlot(s), id);
  if (!pt) return false;
  await page.mouse.click(pt.x, pt.y);
  await settle(200);
  return true;
}
const button = (text) => page.locator(`[data-testid="workshop"] button`, { hasText: text }).first();

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

await page.evaluate(() => {
  const s = window.__wp.sim;
  const v = s.careerNew("Arcade");
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  s.careerBuy(d.def_id, "Worn");
});
/** Start a career night run in the city and wait for the car. */
async function nightRun() {
  await page.evaluate(() => window.__wp.careerApi.startRun("chase"));
  await page.waitForFunction(() => {
    const w = window.__wp;
    return w.sim.careerRunActive() && w.app.views.get(w.app.player)?.panels?.ready !== false && !w.garage.open;
  }, null, { timeout: 60000 });
  await page.waitForTimeout(1500);
}
/** Put the player's car at rest in the middle of a place and wait for its panel. */
async function pullInto(pos) {
  await page.evaluate((p) => window.__wp.sim.debugPlace(window.__wp.app.player, p[0], p[1], 0, 0), pos);
  await page.waitForSelector('[data-testid="place-visit"] [data-testid="place-leave"]', { timeout: 15000 });
  await page.waitForTimeout(400);
}

await nightRun();
const sites = await page.evaluate(() => ({ g: window.__wp.sim.garages(), h: window.__wp.sim.safehouses() }));
check(sites.g.length > 0 && sites.h.length > 0, `career run lists ${sites.g.length} garages and ${sites.h.length} safehouses`);
const marker = await page.evaluate((n) => !!window.__wp.scene.world.getObjectByName(`safehouse:${n}`), sites.h[0]?.name);
check(marker, "safehouses get a ring and beam in the world");

// Garage: pull in, the panel offers the workshop; it opens with the car.
await pullInto(sites.g[0].pos);
const gv = await page.evaluate(() => window.__wp.placeView.visit);
check(gv?.kind === "Garage" && gv.name === sites.g[0].name, `stopping in ${sites.g[0].name} opens the garage panel`);
await shot("place_garage.png");
await page.click('[data-testid="place-workshop"]');
await page.waitForFunction(() => window.__wp.workshop.active, null, { timeout: 30000 });
check(!(await page.evaluate(() => window.__wp.sim.careerRunActive())), "the run ended at the garage and the workshop opened");
await page.waitForTimeout(1500);
await shot("place_garage_workshop.png");
await page.evaluate(() => window.__wp.workshop.close());

// "Back on the road" gives the car back.
await nightRun();
const startPos = await page.evaluate(() => {
  const w = window.__wp;
  const p = w.app.views.get(w.app.player).root.position;
  return [p.x, p.y];
});
await pullInto(sites.g[0].pos);
await page.click('[data-testid="place-leave"]');
await page.waitForTimeout(500);
const left = await page.evaluate(() => ({ v: window.__wp.sim.place().visit, active: window.__wp.sim.careerRunActive() }));
check(!left.v && left.active, "Back on the road: the run goes on");

// Safehouse: end the night there (drive out of the garage ring first: the start spot).
await page.evaluate((p) => window.__wp.sim.debugPlace(window.__wp.app.player, p[0], p[1], 0, 0), startPos);
await page.waitForTimeout(300);
await pullInto(sites.h[0].pos);
const hv = await page.evaluate(() => window.__wp.placeView.visit);
check(hv?.kind === "Safehouse", `stopping in ${sites.h[0].name} opens the safehouse panel`);
await shot("place_safehouse.png");
await page.click('[data-testid="place-end-night"]');
await page.waitForFunction(() => window.__wp.garage.open, null, { timeout: 15000 });
const after = await page.evaluate(() => ({ active: window.__wp.sim.careerRunActive(), text: document.body.innerText }));
check(!after.active && after.text.includes("for the night"), "End the night: the run ends safely and the garage says so");
await shot("place_safehouse_garage.png");

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "places e2e: all passed");
process.exit(failures ? 1 : 0);
