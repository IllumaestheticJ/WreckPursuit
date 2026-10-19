// End-to-end check of the browser testbed in headless Chromium (Playwright).
// Usage: npm run e2e  (needs `npm run wasm` first). Env: CHROMIUM=/path/to/chrome,
// SHOTS=dir to write screenshots, BENCH=1 to measure 1/8/24-car perf.
//
// Checks: the car drives from keyboard input, every debug layer toggles without errors, a
// recorded replay survives a page reload and re-simulates to the same hash with no drift,
// crash scenarios run and report, the city (Port Vesper v2 by default, the old grid via the
// selector) loads with traffic, signals (day/night, lamps change over time) and the city chase,
// and (optionally) perf at 1/8/24 cars.
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
const cams = await page.evaluate(() => window.__wp.cameraModes);
for (const cam of cams) {
  await page.evaluate((c) => window.__wp.setCamera(c), cam);
  await page.waitForTimeout(400);
  if (cam === "orbit") await shot("debug_layers_orbit.png");
  if (cam === "hood") await shot("camera_hood.png");
}
await page.keyboard.up("KeyW");
check(errors.length === 0, `all debug layers (${layerIds.join(", ")} + defaults) and ${cams.length} cameras render without errors`);
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
  if (file === "crash/barrier_frontal.ron") {
    // The repair bill pops up once a scripted scenario ends.
    const auto = await page
      .waitForFunction(() => !document.getElementById("ui-bill").classList.contains("hidden"), null, { timeout: 10000 })
      .then(() => true, () => false);
    check(auto, "repair bill opens automatically when the scenario ends");
  }
  if (file.includes("head_on")) {
    await page.waitForTimeout(300);
    await page.evaluate(() => window.__wp.ui.setBill(false));
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
// Traffic signals: heads at every signalised approach, frame the junction nearest the player
// by day and night, lamps follow the sim and change over time, M toggles the layer.
const frameJunction = () =>
  page.evaluate(() => {
    const heads = window.__wp.sim.signalHeads();
    const st = window.__wp.sim.states();
    const n = heads.length / 10;
    let best = 0;
    let bestD = Infinity;
    for (let i = 0; i < n; i++) {
      const d = Math.hypot(heads[i * 10] - st[1], heads[i * 10 + 1] - st[2]);
      if (d < bestD) [best, bestD] = [i, d];
    }
    const o = best * 10;
    const [x, y, z, yaw, left, right, radius] = heads.slice(o, o + 7);
    const f = [-Math.sin(yaw), Math.cos(yaw)];
    const r = [f[1], -f[0]];
    // Behind and above the approach, looking across the junction at the heads facing us.
    const back = 14;
    const mid = (left + right) / 2;
    window.__wp.lookFrom(
      [x - f[0] * back + r[0] * mid, y - f[1] * back + r[1] * mid, z + 2.6],
      [x + f[0] * radius + r[0] * mid, y + f[1] * radius + r[1] * mid, z + 2.4],
    );
    return { n, head: best };
  });
for (const [hour, label] of [
  [12, "day"],
  [22, "night"],
]) {
  await page.evaluate((h) => {
    document.getElementById("city").checked = true;
    window.__wp.startFreeDrive("debug_sedan", "Arcade", true, h);
  }, hour);
  await page.waitForTimeout(600);
  const framed = await frameJunction();
  await page.waitForTimeout(2500);
  // HUD panels hide the stop line; shoot the scene alone.
  await page.evaluate(() => (document.getElementById("overlay").style.visibility = "hidden"));
  await shot(`signals/junction_${label}.png`);
  await page.evaluate(() => (document.getElementById("overlay").style.visibility = ""));
  const drawn = await page.evaluate(() => ({
    hour: window.__wp.sim.worldInfo()[1],
    drawn: window.__wp.scene.signals.drawnStates(),
    visible: window.__wp.scene.signals.group.visible,
  }));
  const lit = drawn.drawn.filter((c) => c >= 1 && c <= 3).length;
  check(
    framed.n > 8 && drawn.visible && lit === framed.n,
    `signals (${label}, ${drawn.hour.toFixed(1)} h): ${framed.n} heads drawn, ${lit} lit, framed head ${framed.head}`,
  );
}
{
  const before = await page.evaluate(() => window.__wp.scene.signals.drawnStates());
  const changed = await page
    .waitForFunction(
      (b) => {
        const now = window.__wp.scene.signals.drawnStates();
        const sim = Array.from(window.__wp.sim.signalStates());
        return now.some((c, i) => c !== b[i]) && now.every((c, i) => c === sim[i]) ? now.filter((c, i) => c !== b[i]).length : false;
      },
      before,
      // Port Vesper under SwiftShader runs well below real time; a full signal cycle needs
      // minutes of wall time there.
      { timeout: 240000, polling: 500 },
    )
    .then((h) => h.jsonValue())
    .catch(() => 0);
  await page.evaluate(() => (document.getElementById("overlay").style.visibility = "hidden"));
  await shot("signals/junction_night_later.png");
  await page.evaluate(() => (document.getElementById("overlay").style.visibility = ""));
  check(changed > 0, `signal lamps change over time and match the sim (${changed} heads changed)`);
  await page.keyboard.press("Digit9");
  // Software GL frames take a few hundred ms: wait for the next frame to apply the toggle.
  const hidden = await page
    .waitForFunction(() => !window.__wp.scene.signals.group.visible, null, { timeout: 10000 })
    .then(() => true)
    .catch(() => false);
  await page.keyboard.press("Digit9");
  const help = await page.evaluate(() => document.querySelector(".help")?.textContent ?? "");
  check(hidden && help.includes("traffic signals"), "9 hides the signals layer; F1 help lists it");
}
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
// Waits for the state the check needs (no fixed sleep): under SwiftShader the city runs well below
// real time, so a wall-clock wait is either too short (flaky) or wastes minutes. Violations are
// counted per session (`app.eventCounts`), not read from the 12-line feed, which can scroll them out.
await page.evaluate(() => window.__wp.startCityChase("debug_sedan", "Arcade"));
await page.keyboard.down("KeyW");
const chaseReady = () => {
  const wp = window.__wp;
  return (
    wp.sim.worldInfo()[1] >= 23 &&
    (wp.app.eventCounts.TrafficViolation ?? 0) > 0 &&
    wp.sim.policeStatus(wp.app.player) !== null &&
    document.querySelectorAll(".wp-hud-heat").length === 1 &&
    !!document.querySelector("#ui-chase .wp-hud-heat")?.getAttribute("aria-label")
  );
};
const chaseWait = await page
  .waitForFunction(chaseReady, null, { timeout: 240000, polling: 250 })
  .then(() => "ready")
  .catch((e) => `timed out (${String(e).split("\n")[0]})`);
await page.keyboard.up("KeyW");
const chase = await page.evaluate(() => ({
  hour: window.__wp.sim.worldInfo()[1],
  status: window.__wp.sim.policeStatus(window.__wp.app.player),
  panel: document.querySelector("#ui-chase .wp-hud-heat")?.getAttribute("aria-label") ?? null,
  panels: document.querySelectorAll(".wp-hud-heat").length,
  banner: document.getElementById("pursuit-banner").classList.contains("hidden") ? null : document.querySelector("#pursuit-banner b").textContent,
  police: Array.from(window.__wp.sim.vehicleMeta()).filter((_, i, a) => i % 3 === 1 && a[i] === 2).length,
  units: window.__wp.sim.policeUnits().length / 6,
  violations: window.__wp.app.eventCounts.TrafficViolation ?? 0,
  inFeed: document.querySelector(".feed")?.textContent?.includes("TrafficViolation") ?? false,
  simS: window.__wp.sim.worldInfo()[0] / 240,
}));
await shot("city_chase.png");
check(
  chaseWait === "ready" && chase.hour >= 23 && chase.status !== null && chase.panel !== null && chase.panels === 1 && chase.violations > 0,
  `city chase: ${chase.hour.toFixed(2)} h, ${chase.panels} heat panel(s) "${chase.panel}", heat ${chase.status?.heat.toFixed(2)} (${chase.status?.status}), ` +
    `banner ${chase.banner ?? "none"}, ${chase.police} police vehicles + ${chase.units} other units, ` +
    `${chase.violations} violation event(s) (in feed: ${chase.inFeed}) after ${chase.simS.toFixed(1)} s sim; wait ${chaseWait}`,
);
// Pursuit banner + capture → repair bill, driven by a stubbed police_status() (a real capture
// needs minutes of driving; the outcome plumbing is unit-tested in wp_wasm::police).
{
  const flow = await page.evaluate(async () => {
    const sim = window.__wp.sim;
    const real = sim.policeStatus;
    const base = real(window.__wp.app.player);
    const frames = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const read = () => ({
      banner: document.getElementById("pursuit-banner").classList.contains("hidden") ? null : document.querySelector("#pursuit-banner b").textContent,
      bill: !document.getElementById("ui-bill").classList.contains("hidden"),
      units: document.querySelector("#ui-chase .wp-hud-heat")?.getAttribute("aria-label") ?? "",
    });
    const out = {};
    window.__wp.ui.setBill(false);
    const stub = (s) => (sim.policeStatus = () => ({ ...base, last_outcome: null, outcome_age_s: null, ...s }));
    stub({ level: "L2", status: "Pursuit", units_in_pursuit: 3 });
    await frames();
    out.pursuit = read();
    stub({ level: "L2", status: "Searching", cooldown: 0.4, staleness_s: 4, escape_s: 10 });
    await frames();
    out.searching = read();
    stub({ level: "None", status: "Clear", last_outcome: "Escaped", outcome_age_s: 0.5 });
    await frames();
    out.escaped = read();
    out.escapedEnd = window.__wp.ui.lastChaseEnd?.how ?? null;
    // A new chase, ended by a capture (the kit's ChaseEndDetector reads the fresh outcome).
    stub({ level: "L3", status: "Pursuit", units_in_pursuit: 2 });
    await frames();
    stub({ level: "None", status: "Clear", last_outcome: "Captured", outcome_age_s: 0.2 });
    await frames();
    out.captured = read();
    out.capturedEnd = window.__wp.ui.lastChaseEnd;
    stub({ level: "None", status: "Clear", last_outcome: "Captured", outcome_age_s: 30 });
    await frames();
    out.later = read();
    sim.policeStatus = real;
    window.__wp.ui.setBill(false);
    return out;
  });
  check(
    flow.pursuit.banner === "PURSUIT" && flow.searching.banner === "SEARCHING" && flow.escaped.banner === "ESCAPED" && !flow.escaped.bill &&
      flow.captured.banner === "CAPTURED" && flow.captured.bill && flow.later.banner === null &&
      flow.escapedEnd === "escaped" && flow.capturedEnd?.how === "captured" && flow.capturedEnd?.level === "L3",
    `pursuit banner: ${flow.pursuit.banner} → ${flow.searching.banner} → ${flow.escaped.banner} (${flow.escapedEnd}) → ${flow.captured.banner} (bill ${flow.captured.bill ? "opened" : "missing"}, chase end ${flow.capturedEnd?.how} at ${flow.capturedEnd?.level}) → ${flow.later.banner ?? "cleared"}`,
  );
}
// End of run (lead): a real capture in the sim (player parked in front of a patrol car at L2) →
// end-of-run panel + repair bill → "Repair & continue" pays from the ledger and restores the car.
{
  await page.evaluate(() => window.__wp.startCityChase("debug_sedan", "Arcade"));
  const parked = await page
    .waitForFunction(
      () => {
        const wp = window.__wp;
        try {
          wp.sim.debugParkNearPolice(wp.app.player, 14);
          wp.sim.debugSetHeat(wp.app.player, 30);
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
  const ended = await page
    .waitForFunction(() => window.__wp.runEnd.open && !!document.querySelector('#ui-run-end [data-testid="run-end"]'), null, { timeout: 240000, polling: 250 })
    .then(() => true)
    .catch(() => false);
  const before = await page.evaluate(() => {
    const wp = window.__wp;
    const r = wp.runEnd.report;
    return {
      outcome: r?.outcome ?? null,
      fines: r?.fines ?? 0,
      impound: r?.impound_fee ?? null,
      cost: r?.continue_cost ?? 0,
      cashAfter: r?.cash_after ?? 0,
      bill: !document.getElementById("ui-bill").classList.contains("hidden"),
      text: document.querySelector('#ui-run-end [data-testid="run-end"]')?.textContent ?? "",
      held: !wp.sim.setInput(wp.app.player, { ...wp.app.input, throttle: 1 }),
      ledger: wp.sim.runLedger().map((l) => l.reason),
    };
  });
  await shot("run_end_captured.png");
  await page.click('#ui-run-end button.wp-btn--primary').catch(() => {});
  await page.waitForFunction(() => !window.__wp.runEnd.open, null, { timeout: 10000 }).catch(() => {});
  await page.waitForTimeout(300);
  const after = await page.evaluate(() => {
    const wp = window.__wp;
    return {
      open: wp.runEnd.open,
      panel: !!document.querySelector('#ui-run-end [data-testid="run-end"]'),
      bill: !document.getElementById("ui-bill").classList.contains("hidden"),
      health: wp.sim.states()[17],
      cash: wp.sim.runAccount()?.cash ?? null,
      ledger: wp.sim.runLedger().map((l) => l.reason),
    };
  });
  check(
    parked && ended && before.outcome === "Captured" && before.fines > 0 && before.impound > 0 && before.bill && before.held &&
      /Busted/.test(before.text) && /Repair & continue/.test(before.text) && before.ledger.includes("CarImpounded"),
    `capture ends the run: ${before.outcome} (fine $${before.fines}, impound $${before.impound}), result panel + bill ${before.bill ? "open" : "closed"}, input ${before.held ? "held" : "live"}`,
  );
  check(
    !after.open && !after.panel && !after.bill && after.health === 1 && after.cash === before.cashAfter - before.cost &&
      after.ledger.includes("ImpoundRetrieved"),
    `"Repair & continue": health ${after.health}, cash $${before.cashAfter} → $${after.cash} (paid $${before.cost}), panel closed`,
  );
}
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
