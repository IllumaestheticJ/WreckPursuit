// Tow service end to end (headless Chromium; owner 2026-10-28 "tow broken cars to repair
// shops"): a career car is wrecked on a Free Roam run, the "Call a tow truck" prompt shows, the
// tow key opens the destinations with their prices, the tow to the nearest repair shop ends the
// run and opens the garage at that shop, where the wreck is repaired; the ledger has the tow
// line. Then a wreck left on the road gets "Tow to …" in the garage screen.
// Usage: node e2e/tow.mjs (needs `npm run wasm` first). SHOTS=dir for screenshots
// (docs/media/tow/ holds the committed ones).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5217;
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

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const uid = await page.evaluate(() => {
  const s = window.__wp.sim;
  const v = s.careerNew("Arcade");
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  return s.careerBuy(d.def_id, "Worn").active_car;
});

/** A daytime Free Roam career run (no police on the player), car spawned and settled. */
async function roam() {
  await page.evaluate(() => window.__wp.careerApi.startRun("free", { time: "day", weather: "clear" }));
  await page.waitForFunction(() => {
    const w = window.__wp;
    return w.sim.careerRunActive() && w.app.views.get(w.app.player)?.panels?.ready !== false && !w.garage.open;
  }, null, { timeout: 60000 });
  await page.waitForTimeout(2000);
}

// ----- in a run: wrecked → prompt → tow key → nearest shop --------------------------------------
await roam();
await page.evaluate(() => window.__wp.sim.debugTotalLoss(window.__wp.app.player));
await page.waitForSelector('[data-testid="tow-prompt"]', { timeout: 15000 });
const prompt = await page.locator('[data-testid="tow-prompt"]').innerText();
check(/wrecked/i.test(prompt) && /tow truck/i.test(prompt), `the wreck offers a tow (${prompt.replace(/\s+/g, " ")})`);
await shot("01_wrecked_call_a_tow.png");

await page.keyboard.press("Backspace");
await page.waitForSelector('[data-testid="tow-picker"]', { timeout: 10000 });
const offer = await page.evaluate((u) => window.__wp.sim.careerTowOffer(u), uid);
check(offer.ends_run && offer.refused === null && offer.need === "Wrecked", "the tow key opens the picker for the wrecked run car");
const nearest = offer.destinations[0];
check(nearest.nearest && nearest.place.kind === "Garage" && nearest.price > 0, `nearest repair shop first: ${nearest.place.name}, ${(nearest.metres / 1000).toFixed(1)} km, $${nearest.price}`);
const rows = await page.locator('[data-testid="tow-destinations"] tr').count();
check(rows === offer.destinations.length && rows >= 5, `${rows} destinations with prices (every shop; your garage unless the car is in it)`);
await shot("02_tow_destinations.png");

const cashBefore = await page.evaluate(() => window.__wp.sim.careerView().cash);
await page.click(`[data-testid="tow-to-${nearest.place.id}"]`);
await page.waitForFunction(() => window.__wp.garage.open, null, { timeout: 20000 });
const after = await page.evaluate((u) => {
  const v = window.__wp.sim.careerView();
  return { active: window.__wp.sim.careerRunActive(), cash: v.cash, car: v.cars.find((c) => c.uid === u), text: document.body.innerText, place: window.__wp.careerApi.place(), ledger: v.ledger };
}, uid);
check(!after.active, "the tow ended the run");
check(after.place?.kind === "Garage" && after.place.id === nearest.place.id, `the player is at ${nearest.place.name}`);
check(after.car.spot?.kind === "Garage" && after.car.spot.id === nearest.place.id && after.car.total_loss, "the car arrived there, still wrecked");
check(after.cash === cashBefore - nearest.price, `charged $${nearest.price}`);
check(after.text.includes(`Towed to ${nearest.place.name}`), "the garage says where the car was towed");
const towLine = after.ledger.find((l) => l.reason === "Tow");
check(towLine?.cash_delta === -nearest.price && towLine.detail.Tow.to === nearest.place.name, "the ledger has the tow line");
await shot("03_garage_at_the_shop.png");

// Repairs are open at the shop: Repair all.
const repairAll = page.locator('button', { hasText: "Repair all" }).first();
check(await repairAll.isEnabled(), "Repair all is open at the shop the car was towed to");
await repairAll.click();
await page.waitForTimeout(500);
const fixed = await page.evaluate((u) => window.__wp.sim.careerView().cars.find((c) => c.uid === u), uid);
check(fixed.drivable && !fixed.total_loss && fixed.repair_lines.length === 0, "repaired at the destination: it drives again");
await shot("04_repaired_at_the_shop.png");

await page.click('button:has-text("Ledger")');
await page.waitForTimeout(300);
const ledgerText = await page.locator('[data-testid="ledger"]').innerText();
check(new RegExp(`Tow: ${nearest.place.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} → \\d+\\.\\d km`).test(ledgerText), "the ledger tab shows \"Tow: <shop> → n km\"");
await shot("05_ledger_tow_line.png");

// ----- a wreck left on the road: "Tow to …" in the garage --------------------------------------
await page.evaluate(() => window.__wp.careerApi.close());
await roam();
await page.evaluate(() => window.__wp.menu.openPause());
await page.waitForSelector("#gm-tow", { timeout: 5000 });
check(true, "the pause menu offers \"Call a tow truck\" in a career run");
await shot("06_pause_menu_tow.png");
await page.evaluate(() => window.__wp.menu.close());
await page.evaluate(() => window.__wp.sim.debugTotalLoss(window.__wp.app.player));
await page.waitForSelector('[data-testid="tow-prompt"]', { timeout: 15000 });
// Back to the garage without a tow (pause menu → Garage): the wreck stays on the road.
await page.evaluate(() => window.__wp.menu.openPause());
await page.click("#gm-garage");
await page.click("#gm-confirm-yes");
await page.waitForFunction(() => window.__wp.garage.open, null, { timeout: 15000 });
const stranded = await page.evaluate((u) => window.__wp.sim.careerView().cars.find((c) => c.uid === u), uid);
check(stranded.spot?.kind === "Road", "ended without a tow, the wreck is left on the road");
await page.waitForSelector('[data-testid="tow-destinations"]', { timeout: 10000 });
const repairClosed = await page.locator('button', { hasText: "Repair all" }).count();
check(repairClosed === 0, "no repairs on the road: the car needs the tow first");
const workshopOff = await page.locator('.wp-cg-detail button', { hasText: "Workshop" }).first().isDisabled();
check(workshopOff, "no workshop on the road either (drive-to stays for everything but the tow)");
await shot("07_garage_tow_to.png");
const offer2 = await page.evaluate((u) => window.__wp.sim.careerTowOffer(u), uid);
await page.click(`[data-testid="tow-to-${offer2.destinations[0].place.id}"]`);
await page.waitForTimeout(500);
const end = await page.evaluate((u) => ({ car: window.__wp.sim.careerView().cars.find((c) => c.uid === u), place: window.__wp.careerApi.place() }), uid);
check(end.car.spot?.kind === "Garage" && end.place?.id === offer2.destinations[0].place.id, "Tow to … brings it in and the player is at that shop");
await shot("08_garage_after_tow_to.png");

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "tow e2e: all passed");
process.exit(failures ? 1 : 0);
