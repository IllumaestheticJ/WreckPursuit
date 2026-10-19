// End-to-end check of the browser testbed in headless Chromium (Playwright).
// Usage: npm run e2e  (needs `npm run wasm` first). Env: CHROMIUM=/path/to/chrome,
// SHOTS=dir to write screenshots, BENCH=1 to measure 1/8/24-car perf.
//
// Checks: the car drives from keyboard input, every debug layer toggles without errors, a
// recorded replay survives a page reload and re-simulates to the same hash with no drift,
// crash scenarios run and report, the city (Port Vesper v2 by default, the old grid via the
// selector) loads with traffic and the city chase, and (optionally) perf at 1/8/24 cars.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5199;
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

// 1. Drive.
await page.goto(URL);
await ready();
await page.keyboard.down("KeyW");
await page.keyboard.down("KeyD");
await page.waitForTimeout(2500);
const v = await speed();
await page.keyboard.up("KeyD");
check(v > 1.5, `keyboard drives the car (speed ${v.toFixed(2)} m/s)`);
await shot("drive_chase.png");

// 2. Every debug layer on, every camera.
const layerIds = await page.evaluate(() =>
  ["lattice", "stress", "suspension", "regions", "intents"].map((id) => (window.__wp.toggleLayer(id), id)),
);
for (const cam of ["chase", "hood", "bumper", "orbit", "cinematic"]) {
  await page.evaluate((c) => window.__wp.setCamera(c), cam);
  await page.waitForTimeout(400);
  if (cam === "orbit") await shot("debug_layers_orbit.png");
  if (cam === "hood") await shot("camera_hood.png");
}
await page.keyboard.up("KeyW");
check(errors.length === 0, `all debug layers (${layerIds.join(", ")} + defaults) and 5 cameras render without errors`);
await page.evaluate((ids) => ids.forEach((id) => window.__wp.toggleLayer(id)), layerIds);
await page.evaluate(() => window.__wp.setCamera("chase"));

// 3. Record → reload → replay matches hash; simcli verifies the same file headlessly.
await page.evaluate(() => window.__wp.sim.recordStart());
await page.keyboard.down("KeyW");
await page.keyboard.down("KeyA");
await page.waitForTimeout(1500);
await page.keyboard.up("KeyA");
await page.keyboard.press("ShiftLeft");
// Live mode switch (a logged op): the replay must survive it.
await page.evaluate(() => window.__wp.sim.setDamageMode("Simulation"));
await page.keyboard.down("Space");
await page.waitForTimeout(700);
await page.keyboard.up("Space");
await page.keyboard.up("KeyW");
await page.keyboard.press("KeyR");
await page.waitForTimeout(400);
const recorded = await page.evaluate(() => {
  const hash = window.__wp.sim.stateHash();
  const bytes = window.__wp.stopRecording();
  return { hash, bytes: Array.from(bytes) };
});
check(recorded.bytes.length > 32, `recorded replay.bin (${recorded.bytes.length} bytes, final hash ${recorded.hash})`);
const replayFile = join(tmpdir(), `wp-e2e-${process.pid}.replay.bin`);
writeFileSync(replayFile, Buffer.from(recorded.bytes));
const verify = spawnSync("cargo", ["run", "-q", "--release", "-p", "simcli", "--", "replay", replayFile], {
  cwd: join(root, "../.."),
  encoding: "utf8",
});
check(verify.status === 0, `simcli replay verifies the browser recording: ${(verify.stdout + verify.stderr).trim().split("\n").pop()}`);

await page.goto(URL + "?replay=last");
await ready();
await page.evaluate(() => window.__wp.sim.replayPlay(8));
await page.waitForFunction(() => {
  const s = window.__wp.sim.replayStatus();
  return s && s.tick >= s.end_tick;
}, null, { timeout: 120000 });
const status = await page.evaluate(() => window.__wp.sim.replayStatus());
await shot("replay.png");
check(
  status.drift_count === 0 && status.verified_ticks === status.end_tick && status.state_hash === recorded.hash && status.mode === "Simulation",
  `replay after reload: ${status.verified_ticks}/${status.end_tick} tick hashes, final ${status.state_hash} vs recorded ${recorded.hash}, mode ${status.mode}`,
);
// Scrub back and forward again: still no drift.
await page.evaluate((t) => {
  window.__wp.sim.replaySeek(Math.floor(t / 2));
  window.__wp.sim.replaySeek(t);
}, status.end_tick);
const status2 = await page.evaluate(() => window.__wp.sim.replayStatus());
check(status2.drift_count === 0 && status2.state_hash === recorded.hash, "replay scrub back/forward stays drift-free");
const csv = await page.evaluate(() => window.__wp.sim.replayCsv(window.__wp.app.lastReplay, 1, 4));
check(csv.split("\n").length > 10 && csv.startsWith("tick,t_s,speed_mps"), `CSV export (${csv.split("\n").length - 2} rows)`);

// 4. Scenarios (simcli format v2, results from simcli itself).
for (const [file, kmh] of [
  ["crash/barrier_frontal.ron", 56],
  ["crash/barrier_frontal_sim.ron", 56],
  ["crash/head_on.ron", 50],
  ["smoke/straight_line.ron", 0],
]) {
  const contacts0 = await page.evaluate(() => window.__wp.scene.contactsSeen);
  await page.evaluate(([f, s]) => window.__wp.runScenario(f, s), [file, kmh]);
  if (file === "crash/barrier_frontal.ron") {
    await page.evaluate(() => {
      window.__wp.setCamera("chase");
      window.__wp.toggleLayer("lattice");
    });
    await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 240 * 4.7, null, { timeout: 60000 });
    await shot("scenario_barrier.png");
    await page.evaluate(() => window.__wp.toggleLayer("lattice"));
  }
  await page.waitForFunction(() => window.__wp.sim.scenarioResult()?.done === true, null, { timeout: 120000 });
  const res = await page.evaluate(() => window.__wp.sim.scenarioResult());
  const contacts = (await page.evaluate(() => window.__wp.scene.contactsSeen)) - contacts0;
  const exp = res.summary?.expectations ?? [];
  const pend = res.summary?.pending ?? [];
  check(
    res.pass && res.matches_simcli === true,
    `scenario ${file}: ${exp.filter((r) => r.passed).length}/${exp.length} expectations, ` +
      `${pend.filter((r) => r.passed).length}/${pend.length} pending, live = simcli: ${res.matches_simcli}, contact markers ${contacts}`,
  );
  if (file.startsWith("crash/")) check(contacts > 0, `contact layer drew ${contacts} markers for ${file}`);
  if (file === "crash/barrier_frontal_sim.ron") {
    await page.waitForTimeout(300);
    const feed = await page.evaluate(() => document.querySelector(".feed")?.textContent ?? "");
    const kinds = ["GlassShattered", "RegionDamaged", "ComponentFailed", "PartDetached", "TotalLoss"].filter((k) => feed.includes(k));
    check(kinds.length > 0, `event feed shows damage events: ${kinds.join(", ")}`);
  }
  if (file.includes("head_on")) {
    await shot("scenario_head_on.png");
    // UI-kit overlays (workstream 16): live repair bill (F) for the crashed car + chase readout.
    await page.keyboard.press("KeyF");
    await page.waitForTimeout(400);
    const bill = await page.evaluate(() => ({
      lines: document.querySelectorAll("#ui-bill .wp-bill tbody tr").length,
      total: document.querySelector("#ui-bill .wp-bill-total")?.textContent ?? "",
      live: document.querySelector("#ui-bill .wp-panel-title b")?.textContent ?? "",
      chase: !!document.querySelector("#ui-chase .wp-hud-heat"),
    }));
    check(bill.lines > 0 && /\$[1-9]/.test(bill.total) && bill.live.includes("LIVE") && bill.chase, `repair bill priced live after the crash (${bill.lines} lines, ${bill.total}); chase readout mounted`);
    await shot("repair_bill.png");
    await page.keyboard.press("KeyF");
  }
}

// City free drive: greybox + traffic agents.
await page.evaluate(() => {
  document.getElementById("city").checked = true;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", true);
  window.__wp.setCamera("orbit", [-25, -40, 30]);
});
await page.waitForTimeout(4000);
const agents = await page.evaluate(() => window.__wp.sim.trafficAgents().length / 5);
await shot("city_traffic.png");
check(agents > 10, `city free drive: ${agents} traffic agents rendered`);
// The default city is Port Vesper v2: its greybox loads and the car sits on the terrain.
await page.waitForFunction(() => (window.__wp.scene.cities.get("vesper_v2")?.children.length ?? 0) > 0, null, {
  timeout: 30000,
});
const v2 = await page.evaluate(() => {
  const st = window.__wp.sim.states(); // [entity, x, y, z, …]
  return { id: window.__wp.sim.cityId(), x: st[1], y: st[2], z: st[3] };
});
// Lantern Run starts on Lantern Way west of downtown, ≈ 8–12 m above the sea.
check(
  v2.id === "vesper_v2" && v2.z > 4 && v2.z < 30,
  `city free drive in ${v2.id}: greybox loaded, car at (${v2.x.toFixed(0)}, ${v2.y.toFixed(0)}, ${v2.z.toFixed(1)}) m`,
);
// City chase preset: night city, police + traffic, speeding player; heat panel from the UI kit.
await page.evaluate(() => window.__wp.startCityChase("debug_sedan", "Arcade"));
await page.keyboard.down("KeyW");
// Headless SwiftShader renders the city slowly (the sim keeps real time only when frames are
// fast), so wait for simulated time, not wall time: until the speeding violation shows up or
// 8 s of sim have run.
await page
  .waitForFunction(
    () =>
      (document.querySelector(".feed")?.textContent ?? "").includes("TrafficViolation") ||
      window.__wp.sim.worldInfo()[0] >= 240 * 8,
    null,
    { timeout: 180000, polling: 500 },
  )
  .catch(() => {});
await page.keyboard.up("KeyW");
const chase = await page.evaluate(() => ({
  hour: window.__wp.sim.worldInfo()[1],
  status: window.__wp.sim.policeStatus(window.__wp.app.player),
  panel: document.querySelector(".police-hud .wp-hud-heat")?.getAttribute("aria-label") ?? null,
  police: Array.from(window.__wp.sim.vehicleMeta()).filter((_, i, a) => i % 3 === 1 && a[i] === 2).length,
  units: window.__wp.sim.policeUnits().length / 6,
  violations: document.querySelector(".feed")?.textContent?.includes("TrafficViolation") ?? false,
}));
await shot("city_chase.png");
check(
  chase.hour >= 23 && chase.status !== null && chase.panel !== null && chase.violations,
  `city chase: ${chase.hour.toFixed(2)} h, heat panel "${chase.panel}", heat ${chase.status?.heat.toFixed(2)}, ` +
    `${chase.police} police vehicles + ${chase.units} other units, speeding violations in the feed`,
);
// The selector keeps the old flat grid: the chase restarts there.
await page.evaluate(() => {
  const sel = document.getElementById("city-map");
  sel.value = "procedural_12x12";
  sel.dispatchEvent(new Event("change"));
});
await page.waitForTimeout(1500);
const grid = await page.evaluate(() => ({ id: window.__wp.sim.cityId(), name: window.__wp.sim.scenario().name }));
check(grid.id === "procedural_12x12" && grid.name === "city_chase", `city selector: chase restarted in ${grid.id}`);
await page.evaluate(() => {
  const sel = document.getElementById("city-map");
  sel.value = "vesper_v2";
  document.getElementById("city").checked = false;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", false);
});

// 5. Perf.
if (process.env.BENCH) {
  for (const n of [1, 8, 24]) {
    await page.evaluate((k) => window.__wp.startBench(k), n);
    await page.waitForTimeout(6000);
    const b = await page.evaluate(() => window.__wpBench);
    console.log(
      `BENCH ${n} cars: sim ${b.simMsPer60HzFrame.toFixed(3)} ms per 60 Hz frame (${b.ticks} ticks), ` +
        `render ${b.renderMs.toFixed(3)} ms/frame, ${b.fps.toFixed(1)} fps over ${b.frames} frames`,
    );
    if (n === 24) await shot("bench_24.png");
  }
  // Pure sim cost: 240 steps of 1/60 s (4 ticks each) timed inside the page, no rendering.
  const timeSim = () =>
    page.evaluate(() => {
      const t0 = performance.now();
      for (let i = 0; i < 240; i++) window.__wp.sim.step(1 / 60);
      return (performance.now() - t0) / 240;
    });
  for (const n of [1, 8, 24]) {
    await page.evaluate((k) => window.__wp.startBench(k), n);
    await timeSim(); // warm-up
    console.log(`BENCH sim-only ${n} cars: ${(await timeSim()).toFixed(3)} ms per 60 Hz frame`);
  }
  await page.evaluate(() => window.__wp.startFreeDrive("debug_sedan", "Arcade", true));
  await timeSim();
  const agentsNow = await page.evaluate(() => window.__wp.sim.trafficAgents().length / 5);
  console.log(`BENCH sim-only city + ${agentsNow} traffic agents: ${(await timeSim()).toFixed(3)} ms per 60 Hz frame`);
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
stopServer();
console.log(failures ? `${failures} check(s) failed` : "all checks passed");
process.exit(failures ? 1 : 0);
