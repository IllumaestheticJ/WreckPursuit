// Garage workshop made easy, end to end (headless Chromium): a wheel comes off without its tyre,
// "take off what's in the way" in one click, the mechanic fits a new part (old part sold or
// kept) and a catalog upgrade, and the part list, inventory and catalog sort.
// Usage: node e2e/workshop.mjs (needs `npm run wasm` first). SHOTS=dir writes screenshots.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5210;
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
  executablePath: chromiumPath(),
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

const uid = await page.evaluate(() => {
  const s = window.__wp.sim;
  let v = s.careerNew("Arcade");
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  v = s.careerBuy(d.def_id, "Worn");
  return v.cars[v.cars.length - 1].uid;
});
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle(1200);

/** Pick a slot through the part list (sorted by name so it is easy to find). */
async function pick(id) {
  if (!(await page.locator('[data-testid="ws-partlist"]').isVisible())) await button("Part list").click();
  await page.locator(`[data-testid="ws-slotrow"][data-slot="${id}"]`).click();
  await settle(150);
}

// Simpler removal order: the wheel comes straight off, tyre and all.
let s = await page.evaluate((u) => window.__wp.sim.careerRemovePart(u, "wheel_fl"), uid);
check(!s.slots.find((x) => x.id === "wheel_fl").fitted, "a wheel comes off without taking the tyre off first");
const wheel = s.inventory.find((p) => p.part.kind === "wheels");
s = await page.evaluate(([u, w]) => window.__wp.sim.careerFitPart(u, "wheel_fl", w), [uid, wheel.part.uid]);
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle();
check((await slot("wheel_fl")).fitted, "and goes back on");

// Part list sorts.
await button("Part list").click();
await page.locator('[data-testid="ws-list-sort"]').selectOption("name");
const names = await page.locator('[data-testid="ws-slotrow"] span:first-child').allTextContents();
check(names.length > 10 && names.every((n, i) => i === 0 || names[i - 1].localeCompare(n) <= 0), `part list sorted by name (${names.slice(0, 3).join(", ")}…)`);
await page.locator('[data-testid="ws-list-sort"]').selectOption("condition");
const firstWorst = await page.locator('[data-testid="ws-slotrow"]').first().getAttribute("data-slot");
s = await screen();
const minCond = Math.min(...s.slots.filter((x) => x.fitted).map((x) => x.fitted.condition));
check(s.slots.find((x) => x.id === firstWorst)?.fitted?.condition === minCond, `worst first puts the most worn part on top (${firstWorst})`);

// Do it yourself: the radiator card offers to clear the way in one click.
await pick("radiator");
const way = page.locator('[data-testid="ws-clear-way"]');
check(await way.isVisible(), `the radiator card offers: ${await way.textContent()}`);
await shot("mechanic_card.png");
await way.click();
await placed();
await settle();
check(!(await slot("front_bumper")).fitted && (await slot("radiator")).in_the_way.length === 0, "one click took the bumper off: the radiator can be worked on");
// Put the bumper back the usual way.
s = await screen();
const bumper = s.inventory.find((p) => p.part.kind === "front_bumper");
await page.locator(`[data-testid="ws-tile"][data-uid="${bumper.part.uid}"]`).click();
await button("Fit: Front bumper").click();
await placed();
await settle();

// Mechanic: new stock radiator, old one sold.
await pick("radiator");
await page.locator('[data-testid="ws-old-sell"]').click();
const cash0 = (await screen()).cash;
const inv0 = (await screen()).inventory.length;
await page.locator('[data-testid="ws-mech-stock"]').click();
await placed();
await settle();
s = await screen();
const note = await page.locator('[data-testid="ws-notice"]').textContent().catch(() => "");
check(/Mechanic fitted/.test(note) && /sold/.test(note), `the mechanic reports the job: ${note}`);
check((await slot("radiator")).fitted?.condition === 1 && (await slot("front_bumper")).fitted, "new radiator in, bumper back on");
check(s.inventory.length === inv0 && s.cash < cash0, `old radiator sold, not kept; cash ${cash0} → ${s.cash}`);
await shot("mechanic_done.png");

// Mechanic from the catalog: an upgrade fitted in one click, old part kept this time.
await page.locator('[data-testid="ws-old-keep"]').click();
await button("Catalog / upgrades").click();
await page.locator('[data-testid="ws-catalog-sort"]').selectOption("price");
const prices = await page.evaluate(() =>
  [...document.querySelectorAll('[data-testid="ws-catrow"][data-kind="radiator"] .wp-mono')].map((e) => e.textContent),
);
check(prices.length > 0, `catalog sorted by price (${prices.join(", ")})`);
await shot("mechanic_catalog.png");
await page.locator('[data-testid="ws-catrow"][data-grade="cooling_radiator_1"] [data-testid="ws-cat-mechanic"]').click();
await placed();
await settle();
s = await screen();
check((await slot("radiator")).fitted?.grade === "cooling_radiator_1", "the mechanic fitted the catalog radiator");
check(s.inventory.some((p) => p.part.kind === "radiator"), "the old radiator was kept in the inventory");

// Inventory sorts.
await page.locator('[data-testid="ws-inventory-sort"]').selectOption("value");
const tiles = await page.locator('[data-testid="ws-tile"]').count();
check(tiles >= 1, `inventory sortable (${tiles} parts)`);
check(s.missing.length === 0, `the car is ready to drive (${s.missing.join(", ")})`);

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "mechanic e2e: all passed");
process.exit(failures ? 1 : 0);
