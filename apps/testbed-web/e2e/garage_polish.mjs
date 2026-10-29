// Workshop polish (owner 2026-10-29), end to end (headless Chromium):
// - the parts catalog stays open after "Fit for me" and after "Buy", and says what happened;
// - what the player already has is greyed out with its buy buttons disabled: a grade on the car
//   ("on the car"), a grade in the inventory ("owned": no Buy, the mechanic fits that one), a
//   bolt-on installed (Remove stays enabled);
// - garage sounds fire for buying, the mechanic, bolt-ons, and the room tone runs while open.
// Usage: node e2e/garage_polish.mjs (needs `npm run wasm` first). SHOTS=dir writes screenshots.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = Number(process.env.PORT ?? 5214);
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
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist", "--autoplay-policy=no-user-gesture-required"],
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
const catalog = () => page.locator('[data-testid="ws-catalog"]');
const catRow = (grade) => catalog().locator(`[data-testid="ws-catrow"][data-grade="${grade}"]`).first();
const sfx = () => page.evaluate(() => window.__wpAudio?.garageStats?.() ?? null);
const visible = (l) => l.isVisible().catch(() => false);

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

// The radiator's card, then its full catalog (the first click also starts the AudioContext).
const radiatorKind = (await screen()).catalog.find((c) => c.grade === "cooling_radiator_1")?.kind;
const radiator = (await screen()).slots.find((s) => s.kind === radiatorKind)?.id;
await button("Part list").click();
await page.locator(`[data-testid="ws-slotrow"][data-slot="${radiator}"]`).click();
await button("Part list").click();
await settle(300);
await button("Full catalog").click();
await settle(300);
check(await visible(catalog()), "the catalog opens for the radiator");
check((await catRow("cooling_radiator_1").getAttribute("data-owned").catch(() => "")) === "", "an upgrade the player does not have is not marked owned");
await shot("1_catalog_open.png");

// 1. Fit for me: the window stays open, the row shows the part on the car, greyed.
const cash0 = (await screen()).cash;
await catRow("cooling_radiator_1").locator('[data-testid="ws-cat-mechanic"]').click();
await placed();
await settle(1200);
let s = await screen();
check(s.slots.find((x) => x.id === radiator)?.fitted?.grade === "cooling_radiator_1" && s.cash < cash0, "Fit for me fitted the radiator and charged for it");
check(await visible(catalog()), "the catalog stays open after Fit for me");
const fitted = catRow("cooling_radiator_1");
check((await fitted.getAttribute("data-owned").catch(() => null)) === "fitted", "the fitted grade is marked on the car");
check(/on the car/.test((await fitted.locator('[data-testid="ws-owned"]').textContent().catch(() => "")) ?? ""), "with an 'on the car' tag");
check(
  (await fitted.locator('[data-testid="ws-up-buy"]').isDisabled().catch(() => false)) &&
    (await fitted.locator('[data-testid="ws-cat-mechanic"]').isDisabled().catch(() => false)),
  "and its Buy / Fit for me buttons are disabled",
);
const note = await catalog().locator('[data-testid="ws-cat-notice"]').textContent().catch(() => "");
check(/Mechanic fitted/.test(note ?? ""), `the window says what the mechanic did (${note})`);
await shot("2_after_fit_for_me.png");

// 2. Buy another grade: the window stays open, the row says it is owned (in the inventory).
const other = s.catalog.find((c) => c.kind === radiatorKind && c.grade && c.grade !== "cooling_radiator_1" && !c.reason && c.price + c.install_fee < s.cash);
if (other) {
  await catRow(other.grade).locator('[data-testid="ws-up-buy"]').click();
  await settle(600);
  check(await visible(catalog()), `the catalog stays open after Buy (${other.grade})`);
  const r = catRow(other.grade);
  check((await r.getAttribute("data-owned").catch(() => null)) === "inventory", "the bought grade is marked owned");
  check(await r.locator('[data-testid="ws-up-buy"]').isDisabled().catch(() => false), "its Buy is disabled");
  const mine = r.locator('[data-testid="ws-cat-mechanic"]');
  check(/Fit mine for me/.test((await mine.textContent().catch(() => "")) ?? "") && (await mine.isEnabled().catch(() => false)), "and the mechanic can fit the one in the inventory");
  await shot("3_after_buy.png");
  const cash1 = (await screen()).cash;
  await mine.click().catch(() => {});
  await placed();
  await settle(1200);
  s = await screen();
  check(s.slots.find((x) => x.id === radiator)?.fitted?.grade === other.grade, "Fit mine for me fitted the inventory part");
  check(cash1 - s.cash <= other.install_fee, `labour only ($${cash1 - s.cash})`);
  check((await catRow("cooling_radiator_1").getAttribute("data-owned").catch(() => null)) === "inventory", "the radiator taken off is now owned, in the inventory");
} else {
  check(false, "a second radiator grade is for sale");
}

// 3. A bolt-on: installed = greyed, Remove still works.
await catalog().locator('[role="tab"]', { hasText: "Electronics" }).first().click();
await settle(200);
const radar = catRow("elec_radar_1");
await radar.locator('[data-testid="ws-up-install"]').click().catch(() => {});
await settle(600);
check(await visible(catalog()), "the catalog stays open after Install");
check((await radar.getAttribute("data-owned").catch(() => null)) === "installed", "the installed bolt-on is marked on the car");
const remove = radar.locator('[data-testid="ws-up-remove"]');
check(await remove.isEnabled().catch(() => false), "its Remove button stays enabled");
await shot("4_bolt_on_installed.png");
await remove.click().catch(() => {});
if (await visible(radar.locator('[data-testid="ws-confirm"]'))) await radar.locator('[data-testid="ws-confirm"] [data-confirm="yes"]').click();
await settle(600);
check((await radar.getAttribute("data-owned").catch(() => null)) === "", "removed: for sale again");

// 4. Garage sounds.
const st = await sfx();
check(!!st, "the audio system reports garage sounds");
if (st) {
  const r = st.requested;
  check(r.buy >= 1 && r.fit >= 1 && r.install >= 1 && r.remove >= 1, `buy / fit / install / remove sounds asked for (${JSON.stringify(r)})`);
  const running = await page.evaluate(() => window.__wpAudio.running());
  if (running) {
    check(st.played.fit >= 1 && st.played.buy >= 1, `and played (${JSON.stringify(st.played)})`);
    check(st.ambience && st.uiOnly, "the workshop room tone plays, the game buses are shut");
  } else {
    console.log("note: AudioContext not running in this browser; played counts not checked");
  }
}
await page.keyboard.press("Escape");
await settle(200);
check(!(await visible(catalog())), "Esc closes the catalog");
await page.keyboard.press("Escape");
await settle(800);
const after = await sfx();
check(!!after && !after.ambience, "leaving the workshop stops the room tone");

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "garage polish e2e: all passed");
process.exit(failures ? 1 : 0);
