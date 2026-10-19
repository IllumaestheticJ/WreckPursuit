// Headless-Chromium smoke test for the demo page.
//
//   npm run e2e            (builds the demo, serves it with `vite preview`, visits every screen)
//
// For each screen: no console errors / page errors, the screen root renders, a screenshot lands
// in e2e/out/<screen>.png. Also exercises a few interactions (rebinding dialog, repair, mod
// install, captain drag-assign, replay seek). Uses the pre-installed Chromium when present.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const pkg = resolve(here, "..");
const out = join(here, "out");
mkdirSync(out, { recursive: true });

const PORT = 5181;
const BASE = `http://localhost:${PORT}`;

function run(cmd, args, opts = {}) {
  return new Promise((ok, fail) => {
    const p = spawn(cmd, args, { cwd: pkg, stdio: "inherit", ...opts });
    p.on("exit", (code) => (code === 0 ? ok() : fail(new Error(`${cmd} ${args.join(" ")} → ${code}`))));
  });
}

await run("npx", ["vite", "build", "--logLevel", "warn"]);
// Own process group, so the whole npx → vite tree can be stopped at the end.
const server = spawn("npx", ["vite", "preview", "--port", String(PORT), "--strictPort"], { cwd: pkg, stdio: "pipe", detached: true });
const stopServer = () => {
  try {
    process.kill(-server.pid, "SIGTERM");
  } catch {}
};
await new Promise((ok) => {
  server.stdout.on("data", (d) => String(d).includes(String(PORT)) && ok());
  setTimeout(ok, 5000);
});

const executablePath = ["/opt/pw-browsers/chromium", process.env.CHROMIUM_PATH].find((p) => p && existsSync(p));
const browser = await chromium.launch({ executablePath, args: ["--no-sandbox"] });
const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(`console: ${m.text()}`));
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));

const SCREENS = ["hud", "replay", "mainMenu", "pause", "settings", "garage", "modTree", "market", "lobby", "results", "impound", "crewMap", "police", "captain", "scanner"];
let failures = 0;
const check = (cond, msg) => {
  if (!cond) {
    failures++;
    console.error(`FAIL ${msg}`);
  } else console.log(`ok   ${msg}`);
};

for (const s of SCREENS) {
  errors.length = 0;
  await page.goto(`${BASE}/?screen=${s}&t=62&paused=1&chrome=0`);
  await page.waitForSelector(`.wp-root[data-screen="${s}"]`, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(300);
  const rendered = await page.locator(`.wp-root[data-screen="${s}"]`).count();
  check(rendered === 1, `${s}: renders`);
  check(errors.length === 0, `${s}: no errors ${errors.join(" | ")}`);
  await page.screenshot({ path: join(out, `${s}.png`) });
}

// HUD while playing: speed changes over time.
await page.goto(`${BASE}/?screen=hud&chrome=0`);
await page.waitForTimeout(1500);
const sp1 = await page.locator(".wp-hud-speed").textContent();
await page.waitForTimeout(1000);
const sp2 = await page.locator(".wp-hud-speed").textContent();
check(sp1 !== sp2, `hud: speed updates while playing (${sp1} → ${sp2})`);

// Palettes / scale render without errors.
for (const p of ["deuteranopia", "highContrast"]) {
  errors.length = 0;
  await page.goto(`${BASE}/?screen=garage&palette=${p}&scale=1.25&chrome=0&paused=1`);
  await page.waitForTimeout(300);
  check(errors.length === 0, `garage @ ${p} 125%: no errors`);
  await page.screenshot({ path: join(out, `garage-${p}.png`) });
}

// Settings: open the rebinding dialog, press a key, confirm the swap.
await page.goto(`${BASE}/?screen=settings&chrome=0&paused=1`);
await page.getByRole("button", { name: "Rebind Horn on Keyboard" }).click();
await page.waitForSelector(".wp-capture-listen");
await page.waitForTimeout(100); // listener attaches in an effect after paint
await page.keyboard.press("KeyW");
await page.waitForSelector("text=Already used by");
await page.screenshot({ path: join(out, "settings-rebind.png") });
await page.getByRole("button", { name: "Swap" }).click();
const horn = await page.getByRole("button", { name: "Rebind Horn on Keyboard" }).textContent();
check(horn === "W", `settings: horn rebound to W (got ${horn})`);

// Garage: pay for a repair.
await page.goto(`${BASE}/?screen=garage&chrome=0&paused=1`);
const cashBefore = await page.evaluate(() => window.wpModel.get().account.cash);
await page.getByRole("button", { name: /^Pay \$/ }).click();
const cashAfter = await page.evaluate(() => window.wpModel.get().account.cash);
check(cashAfter < cashBefore, `garage: repair charged (${cashBefore} → ${cashAfter})`);

// Mod tree: install a tier-1 mod.
await page.goto(`${BASE}/?screen=modTree&chrome=0&paused=1`);
await page.locator(".wp-mod-node", { hasText: "Ported Intake Manifold" }).click();
await page.screenshot({ path: join(out, "modTree-selected.png") });
await page.getByRole("button", { name: /^Install/ }).click();
const mods = await page.evaluate(() => window.wpModel.get().garage.cars.find((c) => c.uid === 1).mods);
check(mods.includes("engine_intake_2"), `modTree: installed engine_intake_2 (${mods.join(",")})`);

// Captain (synthetic run, toy police, pursuit at L4): order a unit from the side panel, open the
// radial on a road, place spikes there.
await page.goto(`${BASE}/?screen=captain&t=62&chrome=0&paused=1`);
await page.waitForTimeout(400);
await page.locator(".wp-unit-row").nth(1).click();
const unitId = await page.evaluate(() => window.wpModel.get().captain.units[1].id);
await page.getByRole("button", { name: /^Pursue / }).click();
const assignment = await page.evaluate((id) => window.wpModel.get().captain.units.find((u) => u.id === id).assignment, unitId);
check(assignment === "pursue", `captain: unit ordered to pursue (${assignment})`);
// Right-click on a road near the racer: project the racer's position like the map does.
const box = await page.locator(".wp-captain-canvas").boundingBox();
await page.locator(".wp-suspect-row").first().getByRole("button", { name: "★" }).click();
await page.getByRole("button", { name: "Follow" }).click();
await page.waitForTimeout(300);
await page.mouse.click(box.x + box.width / 2 + 4, box.y + box.height / 2, { button: "right" });
await page.waitForSelector(".wp-radial", { state: "attached" });
await page.screenshot({ path: join(out, "captain-radial.png") });
const spikesBefore = await page.evaluate(() => window.wpModel.get().captain.readiness.spikes[0]);
const item = page.locator(".wp-radial-item", { hasText: /^Spikes/ });
await item.click();
const spikesAfter = await page.evaluate(() => window.wpModel.get().captain.readiness.spikes[0]);
check(spikesAfter === spikesBefore + 1, `captain: spikes placed from the radial (${spikesBefore} → ${spikesAfter})`);

// Recorded wp_police run on the captain map.
errors.length = 0;
await page.goto(`${BASE}/?screen=captain&run=police_pursuit&t=135&chrome=0&paused=1`);
await page.waitForTimeout(500);
const src = await page.evaluate(() => window.wpModel.get().captain.source);
check(src === "wp_police" && errors.length === 0, `captain: recorded wp_police run renders (${src}) ${errors.join(" | ")}`);
await page.screenshot({ path: join(out, "captain-wp_police.png") });
await page.goto(`${BASE}/?screen=hud&run=police_pursuit&t=135&chrome=0&paused=1`);
await page.waitForTimeout(300);
await page.screenshot({ path: join(out, "hud-wp_police.png") });

// Replay: seek via the slider and check the tick moved.
await page.goto(`${BASE}/?screen=replay&chrome=0&paused=1`);
await page.locator(".wp-replay-timeline input").fill("12000");
const tick = await page.evaluate(() => window.wpModel.get().replay.tick);
check(tick === 12000, `replay: seek to 12000 (got ${tick})`);

// Perf probe: frame pacing and long tasks while playing (HUD and captain map redraw every frame).
for (const s of ["hud", "captain", "mainMenu"]) {
  await page.goto(`${BASE}/?screen=${s}&chrome=0&t=40`);
  await page.waitForTimeout(500);
  const stats = await page.evaluate(
    () =>
      new Promise((done) => {
        const deltas = [];
        let longTasks = 0;
        let longMs = 0;
        const obs = new PerformanceObserver((l) => {
          for (const e of l.getEntries()) {
            longTasks++;
            longMs += e.duration;
          }
        });
        try {
          obs.observe({ entryTypes: ["longtask"] });
        } catch {}
        let last = performance.now();
        const t0 = last;
        const frame = (t) => {
          deltas.push(t - last);
          last = t;
          if (t - t0 < 3000) requestAnimationFrame(frame);
          else {
            obs.disconnect();
            deltas.sort((a, b) => a - b);
            done({ frames: deltas.length, p50: deltas[Math.floor(deltas.length * 0.5)], p95: deltas[Math.floor(deltas.length * 0.95)], longTasks, longMs });
          }
        };
        requestAnimationFrame(frame);
      }),
  );
  console.log(`perf ${s}: ${stats.frames} frames/3s, frame p50 ${stats.p50.toFixed(1)} ms, p95 ${stats.p95.toFixed(1)} ms, long tasks ${stats.longTasks} (${stats.longMs.toFixed(0)} ms)`);
}

await browser.close();
stopServer();
if (failures) {
  console.error(`${failures} failure(s); screenshots in e2e/out/`);
  process.exit(1);
}
console.log(`all good; screenshots in e2e/out/`);
process.exit(0);
