// Garage workshop end to end (headless Chromium): open the 3D workshop on a career car, pick
// parts with the mouse, take the front bumper and the radiator off, see the car refuse to drive,
// put them back, buy an aftermarket radiator from the catalog and fit it, then drive.
// Usage: node e2e/workshop.mjs (needs `npm run wasm` first). SHOTS=dir writes screenshots.
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

// A fresh career with a worn beater.
const uid = await page.evaluate(() => {
  const s = window.__wp.sim;
  let v = s.careerNew("Arcade");
  // The cheapest worn car leaves cash for a radiator.
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  v = s.careerBuy(d.def_id, "Worn");
  return v.cars[v.cars.length - 1].uid;
});
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle(1500);
check(await page.locator('[data-testid="workshop"]').isVisible(), "the workshop overlay opens over the 3D bay");
await shot("workshop_overview.png");

// Hover and click pick parts.
check(await clickSlot("front_bumper"), "the front bumper is on screen and clickable");
const card = page.locator('[data-testid="ws-card"]');
check((await card.getAttribute("data-slot")) === "front_bumper", "clicking the bumper opens its part card");

// The radiator cannot come off before the bumper.
await page.evaluate(() => window.__wp.workshop.camera("engine"));
await settle();
await shot("workshop_engine_bay.png");
const rad = await slot("radiator");
check(!!rad.remove_blocker, `radiator removal is blocked while the bumper is on (${rad.remove_blocker})`);

// Bumper off (through the card), radiator off.
await clickSlot("front_bumper");
await button("Remove").click();
await placed();
await settle();
check(!(await slot("front_bumper")).fitted, "the bumper is off");
const hidden = await page.evaluate(() => window.__wp.workshop.pointOfSlot("front_bumper"));
check(hidden === null, "the bumper is no longer drawn on the car");
// The radiator through the same api the card uses (it may sit behind the guard on screen).
let s = await page.evaluate((u) => window.__wp.sim.careerRemovePart(u, "radiator"), uid);
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle();
s = await screen();
check(s.missing.includes("Radiator"), `with the radiator out the car is missing: ${s.missing.join(", ")}`);
check(await button("Drive").isDisabled(), "Drive is disabled while a required part is missing");
check(s.inventory.length === 2, `bumper and radiator are in the inventory (${s.inventory.length} parts)`);
await shot("workshop_parts_off.png");

// Buy an aftermarket radiator through the catalog, fit it from the inventory strip.
await button("Catalog").click();
await page.locator('[data-testid="ws-catalog"] [role="tab"]', { hasText: "Cooling" }).click();
await page.locator('[data-testid="ws-catrow"][data-grade="cooling_radiator_1"] button').click();
await page.locator('[data-testid="ws-catalog"] button', { hasText: "Close" }).click();
s = await screen();
const bought = s.inventory.find((p) => p.part.grade === "cooling_radiator_1");
check(!!bought && bought.fits.includes("radiator"), "the bought radiator is in the inventory and fits the empty slot");
await page.locator(`[data-testid="ws-tile"][data-uid="${bought?.part.uid}"]`).click();
await button("Fit: Radiator").click();
await placed();
await settle();
s = await screen();
check((await slot("radiator")).fitted?.grade === "cooling_radiator_1", "the aftermarket radiator is fitted");
// Bumper back on from the strip.
const bumper = s.inventory.find((p) => p.part.kind === "front_bumper");
await page.locator(`[data-testid="ws-tile"][data-uid="${bumper.part.uid}"]`).click();
await button("Fit: Front bumper").click();
await placed();
await settle();
s = await screen();
check(s.missing.length === 0 && (await slot("front_bumper")).fitted, "bumper back on: the car is ready to drive");
const mods = await page.evaluate((u) => window.__wp.sim.careerView().cars.find((c) => c.uid === u).mods, uid);
check(JSON.stringify(mods).includes("cooling_radiator_1"), "the fitted part counts as an installed mod");

// Every fitted part has a place on the car: mesh, cosmetic piece or workshop hardware.
await page.waitForFunction(() => window.__wp.workshop.hardwareSlots().length > 0, null, { timeout: 15000 });
const hw = await page.evaluate(() => window.__wp.workshop.hardwareSlots());
check(hw.includes("battery") && hw.includes("differential"), `the battery and the diff are drawn (${hw.join(", ")})`);
const undrawn = await page.evaluate(() => window.__wp.workshop.undrawnSlots());
check(undrawn.length === 0, `every fitted part is drawn somewhere on the car (missing: ${undrawn.join(", ")})`);

// Underside on the lift.
await button("Underside").click();
await settle(2500);
await shot("workshop_underside.png");
await button("Wheels").click();
await settle(1200);
await shot("workshop_wheels.png");

// Drive out.
await button("Drive").click();
await page.waitForFunction(() => !window.__wp.workshop.active && window.__wp.sim.careerRunActive(), null, { timeout: 30000 });
check(true, "Drive leaves the workshop and starts a career run with the car");

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "workshop e2e: all passed");
process.exit(failures ? 1 : 0);
