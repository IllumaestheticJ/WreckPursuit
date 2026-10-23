// Turn signals (owner playtest 2026-10-21, "traffic doesn't use turn signals"), headless Chromium
// (Playwright). `npm run e2e:signals` (needs `npm run wasm` first). Env: CHROMIUM=/path/to/chrome.
//
// Checks, in the night run preset with the player parked: some traffic car's draw record asks
// for its right indicator, and its right-side lamps are lit in one frame and unlit in another
// (the 1.5 Hz blink) while its left-side lamps stay dark; the same for a left signal; hazards
// light both sides. Lamp state is read back from the drawn car (fleet instance colours or the
// promoted car's VehicleView) via `window.__wp.indicators`. The player's cosmetic stalk (`=`)
// lights the player's right lamps and a second press cancels it.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5197;
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
const context = await browser.newContext({ viewport: { width: 1200, height: 760 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
await page.evaluate(() => localStorage.removeItem("wp.career.v1"));
await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 240, null, { timeout: 60000 });
await page.evaluate(() => window.__wp.indicators.probe(true));

// Per traffic car and side: frames where the record asked for the side with the lamps lit /
// unlit, and frames where the other side was (wrongly) lit.
const LEFT = 1;
const RIGHT = 2;
const cars = new Map();
let samples = 0;
let hazardBoth = 0;
let hazardSamples = 0;
const t0 = Date.now();
const done = () => {
  const ok = (side) => [...cars.values()].some((c) => c[side].lit > 0 && c[side].dark > 0 && c[side].wrong === 0);
  return ok("right") && ok("left");
};
while (Date.now() - t0 < 90000 && !(done() && samples > 40)) {
  const rows = await page.evaluate(() => window.__wp.indicators.rows());
  samples++;
  for (const r of rows) {
    if (r.mode === "none" || r.mode === "culled") continue;
    const hazard = (r.flags & 4) !== 0;
    if (hazard) {
      hazardSamples++;
      if (r.left === r.right) hazardBoth++;
      continue;
    }
    let c = cars.get(r.id);
    if (!c) cars.set(r.id, (c = { left: { lit: 0, dark: 0, wrong: 0 }, right: { lit: 0, dark: 0, wrong: 0 }, modes: new Set() }));
    c.modes.add(r.mode);
    for (const [side, bit, lit, other] of [
      ["right", RIGHT, r.right, r.left],
      ["left", LEFT, r.left, r.right],
    ]) {
      if (r.request === bit) {
        if (lit) c[side].lit++;
        else c[side].dark++;
        if (other) c[side].wrong++;
      } else if (r.request === 0 && lit) {
        // No signal asked for: never lit.
        c[side].wrong++;
      }
    }
  }
  await page.waitForTimeout(130);
}
const summary = (side) => {
  const good = [...cars.entries()].filter(([, c]) => c[side].lit > 0 && c[side].dark > 0 && c[side].wrong === 0);
  const asked = [...cars.values()].filter((c) => c[side].lit + c[side].dark > 0).length;
  const wrong = [...cars.values()].filter((c) => c[side].wrong > 0).length;
  return { good: good.length, asked, wrong, modes: [...new Set(good.flatMap(([, c]) => [...c.modes]))] };
};
const right = summary("right");
const left = summary("left");
console.log(`samples ${samples}, cars ${cars.size}, right ${JSON.stringify(right)}, left ${JSON.stringify(left)}, hazards ${hazardBoth}/${hazardSamples}`);
check(right.good > 0, `a car signalling right blinks its right lamps (lit and unlit frames), left dark (${right.good} of ${right.asked} cars, drawn as ${right.modes.join("/")})`);
check(left.good > 0, `a car signalling left blinks its left lamps, right dark (${left.good} of ${left.asked} cars)`);
check(right.wrong === 0 && left.wrong === 0, `no car lit the wrong side or lit lamps without a signal (right ${right.wrong}, left ${left.wrong})`);
check(hazardBoth === hazardSamples, `hazards light both sides together (${hazardBoth}/${hazardSamples} frames)`);

// Player's cosmetic stalk: `=` right on (blinking), `=` again off.
await page.evaluate(() => window.__wp.indicators.probe(false));
await page.keyboard.press("Equal");
let litFrames = 0;
let darkFrames = 0;
let leftLit = 0;
// The panel mesh's own indicator lenses (render/panels.ts `indicator_*` parts) blink with the
// lamps: same frames lit, left side dark.
let lens = null;
let lensAgree = 0;
let lensLeft = 0;
for (let i = 0; i < 30; i++) {
  const [p, l] = await page.evaluate(() => [window.__wp.indicators.player(), window.__wp.indicators.playerLenses()]);
  if (p?.right) litFrames++;
  else darkFrames++;
  if (p?.left) leftLit++;
  if (l) lens = l;
  if (l && l.right === !!p?.right) lensAgree++;
  if (l?.left) lensLeft++;
  await page.waitForTimeout(110);
}
const stalkDir = await page.evaluate(() => window.__wp.indicators.stalk.dir);
check(stalkDir === 1 && litFrames > 0 && darkFrames > 0 && leftLit === 0, `player's right indicator blinks after "=" (lit ${litFrames}, dark ${darkFrames}, left lit ${leftLit})`);
if (lens && lens.count > 0)
  check(lensAgree === 30 && lensLeft === 0, `the player's ${lens.count} mesh indicator lenses blink with it (${lensAgree}/30 frames agree, left lit ${lensLeft}; ${lens.names.join(", ")})`);
else console.log(`note  the player's car has no mesh indicator lenses (box markers only)`);
await page.keyboard.press("Equal");
await page.waitForTimeout(300);
const off = await page.evaluate(() => ({ dir: window.__wp.indicators.stalk.dir, lit: window.__wp.indicators.player() }));
check(off.dir === 0 && !off.lit?.left && !off.lit?.right, `a second "=" cancels it (${JSON.stringify(off)})`);

check(errors.length === 0, `no page errors (${errors.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} check(s) FAILED` : "all signal checks passed");
process.exit(failures ? 1 : 0);
