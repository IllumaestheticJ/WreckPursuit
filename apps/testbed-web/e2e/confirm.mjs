// "Are you sure?" prompts (owner request 2026-10-26). Headless Chromium, Playwright; needs
// `npm run wasm` first. Run alone: `npm run e2e:confirm`. Env: CHROMIUM=/path/to/chrome,
// CONFIRM_SHOTS=dir to write confirm_*.png.
//
// Checks: in a night run, pause → Restart asks first; N, Esc and Enter on the focused Cancel all
// cancel (focus back on Restart, the run keeps going); Y restarts. Pause → Quit asks and the mouse
// Yes quits to the main screen. Free drive restarts without asking. Settings → Reset to defaults
// asks. In the career garage, Sell and Import save ask; Esc cancels the row and keeps the garage.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5199;
const shots = process.env.CONFIRM_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 100) throw new Error("vite did not start");
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
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const frames = (n) => page.evaluate((k) => new Promise((r) => { let i = 0; const f = () => (++i >= k ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }), n);
const tick = () => page.evaluate(() => window.__wp.sim.worldInfo()[0]);
const menu = () =>
  page.evaluate(() => ({ screen: window.__wp.menu.screen, ask: window.__wp.menu.confirming, focus: document.activeElement?.id ?? "", text: document.querySelector('[data-testid="gm-confirm"]')?.textContent ?? "" }));
const shot = (name) => shots && page.screenshot({ path: join(shots, `confirm_${name}.png`) });
const pause = async () => {
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => window.__wp.menu.screen === "pause");
};

// ----- night run: restart -----------------------------------------------------------------
await page.goto(`${URL}?chase=1`);
await ready();
await frames(60);
await pause();
await page.click("#gm-restart");
let m = await menu();
check(m.ask === "restart" && m.focus === "gm-confirm-no" && /night run/i.test(m.text), `Restart asks first, Cancel focused (${m.ask}, ${m.focus}: ${m.text})`);
await shot("restart");
await page.keyboard.press("KeyN");
m = await menu();
check(m.ask === null && m.screen === "pause" && m.focus === "gm-restart", `N cancels back to Restart (${m.screen}, ${m.focus})`);
await page.keyboard.press("Enter"); // Restart is focused: asks again
await page.keyboard.press("Escape");
m = await menu();
check(m.ask === null && m.screen === "pause", `Esc cancels the question, not the pause menu (${m.ask}, ${m.screen})`);
await page.click("#gm-restart");
await page.keyboard.press("Enter"); // Cancel is focused
m = await menu();
check(m.ask === null && m.screen === "pause", `Enter on the focused Cancel cancels (${m.ask}, ${m.screen})`);
const before = await tick();
await page.click("#gm-restart");
await page.keyboard.press("KeyY");
await frames(10);
m = await menu();
const after = await tick();
check(m.screen === "closed" && after < before, `Y restarts the night run (menu ${m.screen}, tick ${before} → ${after})`);

// ----- night run: quit with the mouse ------------------------------------------------------
await frames(30);
await pause();
await page.click("#gm-quit");
m = await menu();
check(m.ask === "quit", `Quit to main menu asks first (${m.ask}: ${m.text})`);
await page.click("#gm-confirm-yes");
m = await menu();
check(m.screen === "main" && m.ask === null, `the mouse Yes quits to the main screen (${m.screen})`);

// ----- settings reset ------------------------------------------------------------------------
await page.click("#gm-settings");
await page.click("#gm-reset");
m = await menu();
check(m.ask === "resetSettings", `Settings → Reset to defaults asks first (${m.ask})`);
await page.click("#gm-confirm-no");
m = await menu();
check(m.screen === "settings" && m.focus === "gm-reset", `Cancel returns to settings (${m.screen}, ${m.focus})`);

// ----- free drive: no question -----------------------------------------------------------
await page.goto(URL);
await ready();
await frames(60);
await pause();
await page.click("#gm-restart");
m = await menu();
check(m.screen === "closed" && m.ask === null, `free drive restarts without asking (${m.screen}, ${m.ask})`);

// ----- career garage: sell, import save -------------------------------------------------------
await page.evaluate(() => {
  const v = window.__wp.careerApi.view();
  if (v.cars.length === 0 && v.floor_car) window.__wp.careerApi.floorCar();
  window.__wp.openGarage();
});
await page.waitForSelector("text=Sell", { timeout: 20000 });
const cars0 = await page.evaluate(() => window.__wp.careerApi.view().cars.length);
await page.click('button:has-text("Sell")');
// The row focuses Cancel and takes its keys in an effect (after the render): wait for it.
await page.waitForFunction(() => document.activeElement?.getAttribute("data-confirm") === "no", null, { timeout: 5000 }).catch(() => {});
const sellAsk = await page.locator('[data-testid="career-confirm"]').count();
const sellFocus = await page.evaluate(() => document.activeElement?.getAttribute("data-confirm"));
check(sellAsk === 1 && sellFocus === "no", `Sell asks first, Cancel focused (${sellAsk}, ${sellFocus})`);
await shot("sell");
await page.keyboard.press("Escape");
const afterEsc = await page.evaluate(() => ({ rows: document.querySelectorAll('[data-testid="career-confirm"]').length, garage: window.__wp.garage.open, menu: window.__wp.menu.screen }));
const cars1 = await page.evaluate(() => window.__wp.careerApi.view().cars.length);
check(afterEsc.rows === 0 && afterEsc.garage && afterEsc.menu === "closed" && cars1 === cars0, `Esc cancels the sell and keeps the garage (${JSON.stringify(afterEsc)}, cars ${cars0} → ${cars1})`);
await page.click('button:has-text("Import save")');
await page.waitForFunction(() => document.activeElement?.getAttribute("data-confirm") === "no", null, { timeout: 5000 }).catch(() => {});
const importAsk = await page.locator('[data-testid="career-confirm"]').textContent();
check(/replaces this career/.test(importAsk ?? ""), `Import save asks first (${importAsk})`);
await page.keyboard.press("KeyN");
check((await page.locator('[data-testid="career-confirm"]').count()) === 0, "N cancels the import");

check(errors.length === 0, `no page errors (${errors.join(" | ")})`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `${failures} FAILED` : "all passed");
process.exit(failures ? 1 : 0);
