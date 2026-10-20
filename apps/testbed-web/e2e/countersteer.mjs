// Countersteer slider (owner row 57): the assist-row slider sets the player's
// countersteer_help through wp_wasm, is remembered in wp.assistOverrides, and survives a
// reload. `node e2e/countersteer.mjs`
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5197;
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
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
const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const help = () => page.evaluate(() => window.__wp.sim.assists(window.__wp.app.player).countersteer_help);
await page.goto(URL);
await ready();
check((await page.locator("#cstr").count()) === 1, "slider in the assist row");
const initial = await help();
check(Math.abs(Number(await page.inputValue("#cstr")) - Math.round(initial * 100)) < 1e-6, `slider shows the preset (${initial})`);
await page.locator("#cstr").fill("85");
await page.locator("#cstr").dispatchEvent("input");
check(Math.abs((await help()) - 0.85) < 1e-6, `sim countersteer_help = ${await help()}`);
check((await page.textContent("#cstr-val")).trim() === "85%", "label shows 85%");
const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("wp.assistOverrides") ?? "{}"));
check(saved.countersteer_help === 0.85, `saved in wp.assistOverrides (${JSON.stringify(saved)})`);
check(saved.tcs === undefined || typeof saved.tcs === "boolean", "checkbox overrides untouched");
await page.reload();
await ready();
check(Math.abs((await help()) - 0.85) < 1e-6, `kept after a reload (${await help()})`);
check(Number(await page.inputValue("#cstr")) === 85, "slider restored after a reload");
check(errors.length === 0, `no page errors ${errors.join(" | ")}`);
await browser.close();
console.log(failures ? `${failures} countersteer check(s) failed` : "all countersteer checks passed");
process.exit(failures ? 1 : 0);
