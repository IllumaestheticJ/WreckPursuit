// Buying upgrades inside the garage workshop, end to end (headless Chromium): click a part on
// the car, its card lists the slot's upgrades with prices; buy one and have it fitted; a part
// that needs another part first stays clickable and buys both together (fitted in order, or
// into the inventory); bolt-on upgrades (brake pads...) are sold there too; the career garage
// has no Mods screen any more.
// Usage: node e2e/workshop_buy.mjs (needs `npm run wasm` first). SHOTS=dir writes screenshots.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = Number(process.env.PORT ?? 5212);
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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1243/chrome-linux64/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const screen = () => page.evaluate(() => window.__wp.workshop.current());
const settle = (ms = 900) => page.waitForTimeout(ms);
const placed = () =>
  page.waitForFunction(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return w.workshop.active && v?.panels.ready && w.scene.world.getObjectByName("workshop-room");
  }, null, { timeout: 30000 });
const button = (text) => page.locator(`[data-testid="workshop"] button`, { hasText: text }).first();
const card = () => page.locator('[data-testid="ws-card"]');
const row = (grade) => card().locator(`[data-testid="ws-catrow"][data-grade="${grade}"]`);

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

const uid = await page.evaluate(() => {
  const s = window.__wp.sim;
  let v = s.careerNewWithCash("Arcade", 80000);
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  v = s.careerBuy(d.def_id, "Worn");
  return v.cars[v.cars.length - 1].uid;
});
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle(1200);

/** Select a slot: click it on the car when it is drawn, else through the part list. */
async function pick(id, clickOnCar = false) {
  if (clickOnCar) {
    const pt = await page.evaluate((s) => window.__wp.workshop.pointOfSlot(s), id);
    if (pt) {
      await page.mouse.move(pt.x, pt.y);
      await settle(250);
      return pt;
    }
  }
  if (!(await page.locator('[data-testid="ws-partlist"]').isVisible())) await button("Part list").click();
  await page.locator(`[data-testid="ws-slotrow"][data-slot="${id}"]`).click();
  if (await page.locator('[data-testid="ws-partlist"]').isVisible()) await button("Part list").click();
  await settle(200);
  return null;
}
const kindOf = async (grade) => (await screen()).catalog.find((c) => c.grade === grade)?.kind;
const slotOfKind = async (kind) => (await screen()).slots.find((s) => s.kind === kind)?.id;

// 1. Hover a part on the car: the tag says what upgrades cost; click it: the card lists them.
await button("Overview").click();
await settle(1200);
const bumper = (await screen()).slots.find((s) => s.id.startsWith("bumper_front") || s.kind === "bumper_front")?.id ?? "hood";
const pt = await pick(bumper, true);
if (pt) {
  const tag = await page.locator(".wp-ws-hover").textContent().catch(() => "");
  check(/upgrades from \$/.test(tag ?? ""), `hovering ${bumper} shows the upgrade price (${tag})`);
  await shot("workshop_buy_hover.png");
  await page.mouse.click(pt.x, pt.y);
  await settle(300);
} else {
  await pick(bumper);
}
check((await card().getAttribute("data-slot")) === bumper, `clicking the part opens its card (${bumper})`);

// 2. Radiator: upgrades with prices on the card, one fitted by the mechanic from there.
const radiator = await slotOfKind(await kindOf("cooling_radiator_1"));
await pick(radiator);
const rows = await card().locator('[data-testid="ws-upgrades"] [data-testid="ws-catrow"]').count();
check(rows > 0, `the radiator card lists ${rows} upgrades`);
await shot("workshop_buy_card.png");
const cash0 = (await screen()).cash;
await row("cooling_radiator_1").locator('[data-testid="ws-cat-mechanic"]').click();
await settle(1500);
await placed();
let s = await screen();
check(s.slots.find((x) => x.id === radiator)?.fitted?.grade === "cooling_radiator_1", "Fit for me on the card fitted the radiator");
check(s.cash < cash0, `and charged for it ($${cash0} → $${s.cash})`);
check((await row("cooling_radiator_1").getAttribute("data-owned")) === "fitted", "the card greys the fitted radiator out (on the car)");
check(await row("cooling_radiator_1").locator('[data-testid="ws-cat-mechanic"]').isDisabled(), "and its Fit for me is disabled");

// 3. Brake kit needs brake pads (a bolt-on): clickable, buys both, fitted in order.
const brakeKind = await kindOf("brakes_kit_2");
const brake = await slotOfKind(brakeKind);
await pick(brake);
const needs = await row("brakes_kit_2").locator('[data-testid="ws-needs"]').textContent().catch(() => null);
check(!!needs && /pad/i.test(needs), `the race brake kit says what it needs (${needs})`);
const both = row("brakes_kit_2").locator('[data-testid="ws-fit-both"]');
check(await both.isEnabled(), "and offers to buy both");
await row("brakes_kit_2").scrollIntoViewIfNeeded();
await shot("workshop_buy_needs.png");
await both.click();
// A big spend asks first.
if (await card().locator('[data-testid="ws-confirm"]').isVisible().catch(() => false)) {
  await card().locator('[data-testid="ws-confirm"] [data-confirm="yes"]').click();
}
await settle(1500);
await placed();
s = await screen();
const kitOn = s.slots.filter((x) => x.kind === brakeKind).every((x) => x.fitted?.grade === "brakes_kit_2");
check(kitOn, "the brake kit is fitted on every corner");
const tree = await page.evaluate((u) => window.__wp.sim.careerMods(u), uid);
check(tree.mods.find((m) => m.id === "brakes_pads_1")?.installed, "and the pads it needed went on first");
const notice = await page.locator('[data-testid="ws-notice"]').textContent().catch(() => "");
check(/Bought and fitted/.test(notice ?? ""), `notice: ${notice}`);
await pick(brake);
await shot("workshop_buy_both_done.png");

// 4. ECU stage 2 needs the intake: buy both into the inventory, to fit by hand.
const ecuKind = await kindOf("engine_ecu_2");
const ecu = await slotOfKind(ecuKind);
await pick(ecu);
const loose = row("engine_ecu_2").locator('[data-testid="ws-buy-both"]');
if (await loose.count()) {
  await loose.click();
  if (await card().locator('[data-testid="ws-confirm"]').isVisible().catch(() => false)) {
    await card().locator('[data-testid="ws-confirm"] [data-confirm="yes"]').click();
  }
  await settle(600);
  s = await screen();
  const grades = s.inventory.map((p) => p.part.grade);
  check(grades.includes("engine_ecu_2") && grades.includes("engine_intake_1"), "Buy both put the ECU and the intake in the inventory");
} else {
  check(false, "the ECU offers Buy both into the inventory");
}

// 5. The catalog sells the bolt-on upgrades too.
await button("Full catalog").click();
await page.locator('[data-testid="ws-catalog"] [role="tab"]', { hasText: "Electronics" }).first().click().catch(() => {});
await settle(200);
const radar = await page.locator('[data-testid="ws-catalog"] [data-testid="ws-catrow"][data-grade="elec_radar_1"]').count();
check(radar === 1, "the catalog's Electronics tab sells the radar detector");
await shot("workshop_buy_catalog.png");
await page.keyboard.press("Escape");

// 6. The career garage has no Mods screen.
await page.keyboard.press("Escape");
await settle(800);
const modsBtn = await page.locator(".wp-cg-detail button", { hasText: /^Mods$/ }).count();
check(modsBtn === 0, "the career garage has no Mods button");
await shot("workshop_buy_garage.png");

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "workshop buy e2e: all passed");
process.exit(failures ? 1 : 0);
