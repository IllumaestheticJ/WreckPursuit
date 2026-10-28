// Car lot visit end to end (headless Chromium): visit the fullest dealership, browse the rows
// with the arrow keys and check the highlighted car always stays in view in the list (it used to
// scroll off the bottom), the camera follows it, the panel fits the screen at laptop and phone
// sizes, Enter asks to buy, buying moves on to the next car and Esc goes back to the garage. Usage: node e2e/lot_visit.mjs (needs `npm run wasm` first). SHOTS=dir.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5213;
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
const page = await (await browser.newContext({ viewport: { width: 1366, height: 768 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const L = '[data-testid="lot-visit"]';

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

// A rich career so every tier of dealership is open; visit the one with the most cars today.
const lotId = await page.evaluate(() => {
  const v = window.__wp.sim.careerNewWithCash("Arcade", 5_000_000);
  const lots = v.lots.filter((l) => l.kind === "Dealership").sort((a, b) => b.in_stock - a.in_stock);
  return lots[0].id;
});
await page.evaluate((id) => window.__wp.careerApi.visitLot(id), lotId);
await page.waitForSelector(L, { timeout: 20000 });
await page.waitForTimeout(1500);
await shot("lot_visit_open.png");
/** A wide look over the whole lot from behind the gate. */
const overview = async (name) => {
  await page.evaluate(() => window.__wp.lookFrom([-26, -26, 17], [0, 22, 0]));
  await page.waitForTimeout(800);
  await shot(name);
};
await overview("lot_dealership_overview.png");
const drawn = await page.evaluate(() => {
  const r = window.__wp.scene.world.getObjectByName("lot-dressing");
  return r ? r.children.length : 0;
});
check(drawn > 20, `the lot is dressed (${drawn} pieces)`);
await page.keyboard.press("ArrowRight");
await page.keyboard.press("ArrowLeft");
await page.waitForTimeout(600);

/** Where the selected card sits inside the scrolling list (and whether it is fully visible). */
const selected = () =>
  page.evaluate((L) => {
    const list = document.querySelector(`${L} [role="listbox"]`);
    const card = list?.querySelector('[aria-selected="true"]');
    if (!list || !card) return null;
    const a = list.getBoundingClientRect();
    const b = card.getBoundingClientRect();
    return {
      slot: Number(card.getAttribute("data-slot")),
      visible: b.top >= a.top - 1 && b.bottom <= a.bottom + 1 && b.bottom <= innerHeight && b.top >= 0,
      count: list.querySelectorAll('[data-slot]:not(:disabled)').length,
    };
  }, L);

const first = await selected();
check(!!first && first.count >= 6, `dealership ${lotId} lists ${first?.count ?? 0} cars`);
// Walk the whole lot forward and back; the highlighted card must stay fully on screen.
let allVisible = true;
let camOk = true;
const seen = new Set();
for (let i = 0; i < (first?.count ?? 0) * 2 + 2; i++) {
  await page.keyboard.press(i <= (first?.count ?? 0) ? "ArrowRight" : "ArrowLeft");
  await page.waitForTimeout(120);
  const s = await selected();
  if (!s?.visible) allVisible = false;
  seen.add(s?.slot);
  // The camera orbits the highlighted car.
  const focus = await page.evaluate((slot) => {
    const w = window.__wp;
    return w.sim.lotVisit()?.cars.find((c) => c.slot === slot)?.entity === w.app.focus;
  }, s?.slot);
  if (!focus) camOk = false;
  if (i === first.count - 2) await shot("lot_visit_scrolled.png");
}
check(allVisible, "the highlighted car stays in view in the list while browsing with the arrow keys");
check(seen.size === first?.count, `browsing visits every car (${seen.size}/${first?.count})`);
check(camOk, "the camera follows the highlighted car");

// The overlay fits the window: the buy button and the list are both on screen, no page scroll.
const fit = async () =>
  page.evaluate((L) => {
    const buy = document.querySelector('[data-testid="lot-buy"]')?.getBoundingClientRect();
    const list = document.querySelector(`${L} [role="listbox"]`)?.getBoundingClientRect();
    const lot = document.querySelector(L).getBoundingClientRect();
    return {
      buy: !!buy && buy.bottom <= innerHeight && buy.top >= 0,
      list: !!list && list.height >= 80,
      inside: lot.right <= innerWidth && lot.bottom <= innerHeight,
    };
  }, L);
const wide = await fit();
check(wide.buy && wide.list && wide.inside, `1366x768: buy button ${wide.buy}, list ${wide.list}, panel inside ${wide.inside}`);

await page.setViewportSize({ width: 390, height: 780 });
await page.waitForTimeout(600);
await shot("lot_visit_phone.png");
const phone = await fit();
check(phone.buy && phone.list && phone.inside, `390x780: buy button ${phone.buy}, list ${phone.list}, panel inside ${phone.inside}`);

// Clicking a card picks it (and keeps it in view).
await page.setViewportSize({ width: 1366, height: 768 });
await page.waitForTimeout(400);
await page.locator(`${L} [data-slot]:not(:disabled)`).nth(2).click();
const clicked = await selected();
check(!!clicked?.visible, "clicking a car selects it");

// Enter asks before buying, N cancels; buying moves the highlight on to the next car for sale.
await page.keyboard.press("ArrowDown");
await page.keyboard.press("Enter");
await page.waitForTimeout(200);
const asked = await page.locator('[data-testid="lot-confirm"]').isVisible().catch(() => false);
await page.keyboard.press("KeyN");
await page.waitForTimeout(200);
const cancelled = !(await page.locator('[data-testid="lot-confirm"]').isVisible().catch(() => false));
check(asked && cancelled, `Enter asks to buy (${asked}), N cancels (${cancelled})`);
const before = await selected();
await page.keyboard.press("Enter");
await page.waitForTimeout(200);
await page.keyboard.press("KeyY");
await page.waitForTimeout(400);
const after = await selected();
const msg = await page.locator('[data-testid="lot-msg"]').innerText().catch(() => "");
check(/Bought/.test(msg) && !!after && after.slot !== before?.slot && after.count === before.count - 1, `buying: "${msg}", highlight moves on (${before?.slot} → ${after?.slot})`);
await shot("lot_visit_bought.png");

// Esc leaves the lot for the garage (it used to open the pause menu instead).
await page.keyboard.press("Escape");
await page.waitForTimeout(500);
const left = await page.evaluate(() => ({ garage: window.__wp.garage.open, menu: !!window.__wp.menu?.isOpen, lot: !!document.querySelector('[data-testid="lot-visit"]') }));
check(left.garage && !left.menu && !left.lot, `Esc: back to the garage ${left.garage}, pause menu ${left.menu}`);

// The other kinds of lot: a used lot and a junkyard, wide shots.
for (const kind of ["UsedLot", "Junkyard"]) {
  const id = await page.evaluate((k) => window.__wp.sim.careerView().lots.find((l) => l.kind === k)?.id, kind);
  await page.evaluate((id) => window.__wp.careerApi.visitLot(id), id);
  await page.waitForSelector(L, { timeout: 20000 });
  await page.waitForTimeout(1200);
  await shot(`lot_${kind}_browse.png`);
  await overview(`lot_${kind}_overview.png`);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);
}

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "lot visit e2e: all passed");
process.exit(failures ? 1 : 0);
