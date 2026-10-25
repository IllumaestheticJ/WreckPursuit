// Paint shop and car colour, end to end (headless Chromium): a career car keeps one colour in the
// workshop, on a career run and on a test drive of the same model; the workshop paint shop
// previews a colour, resprays (charging the fee), previews and fits a vinyl.
// Usage: node e2e/paint_shop.mjs (needs `npm run wasm` first). SHOTS=dir writes screenshots.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5212;
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
const settle = (ms = 900) => page.waitForTimeout(ms);
/** Wait until the car is built and the bay placed. */
const placed = () =>
  page.waitForFunction(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return w.workshop.active && v?.panels.ready && w.scene.world.getObjectByName("workshop-room");
  }, null, { timeout: 30000 });

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

const hex = (v) => `#${v.toString(16).padStart(6, "0")}`;
const viewPaint = () => page.evaluate(() => window.__wp.app.views.get(window.__wp.app.player)?.paintHex);
const careerCar = (u) => page.evaluate((x) => window.__wp.sim.careerView().cars.find((c) => c.uid === x), u);

// A dealer car (no lot paint saved) has one colour everywhere.
const uid = await page.evaluate(() => {
  const s = window.__wp.sim;
  let v = s.careerNew("Arcade");
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  v = s.careerBuy(d.def_id, "Worn");
  return v.cars[v.cars.length - 1].uid;
});
const car0 = await careerCar(uid);
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle(1200);
check((await viewPaint()) === car0.paint, `workshop shows the car's colour ${hex(car0.paint)} (view ${hex(await viewPaint())})`);
await page.evaluate(() => window.__wp.careerApi.startRun("free"));
await page.waitForFunction(() => window.__wp.sim.careerRunActive() && window.__wp.app.views.get(window.__wp.app.player)?.panels?.ready, null, { timeout: 60000 });
await settle(800);
check((await viewPaint()) === car0.paint, `career run shows the same colour (view ${hex(await viewPaint())})`);
await page.evaluate(() => window.__wp.openGarage());
await page.evaluate((def) => window.__wp.startFreeDrive(def, "Arcade", false), car0.def_id);
await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.panels?.ready, null, { timeout: 60000 });
await settle(800);
check((await viewPaint()) === car0.paint, `a test drive in the same model shows the same colour (view ${hex(await viewPaint())})`);

// Paint shop: preview a colour (free), then respray.
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle(1000);
await page.locator('[data-testid="ws-paint-shop"]').click();
await page.waitForSelector('[data-testid="paint-shop"]');
const target = 0xff4fa3;
const cash0 = (await screen()).cash;
await page.locator(`[data-paint="${hex(target)}"]`).click();
await settle(300);
check((await viewPaint()) === target && (await screen()).cash === cash0, `a swatch previews on the car for free (view ${hex(await viewPaint())})`);
await shot("paint_preview.png");
await page.locator('[data-testid="ps-respray"]').click();
await settle(300);
const car1 = await careerCar(uid);
check(car1.paint === target && (await screen()).cash === cash0 - car0.respray_fee, `respray saved ${hex(car1.paint)} for ${car0.respray_fee}`);

// Vinyls: preview, then fit.
await page.locator('[data-testid="paint-shop"] [role="tab"]', { hasText: "Vinyls" }).click();
const rows = await page.locator('[data-testid="paint-shop"] .wp-ps-row').evaluateAll((els) => els.map((e) => e.dataset.mod));
const vinyl = rows.includes("vinyl_stripes") ? "vinyl_stripes" : rows[0];
await page.locator(`[data-testid="ps-preview-${vinyl}"]`).click();
await settle(1200);
const looked = await page.evaluate(() => !!window.__wp.app.views.get(window.__wp.app.player).look);
check(looked && !(await careerCar(uid)).mods.some((m) => m.id === vinyl), `preview draws ${vinyl} without buying it`);
await shot("paint_vinyl_preview.png");
await page.locator(`[data-testid="ps-buy-${vinyl}"]`).click();
await placed();
await settle(1200);
check((await careerCar(uid)).mods.some((m) => m.id === vinyl), `${vinyl} fitted`);
check((await viewPaint()) === target, `the car keeps its new colour after the rebuild (view ${hex(await viewPaint())})`);
await shot("paint_vinyl_fitted.png");

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "paint shop e2e: all passed");
process.exit(failures ? 1 : 0);
