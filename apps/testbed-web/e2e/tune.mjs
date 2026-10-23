// End-to-end check of the free car tune (G10) in headless Chromium.
// Usage: node e2e/tune.mjs  (needs `npm run wasm` first). Env: CHROMIUM, SHOTS=dir.
//
// Checks: the garage's Tune page (four 5-step sliders, keyboard-steppable, Reset) saves the
// tune on the career car for free and through a reload; a career run spawns the car with it;
// the developer panel's "car tune" row stores a per-viewer tune that quick runs (free drive,
// races) put on the player entity; stock leaves scenarios untouched.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5203;
const URL = `http://localhost:${PORT}/?dev=1`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(URL)).ok) return;
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
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const ready = async () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const G = '[data-testid="career-garage"]';

await page.goto(URL);
await ready();

// 1. Garage Tune page: buy a car, open Tune, step the sliders with the keyboard.
{
  const fresh = await page.evaluate(() => window.__wp.careerApi.newCareer("Arcade"));
  await page.click("#sc-garage");
  await page.waitForSelector(G, { timeout: 10000 });
  await page.click(`${G} button[role="tab"]:has-text("Dealer")`);
  // Dealer columns: name, class, PI, 0–100, New, Used, Worn (7th) buy buttons, lock badge.
  await page.click(`${G} tr[data-car="junkyard_beater"] td:nth-child(7) button`);
  await page.click(`${G} button[role="tab"]:has-text("Cars")`);
  await page.click(`${G} button:text-is("Tune")`);
  const opened = await page.waitForSelector('[data-testid="tune-page"]', { timeout: 5000 }).then(() => true).catch(() => false);
  const sliders = await page.$$eval('[data-testid="tune-page"] input[type="range"]', (els) => els.map((e) => [e.min, e.max, e.step, e.value].join("/")));
  await shot("tune_stock.png");
  const cashBefore = (await page.evaluate(() => window.__wp.careerApi.view())).cash;

  // Suspension → 5 (two steps right), gearing → 1 (two left), diff → 4, brakes → 2.
  const press = async (key, testid, n) => {
    await page.focus(`[data-testid="${testid}"]`);
    for (let i = 0; i < n; i++) await page.keyboard.press(key);
  };
  await press("ArrowRight", "tune-suspension", 2);
  await press("ArrowLeft", "tune-gearing", 2);
  await press("ArrowRight", "tune-diff", 1);
  await press("ArrowLeft", "tune-brake_balance", 1);
  const tuned = await page.evaluate(() => window.__wp.careerApi.view());
  const summary = await page.textContent('[data-testid="tune-summary"]');
  const diffLabel = await page.textContent('[data-testid="tune-diff-value"]');
  await shot("tune_set.png");
  check(
    opened && sliders.length === 4 && sliders.every((s) => s === "1/5/1/3"),
    `garage: Tune page opens with 4 sliders at stock (${sliders.join(", ")})`,
  );
  const t = tuned.cars[0].tune;
  check(
    t.suspension === 5 && t.gearing === 1 && t.diff === 4 && t.brake_balance === 2 && tuned.cash === cashBefore && fresh.cash > 0,
    `garage: keyboard steps saved ${JSON.stringify(t)} for free (cash $${cashBefore} → $${tuned.cash})`,
  );
  check(summary.includes("Tight LSD") && diffLabel.includes("4 · Tight LSD"), `garage: labels follow (${summary.trim()})`);

  // Survives a reload (career save in localStorage).
  await page.click(`${G} button:has-text("Back")`);
  const badge = await page.isVisible(`${G} [data-testid^="car-"] :text("tuned")`);
  await page.evaluate(() => window.__wp.persistCareer());
  await page.reload();
  await ready();
  const after = await page.evaluate(() => window.__wp.careerApi.view());
  check(badge && JSON.stringify(after.cars[0].tune) === JSON.stringify(t), `garage: "tuned" badge, tune survives a reload`);

  // A career run spawns the car with it.
  await page.click("#sc-garage");
  await page.waitForSelector(G, { timeout: 10000 });
  await page.click(`${G} button:has-text("Free drive")`);
  await page.waitForTimeout(300);
  const run = await page.evaluate(() => ({ active: window.__wp.sim.careerRunActive(), e: window.__wp.sim.scenario().entities[0] }));
  check(run.active && JSON.stringify(run.e.tune) === JSON.stringify(t), `career run: player entity carries the tune (${JSON.stringify(run.e.tune)})`);

  // Reset to stock in the garage: the next run spawns stock (no tune field).
  await page.evaluate(() => window.__wp.openGarage());
  await page.waitForSelector(G, { timeout: 10000 });
  await page.click(`${G} button:text-is("Tune")`);
  await page.click(`[data-testid="tune-page"] button:has-text("Reset to stock")`);
  const reset = await page.evaluate(() => window.__wp.careerApi.view().cars[0].tune);
  await page.click(`${G} button:has-text("Free drive")`);
  await page.waitForTimeout(300);
  const stockRun = await page.evaluate(() => window.__wp.sim.scenario().entities[0]);
  check(
    reset.suspension === 3 && reset.gearing === 3 && reset.diff === 3 && reset.brake_balance === 3 && stockRun.tune === undefined,
    `garage: Reset to stock → next run has no tune field`,
  );
  await page.evaluate(() => window.__wp.openGarage());
  await page.evaluate(() => window.__wp.careerApi.close());
}

// 2. Developer panel row → quick runs (career off).
{
  await page.evaluate(() => {
    const box = document.getElementById("career");
    if (box.checked) box.click();
  });
  const car = await page.$eval("#car", (s) => s.value);
  const rowOk = (await page.$$("#tune-row input[type=range]")).length === 4;
  await page.$eval('#tune-row input[data-tune="gearing"]', (el) => {
    el.value = "5";
    el.dispatchEvent(new Event("input"));
  });
  await page.$eval('#tune-row input[data-tune="diff"]', (el) => {
    el.value = "1";
    el.dispatchEvent(new Event("input"));
  });
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem("wp.tune.v1") ?? "{}"));
  await page.click("#sc-free");
  await page.waitForTimeout(300);
  const free = await page.evaluate(() => window.__wp.sim.scenario().entities[0]);
  check(
    rowOk && stored[car]?.gearing === 5 && stored[car]?.diff === 1 && free.car === car && free.tune?.gearing === 5 && free.tune?.diff === 1,
    `dev row: ${car} tune stored per viewer and used by free drive (${JSON.stringify(free.tune)})`,
  );
  const race = await page.evaluate((c) => {
    const wp = window.__wp;
    const id = wp.sim.raceList(c, "Arcade")[0]?.id;
    if (!id) return null;
    wp.startRace(id, c);
    return { id, e: wp.sim.scenario().entities[0] };
  }, car);
  check(race && race.e.tune?.gearing === 5, `race ${race?.id}: player entity carries the quick tune`);
  await shot("tune_dev_row.png");
  await page.click("#tune-row button:has-text('stock')");
  await page.click("#sc-free");
  await page.waitForTimeout(300);
  const plain = await page.evaluate(() => ({ e: window.__wp.sim.scenario().entities[0], s: localStorage.getItem("wp.tune.v1") }));
  check(plain.e.tune === undefined && plain.s === "{}", "dev row: stock → quick runs untouched, tune forgotten");
}

check(errors.length === 0, `no page errors${errors.length ? `: ${errors.slice(0, 3).join(" | ")}` : ""}`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "all passed");
process.exit(failures ? 1 : 0);
