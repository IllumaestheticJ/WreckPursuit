// Helicopter-only browser check (the same block runs inside e2e/run.mjs).
// Usage: node e2e/helicopter.mjs  (needs `npm run wasm` first). Env: CHROMIUM, SHOTS=dir.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5196;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
const stopServer = () => server.kill("SIGTERM");
process.on("exit", stopServer);

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(URL);
      if (r.ok) return;
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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1400, height: 860 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

const ready = async () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const speed = () => page.evaluate(() => window.__wp.sim.states()[14]);
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });

await page.goto(URL);
await ready();
// Helicopter (P9): at wanted level 3 a helicopter tracks the racer; at night (the chase is at
// 23:00) it is drawn as an airframe with a spotlight on the car. The real launch takes 120 s at
// night, so the e2e puts one up with the debug hook once the pursuit is on.
{
  await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
  const parked = await page
    .waitForFunction(
      () => {
        const wp = window.__wp;
        try {
          wp.sim.debugParkNearPolice(wp.app.player, 14);
          wp.sim.debugSetHeat(wp.app.player, 60);
          return true;
        } catch {
          return false;
        }
      },
      null,
      { timeout: 120000, polling: 100 },
    )
    .then(() => true)
    .catch(() => false);
  const launched = await page
    .waitForFunction(
      () => {
        const wp = window.__wp;
        try {
          wp.sim.debugLaunchHelicopter(wp.app.player, 120);
          return true;
        } catch {
          return false;
        }
      },
      null,
      { timeout: 120000, polling: 100 },
    )
    .then(() => true)
    .catch(() => false);
  // Drive off so the stop doesn't turn into a capture; the helicopter follows.
  await page.keyboard.down("KeyW");
  const tracking = await page
    .waitForFunction(() => {
      const h = window.__wp.sim.policeHelicopters();
      return h.length >= 8 && (h[7] & 1) === 1 && window.__wp.scene.spotlightsLit > 0;
    }, null, { timeout: 120000, polling: 100 })
    .then(() => true)
    .catch(() => false);
  await page.waitForTimeout(800);
  await page.keyboard.up("KeyW");
  const heli = await page.evaluate(() => {
    const wp = window.__wp;
    const h = wp.sim.policeHelicopters();
    const s = wp.sim.states();
    const player = [s[1], s[2], s[3]];
    return {
      n: h.length / 8,
      visual: (h[7] & 1) === 1,
      alt: h[2] - player[2],
      aimOff: Math.hypot(h[3] - player[0], h[4] - player[1]),
      shown: wp.scene.helicoptersShown,
      lit: wp.scene.spotlightsLit,
      player,
      heli: [h[0], h[1], h[2]],
    };
  });
  await shot("helicopter_spotlight_chase.png");
  if (shots) {
    // Side-on view: the beam from the helicopter down to the car.
    const p = heli.player;
    await page.evaluate(([p, h]) => window.__wp.lookFrom([p[0] - 70, p[1] - 70, p[2] + 45], [(p[0] + h[0]) / 2, (p[1] + h[1]) / 2, (p[2] + h[2]) / 2]), [p, heli.heli]);
    await page.waitForTimeout(400);
    await shot("helicopter_spotlight_beam.png");
  }
  check(
    parked && launched && tracking && heli.n === 1 && heli.visual && heli.shown >= 1 && heli.lit === 1 && heli.alt > 60 && heli.aimOff < 30,
    `helicopter: launched ${launched}, ${heli.n} tracking with visual ${heli.visual}, ${heli.alt.toFixed(0)} m above the car, spotlight ${heli.aimOff.toFixed(1)} m off it, ${heli.lit} lit`,
  );
}
check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
stopServer();
console.log(failures ? `${failures} check(s) failed` : "all checks passed");
process.exit(failures ? 1 : 0);
