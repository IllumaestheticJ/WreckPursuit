// End-to-end check of the browser testbed in headless Chromium (Playwright).
// Usage: npm run e2e  (needs `npm run wasm` first). Env: CHROMIUM=/path/to/chrome,
// SHOTS=dir to write screenshots, BENCH=1 to measure 1/8/24-car perf.
//
// Checks: the car drives from keyboard input, every debug layer toggles without errors, a
// recorded replay survives a page reload and re-simulates to the same hash with no drift,
// crash scenarios run and report, the city (Port Vesper v2 by default, the old grid via the
// selector) loads with traffic, signals (day/night, lamps change over time) and the night run,
// weather (e2e/weather.mjs: selector, rain, wet roads, spray, fog, wipers, rainy-night chase),
// traffic / police LOD hand-overs are seamless (no missing mesh, < 0.5 m swap jump, no pop-in
// within 300 m), gamepad + rebindable controls (e2e/gamepad.mjs), touch controls
// (e2e/touch.mjs), browser perf budgets
// (e2e/perf_check.mjs, PERF=0 skips), parked cars and cones are drawn (every prop within 150 m
// has exactly one drawn instance, near parked cars use the full mesh, a knocked parked car's drawn
// pose follows the sim within 0.2 m), crash effects (e2e/crash_fx.mjs: sparks, glass, debris,
// smoke, skid marks, no steady-state allocation), instant crash replays (e2e/crash_replay.mjs),
// street racers (e2e/street.mjs) and traffic colliders (e2e/traffic_bodies.mjs), and
// (optionally) perf at 1/8/24 cars.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5199;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) for (const d of ["", "signals", "tunnels"]) mkdirSync(join(shots, d), { recursive: true });

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
  executablePath: chromiumPath(),
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

// 1b. Car stat bars under the picker follow the selected car (physics-measured table).
const statBars = await page.evaluate(() => {
  const el = document.getElementById("car-stats");
  const widths = () => Array.from(el.querySelectorAll(".car-stat-track i")).map((i) => parseFloat(i.style.width));
  return { visible: !el.classList.contains("hidden"), car: el.dataset.car, picked: document.getElementById("car").value, widths: widths() };
});
check(
  statBars.visible && statBars.car === statBars.picked && statBars.widths.length === 3 && statBars.widths.every((w) => w > 0 && w <= 100),
  `car picker shows acceleration / top speed / cornering bars for ${statBars.car} (${statBars.widths.join(" / ")} %)`,
);

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
const agents = await page.evaluate(() => window.__wp.sim.trafficAgents().length / 7);
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
  // Only heads near the camera are drawn (SIGNAL_DRAW_M, browser perf pass); 255 = not drawn.
  const shown = drawn.drawn.filter((c) => c !== 255).length;
  const lit = drawn.drawn.filter((c) => c >= 1 && c <= 3).length;
  check(
    shown > 8 && drawn.visible && lit === shown && drawn.drawn[framed.head] !== 255,
    `signals (${label}, ${drawn.hour.toFixed(1)} h): ${shown} of ${framed.n} heads drawn (near the camera), ${lit} lit, framed head ${framed.head}`,
  );
}
{
  const before = await page.evaluate(() => window.__wp.scene.signals.drawnStates());
  const changed = await page
    .waitForFunction(
      (b) => {
        const now = window.__wp.scene.signals.drawnStates();
        const sim = Array.from(window.__wp.sim.signalStates());
        const drawn = (c, i) => c !== 255 && b[i] !== 255;
        return now.some((c, i) => drawn(c, i) && c !== b[i]) && now.every((c, i) => c === 255 || c === sim[i])
          ? now.filter((c, i) => drawn(c, i) && c !== b[i]).length
          : false;
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
// `?city=<id>` opens free drive in that city with its greybox (it used to only pick the selector
// entry, so the link opened on the flat world). Separate page: the main one keeps its session.
{
  const p2 = await context.newPage();
  p2.on("pageerror", (e) => errors.push(String(e)));
  await p2.goto(`${URL}?city=procedural_12x12`);
  await p2.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
  const meshes = await p2
    .waitForFunction(
      () => {
        let n = 0;
        window.__wp.scene.cities.get("procedural_12x12")?.traverse((o) => o.isMesh && n++);
        return n;
      },
      null,
      { timeout: 30000 },
    )
    .then((h) => h.jsonValue())
    .catch(() => 0);
  const id = await p2.evaluate(() => window.__wp.sim.cityId());
  check(id === "procedural_12x12" && meshes > 0, `?city=procedural_12x12 opens free drive in ${id || "(flat world)"}, ${meshes} greybox meshes`);
  await p2.close();
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
// Tunnels (backlog B04): the Hollins Tunnel and Founders Cut are in the greybox — walls, roof
// deck and emissive ceiling lights — and draw from the portal and inside the tube.
{
  const tunnel = await page.evaluate(() => {
    const out = { tunnel_wall: 0, tunnel_roof: 0, tunnel_light: 0, emissive: 0 };
    window.__wp.scene.cities.get("vesper_v2").traverse((o) => {
      const name = o.material?.name;
      if (name in out) {
        out[name] += o.geometry.attributes.position.count;
        if (name === "tunnel_light") out.emissive = Math.max(out.emissive, o.material.emissive?.r ?? 0);
      }
    });
    return out;
  });
  check(
    tunnel.tunnel_wall > 100 && tunnel.tunnel_roof > 100 && tunnel.tunnel_light > 100 && tunnel.emissive > 0.5,
    `tunnels in the greybox: ${tunnel.tunnel_wall} wall, ${tunnel.tunnel_roof} roof, ${tunnel.tunnel_light} light vertices (lights emissive ${tunnel.emissive.toFixed(2)})`,
  );
  await page.evaluate(() => (document.getElementById("overlay").style.visibility = "hidden"));
  for (const [name, from, to] of [
    ["tunnels/founders_cut_north_portal.png", [409, 1335, 27], [416, 1245, 18]],
    ["tunnels/hollins_tunnel_inside.png", [-850, 1388, 51.5], [-1000, 1396, 56]],
  ]) {
    await page.evaluate(([f, t]) => window.__wp.lookFrom(f, t), [from, to]);
    await page.waitForTimeout(1500);
    await shot(name);
  }
  await page.evaluate(() => (document.getElementById("overlay").style.visibility = ""));
}
// Night run preset: night city, police + traffic, speeding player; heat panel from the UI kit.
// Waits for the state the check needs (no fixed sleep): under SwiftShader the city runs well below
// real time, so a wall-clock wait is either too short (flaky) or wastes minutes. Violations are
// counted per session (`app.eventCounts`), not read from the 12-line feed, which can scroll them out.
await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
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
  units: window.__wp.sim.policeUnits().length / 8,
  violations: window.__wp.app.eventCounts.TrafficViolation ?? 0,
  inFeed: document.querySelector(".feed")?.textContent?.includes("TrafficViolation") ?? false,
  simS: window.__wp.sim.worldInfo()[0] / 240,
}));
await shot("night_run.png");
check(
  chaseWait === "ready" && chase.hour >= 23 && chase.status !== null && chase.panel !== null && chase.panels === 1 && chase.violations > 0,
  `night run: ${chase.hour.toFixed(2)} h, ${chase.panels} heat panel(s) "${chase.panel}", heat ${chase.status?.heat.toFixed(2)} (${chase.status?.status}), ` +
    `banner ${chase.banner ?? "none"}, ${chase.police} police vehicles + ${chase.units} other units, ` +
    `${chase.violations} violation event(s) (in feed: ${chase.inFeed}) after ${chase.simS.toFixed(1)} s sim; wait ${chaseWait}`,
);
// Art style (machine S, render/style.ts): low-poly dark by default in the night chase (palette
// fog, street lamps along the kerbs, post stack), the selector switches to greybox and back.
const art = await page.evaluate(async () => {
  const w = window.__wp;
  const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const { PALETTE } = await import("/src/render/palette.ts");
  const low = { id: w.style.id, fog: w.scene.scene.fog?.density ?? 0, lamps: w.style.lampCount("vesper_v2"), override: !!w.scene.renderOverride };
  const sel = document.getElementById("style");
  // Props (render/props.ts): palette cones; a live switch rebuilds them in place.
  // Sim paused: props must come back exactly where they were (the frame loop still runs
  // props.update, which rebuilds them for the new style).
  const wasPaused = w.app.paused;
  w.app.paused = true;
  await frame();
  const coneLow = w.props.coneMat.color.getHex();
  const propsBefore = { n: w.props.count, pose: Array.from(w.props.pose) };
  sel.value = "greybox";
  sel.dispatchEvent(new Event("change"));
  await frame();
  const grey = { id: w.style.id, fog: w.scene.scene.fog?.density ?? 0 };
  const coneGrey = w.props.coneMat.color.getHex();
  sel.value = "lowpoly";
  sel.dispatchEvent(new Event("change"));
  await frame();
  await frame();
  const others = [PALETTE.lampGlow, PALETTE.signalGlow, PALETTE.headlightGlow, PALETTE.brakeGlow, PALETTE.windowGlow];
  const brightest = Math.min(PALETTE.policeRedGlow, PALETTE.policeBlueGlow) >= 3 * Math.max(...others);
  // Instanced fleet (render/fleet.ts): palette liveries and HDR light bars in low-poly dark.
  const { styledPaint, styledLamp, TRAFFIC_PALETTE, BAR } = await import("/src/render/fleet.ts");
  const fleetPaint = styledPaint(TRAFFIC_PALETTE[2]) === PALETTE.trafficPaints[2];
  const bar = styledLamp(w.scene.hemi.color.clone(), BAR.blue);
  const fleetBar = Math.max(bar.r, bar.g, bar.b) > 3;
  const fleetRough = w.lod.fleet.mats.paint.roughness;
  const pose = Array.from(w.props.pose);
  const propsKept = w.props.count === propsBefore.n && pose.length === propsBefore.pose.length && pose.every((v, i) => v === propsBefore.pose[i]);
  w.app.paused = wasPaused;
  const cones = { low: coneLow === PALETTE.propCone, grey: coneGrey !== PALETTE.propCone, back: w.props.coneMat.color.getHex() === PALETTE.propCone, n: w.props.stats(w.rig.camera).cones, propsKept, props: w.props.count };
  return { low, grey, back: w.style.id, brightest, fleetPaint, fleetBar, fleetRough, cones };
});
check(
  art.low.id === "lowpoly" && art.low.override && art.low.fog > 0.001 && art.low.lamps > 100 && art.grey.id === "greybox" && art.grey.fog === 0 && art.back === "lowpoly" && art.brightest,
  `art style: low-poly dark in the chase (fog ${art.low.fog.toFixed(4)}, ${art.low.lamps} street lamps), greybox on the selector (fog ${art.grey.fog}), back to ${art.back}; police bars ≥ 3× every other glow: ${art.brightest}`,
);
check(
  art.fleetPaint && art.fleetBar && art.fleetRough === 0.85,
  `art style on the instanced fleet: palette livery ${art.fleetPaint}, HDR light bar ${art.fleetBar}, panel-matched paint roughness ${art.fleetRough}`,
);
check(
  art.cones.low && art.cones.grey && art.cones.back && art.cones.propsKept && art.cones.props > 0,
  `art style on props: cone material = PALETTE.propCone (${art.cones.n} cones in this session) (greybox orange when switched, palette again after), ${art.cones.props} props rebuilt in place across the switch: ${art.cones.propsKept}`,
);
await shot("night_run_greybox_toggle.png");

// City map HUD (workstream 16): the corner minimap shows the player and the police near them;
// M / Tab open the full city map, M / Tab / Esc close it. The player is parked in front of a
// patrol car so at least one police dot is inside the minimap.
{
  const parked = await page
    .waitForFunction(
      () => {
        try {
          window.__wp.sim.debugSetHeat(window.__wp.app.player, 0);
          window.__wp.sim.debugParkNearPolice(window.__wp.app.player, 40);
          return true;
        } catch (e) {
          window.__mapParkError = String(e);
          return false;
        }
      },
      null,
      { timeout: 120000, polling: 100 },
    )
    .then(() => true)
    .catch(() => false);
  // Wait for a drawn frame with the parked player (the sim runs well below real time here).
  await page
    .waitForFunction(() => window.__wp.map.hud.stats().minimap.units >= 1, null, { timeout: 30000, polling: 200 })
    .catch(() => {});
  const mini = await page.evaluate(() => {
    const s = window.__wp.map.hud.stats();
    const el = document.getElementById("wp-minimap");
    const r = el?.getBoundingClientRect();
    return { ...s.minimap, err: window.__mapParkError ?? null, city: s.city, district: s.district, visible: !!r && r.width > 100 && !el.hidden, total: window.__wp.sim.mapDynamic(window.__wp.app.player)[1] };
  });
  await shot("map_minimap_city_chase.png");
  check(
    parked && mini.visible && mini.player && mini.units >= 1 && mini.city === "vesper_v2",
    `minimap${parked ? "" : ` (park near police failed: ${mini.err})`}: player + ${mini.units} police dot(s) of ${mini.total} units (${mini.blinking} blinking), radius ${mini.radiusM.toFixed(0)} m, district ${mini.district ?? "?"}`,
  );
  const state = () => page.evaluate(() => ({ open: window.__wp.map.isOpen, shown: !document.getElementById("wp-map-full").hidden, ...window.__wp.map.hud.stats().full }));
  await page.keyboard.press("KeyM");
  await page.waitForTimeout(300);
  const opened = await state();
  await shot("map_full_city.png");
  await page.keyboard.press("KeyM");
  await page.waitForTimeout(100);
  const closedM = await state();
  await page.keyboard.press("Tab");
  await page.waitForTimeout(100);
  const openedTab = await state();
  await page.keyboard.press("Escape");
  await page.waitForTimeout(100);
  const closedEsc = await state();
  check(
    opened.open && opened.shown && opened.player && opened.units >= 1 && opened.districts >= 6 && !closedM.open && !closedM.shown && openedTab.open && !closedEsc.open,
    `full map: M opens (${opened.units} units, ${opened.districts} districts, ${opened.pois} POIs), M closes, Tab opens, Esc closes`,
  );
}
// LOD hand-over (playtest row 25): in a fresh chase, every traffic agent / police unit / sim
// vehicle is drawn with its real mesh on every frame after the first second (no placeholder,
// no gap); promotions and demotions (instanced fleet <-> full VehicleView) keep the pose: the
// drawn car moves < 0.5 m relative to its sim pose at the swap (frame-rate independent; the sim
// pose's own tick-by-tick continuity is wp_wasm's lod::tests); traffic never appears or vanishes
// within 300 m of the player.
{
  await page.evaluate(() => {
    window.__wp.startNightRun("debug_sedan", "Arcade");
    window.__wp.lod.log.start(0);
  });
  await page.keyboard.down("KeyW");
  const lodWait = await page
    .waitForFunction(
      () => {
        const s = window.__wp.lod.log.summary();
        return window.__wp.sim.worldInfo()[0] / 240 > 12 && s.swaps >= 2;
      },
      null,
      { timeout: 240000, polling: 500 },
    )
    .then(() => "ready")
    .catch(() => "timed out");
  await page.keyboard.up("KeyW");
  const lod = await page.evaluate(() => ({ ...window.__wp.lod.log.summary(), simS: window.__wp.sim.worldInfo()[0] / 240 }));
  await shot("lod_chase.png");
  const worst = lod.worstSwaps[0];
  check(
    lod.missing === 0,
    `LOD: no car drawn without its mesh after the first second (${lod.frames} frames, ${lod.cars} cars, ${lod.missing} missing` +
      `${lod.missing ? ": " + JSON.stringify(lod.missingSamples.slice(0, 3)) : ""}; longest placeholder ${lod.maxPlaceholderS.toFixed(2)} s)`,
  );
  check(
    lodWait === "ready" && lod.swaps >= 2 && lod.maxSwapJump < 0.5,
    `LOD: ${lod.swaps} promotion/demotion swaps in ${lod.simS.toFixed(1)} s sim, max hand-over jump (drawn vs sim pose) ${lod.maxSwapJump.toFixed(3)} m` +
      (worst ? ` (${worst.key} ${worst.from}→${worst.to})` : "") + `; wait ${lodWait}`,
  );
  check(
    lod.minAppearTraffic >= 300 && lod.minVanishTraffic >= 300,
    `LOD: traffic appears ≥ ${lod.minAppearTraffic.toFixed(0)} m and vanishes ≥ ${lod.minVanishTraffic.toFixed(0)} m from the player` +
      ` (police: appear ${lod.minAppearPolice.toFixed(0)} m, vanish ${lod.minVanishPolice.toFixed(0)} m)`,
  );
}
// Props (playtest row 50 "hitting invisible objects"): every parked car / cone is a physical
// body in the sim, so every one must be drawn. In a night run: the instanced props match the
// sim's prop list (each parked car drawn exactly once; every prop within 150 m of the camera
// drawn; parked cars within 150 m use the full traffic mesh), and a parked car knocked by the
// player is drawn where the sim has it (instance matrix read back vs prop_pose, every frame,
// < 0.2 m) while it moves. Owner playtest 2026-10-21 "parked cars don't take damage": the
// 43 km/h knock dents it, and the dent is drawn (its own panel mesh, skinned from its lattice).
{
  await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
  await page
    .waitForFunction(() => window.__wp.props.count > 0 && window.__wp.sim.worldInfo()[0] > 240, null, { timeout: 120000, polling: 200 })
    .catch(() => {});
  const knock = await page.evaluate(async () => {
    const wp = window.__wp;
    const frame = () => new Promise((r) => requestAnimationFrame(r));
    const target = wp.sim.debugKnockProp(wp.app.player, -1, 12);
    const start = Array.from(wp.sim.propPose(target));
    let maxErr = 0;
    let maxMove = 0;
    let frames = 0;
    let undrawn = 0;
    const t0 = wp.sim.worldInfo()[0];
    while (wp.sim.worldInfo()[0] - t0 < 240 * 3 && frames < 600) {
      await frame();
      frames++;
      const truth = Array.from(wp.sim.propPose(target));
      const drawn = wp.props.drawnPose(target);
      if (!drawn) {
        undrawn++;
        continue;
      }
      maxErr = Math.max(maxErr, Math.hypot(drawn[0] - truth[0], drawn[1] - truth[1], drawn[2] - truth[2]));
      maxMove = Math.max(maxMove, Math.hypot(truth[0] - start[0], truth[1] - start[1], truth[2] - start[2]));
    }
    // Let the camera settle on the stopped car, then count what is drawn around it.
    for (let i = 0; i < 30; i++) await frame();
    // Cost of the per-frame wasm call with nothing moving.
    const c0 = performance.now();
    for (let i = 0; i < 200; i++) wp.sim.propsMoved();
    const movedCallMs = (performance.now() - c0) / 200;
    const w0 = performance.now();
    for (let i = 0; i < 200; i++) wp.sim.parkedWrecks();
    const wrecksCallMs = (performance.now() - w0) / 200;
    const wreck = wp.props.wreck(target);
    const wrecks = wp.sim.parkedWrecks();
    let health = 1;
    for (let o = 0; o < wrecks.length; o += 12) if (wrecks[o] === target) health = wrecks[o + 2];
    return { target, frames, maxErr, maxMove, undrawn, stats: wp.props.stats(wp.rig.camera, 150), model: wp.props.model, movedCallMs, wreck, health, wrecksCallMs };
  });
  await shot("props_knock.png");
  const st = knock.stats;
  check(
    st.parked > 1000 && st.drawnFull + st.drawnProxy + st.drawnBox + st.wrecksDrawn === st.parked && st.inRange > 0 && st.drawnInRange === st.inRange &&
      (st.fullInRange === st.parkedInRange || st.fullInRange === st.maxFull || (st.parkedInRange > st.fullInRange && st.fullInRange === st.drawnFull)),
    `props: ${st.parked} parked cars (${knock.model ?? "box stand-in"}) in ${st.chunks} chunks + ${st.cones} cones; drawn ${st.drawnFull} full + ${st.drawnProxy} proxy + ${st.drawnBox} box + ${st.wrecksDrawn} dented; ` +
      `${st.drawnInRange}/${st.inRange} props within 150 m drawn, ${st.fullInRange}/${st.parkedInRange} parked cars there with the full mesh ` +
      `(${st.fullInFullRange}/${st.inFullRange} within ${250} m); ${st.repicks} re-picks, ${st.repickWrites} + ${st.movedWrites} matrix writes; ` +
      `update ${st.updateMeanMs.toFixed(3)} ms mean / ${st.updateMaxMs.toFixed(2)} ms worst, props_moved() ${knock.movedCallMs.toFixed(3)} ms`,
  );
  check(
    knock.maxMove > 0.3 && knock.maxErr < 0.2 && knock.undrawn === 0,
    `props: knocked parked car #${knock.target} moved ${knock.maxMove.toFixed(2)} m; drawn vs sim pose ≤ ${knock.maxErr.toFixed(3)} m over ${knock.frames} frames` +
      (knock.undrawn ? `; ${knock.undrawn} frames undrawn` : ""),
  );
  check(
    knock.wreck !== null && knock.wreck.drawn && knock.wreck.maxDisp > 0.005 && knock.health < 1,
    `props: the knocked parked car is dented (health ${knock.health.toFixed(3)}) and drawn dented: ` +
      (knock.wreck ? `own panel mesh ${knock.wreck.drawn ? "on" : "not loaded"}, largest drawn vertex offset ${knock.wreck.maxDisp.toFixed(3)} m` : "never promoted") +
      `; ${st.wrecks} dented parked car(s), ${st.wreckReskins} re-skins, parked_wrecks() ${knock.wrecksCallMs.toFixed(3)} ms`,
  );
}
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
    // Capture sequence (playtest B5): the banner alone first (2.5 s), then the bill opens and
    // the banner steps aside while it is open.
    const t0 = performance.now();
    while (!read().bill && performance.now() - t0 < 8000) await frames();
    out.billAfterMs = Math.round(performance.now() - t0);
    out.billed = read();
    stub({ level: "None", status: "Clear", last_outcome: "Captured", outcome_age_s: 30 });
    await frames();
    out.later = read();
    sim.policeStatus = real;
    window.__wp.ui.setBill(false);
    return out;
  });
  check(
    flow.pursuit.banner === "PURSUIT" && flow.searching.banner === "SEARCHING" && flow.escaped.banner === "ESCAPED" && !flow.escaped.bill &&
      flow.captured.banner === "CAPTURED" && !flow.captured.bill && flow.billed.bill && flow.billed.banner === null && flow.billAfterMs >= 2000 &&
      flow.later.banner === null &&
      flow.escapedEnd === "escaped" && flow.capturedEnd?.how === "captured" && flow.capturedEnd?.level === "L3",
    `pursuit banner: ${flow.pursuit.banner} → ${flow.searching.banner} → ${flow.escaped.banner} (${flow.escapedEnd}) → ${flow.captured.banner} alone (bill ${flow.captured.bill ? "already open" : "waits"}), ` +
      `bill ${flow.billed.bill ? "opened" : "missing"} after ${flow.billAfterMs} ms with the banner ${flow.billed.banner ? "still up" : "hidden"} ` +
      `(chase end ${flow.capturedEnd?.how} at ${flow.capturedEnd?.level}) → ${flow.later.banner ?? "cleared"}`,
  );
}
// End of run (lead): a real capture in the sim (player parked in front of a patrol car at L2) →
// end-of-run panel + repair bill → "Repair & continue" pays from the ledger and restores the car.
{
  await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
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
  // Capture sequence (playtest B5): banner 2.5 s → end-of-run card → the bill 1.5 s later.
  await page.waitForFunction(() => !document.getElementById("ui-bill").classList.contains("hidden"), null, { timeout: 15000, polling: 100 }).catch(() => {});
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
// Career (machine G): garage → buy a beater → night run with the career car → busted → garage
// (car in the pound, saved to localStorage) → reload (the career survives) → release → repair
// all → a new run uses the repaired car.
{
  const G = '[data-testid="career-garage"]';
  const fresh = await page.evaluate(() => window.__wp.careerApi.newCareer("Arcade"));
  await page.click("#sc-garage");
  const opened = await page.waitForSelector(G, { timeout: 10000 }).then(() => true).catch(() => false);
  await page.click(`${G} button[role="tab"]:has-text("Dealer")`);
  // Dealer columns: name, class, PI, 0–100, New, Used, Worn (7th) buy buttons, lock badge.
  await page.click(`${G} tr[data-car="junkyard_beater"] td:nth-child(7) button`);
  const bought = await page.evaluate(() => window.__wp.careerApi.view());
  await shot("career_garage_bought.png");
  await page.click(`${G} button:has-text("Night run")`);
  const run = await page.evaluate(() => {
    const wp = window.__wp;
    return { active: wp.sim.careerRunActive(), car: wp.sim.scenario().entities[0].car, open: wp.garage.open };
  });
  check(
    opened && fresh.cash > 0 && bought.cars.length === 1 && bought.cash === fresh.cash - bought.dealer.find((d) => d.def_id === "junkyard_beater").price_worn &&
      run.active && run.car === "junkyard_beater" && !run.open,
    `career: garage opens, beater bought ($${fresh.cash} → $${bought.cash}), night run runs the career car (${run.car})`,
  );
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
  const busted = await page
    .waitForFunction(
      () => window.__wp.runEnd.open && [...document.querySelectorAll("#ui-run-end button")].some((b) => b.textContent.includes("Garage")),
      null,
      { timeout: 240000, polling: 250 },
    )
    .then(() => true)
    .catch(() => false);
  await page.click('#ui-run-end button:has-text("Garage")').catch(() => {});
  await page.waitForSelector(G, { timeout: 10000 }).catch(() => {});
  const stored = await page.evaluate(() => {
    const p = JSON.parse(localStorage.getItem("wp.career.v1") ?? "null");
    return { location: p?.account?.garage?.[0]?.location ?? null, captures: p?.account?.heat_history?.captures ?? 0, version: p?.version };
  });
  await shot("career_garage_after_bust.png");
  check(
    parked && busted && stored.version === 2 && stored.captures === 1 && stored.location && typeof stored.location === "object" && "Impound" in stored.location,
    `career: busted → garage, saved (v${stored.version}): captures ${stored.captures}, car ${JSON.stringify(stored.location)}`,
  );

  await page.reload();
  await ready();
  await page.click("#sc-garage");
  await page.waitForSelector(G, { timeout: 10000 }).catch(() => {});
  const reloaded = await page.evaluate(() => window.__wp.careerApi.view());
  await page.click(`${G} button[role="tab"]:has-text("Impound")`);
  await page.click(`${G} button:has-text("Pay & release")`);
  await page.click(`${G} button[role="tab"]:has-text("Cars")`);
  const released = await page.evaluate(() => window.__wp.careerApi.view());
  await page.click(`${G} button:has-text("Repair all")`).catch(() => {});
  const repaired = await page.evaluate(() => window.__wp.careerApi.view());
  check(
    reloaded.cars[0]?.location === "Impound" && released.cars[0]?.location === "Garage" && released.cash < reloaded.cash &&
      (repaired.cars[0]?.repair_lines.length ?? 1) === 0 && repaired.cars[0]?.health === 1 && repaired.can_start,
    `career: survives a reload (car ${reloaded.cars[0]?.location}), released ($${reloaded.cash} → $${released.cash}), repaired (health ${repaired.cars[0]?.health})`,
  );
  await page.click(`${G} button:has-text("Free drive")`);
  await page.waitForTimeout(500);
  const again = await page.evaluate(() => {
    const wp = window.__wp;
    const out = { active: wp.sim.careerRunActive(), car: wp.sim.scenario().entities[0].car, health: wp.sim.states()[17] };
    wp.openGarage();
    const save = wp.sim.careerSave();
    const view = wp.careerApi.view();
    const back = wp.careerApi.importSave(save);
    return { ...out, reimported: back.cash === view.cash && back.day === view.day && wp.sim.careerSave() === save };
  });
  check(
    again.active && again.car === "junkyard_beater" && again.health === 1 && again.reimported,
    `career: new run with the repaired car (${again.car}, health ${again.health}); export → import keeps the career`,
  );
  await page.evaluate(() => window.__wp.careerApi.close());
}
// The selector keeps the old flat grid: the chase restarts there. Start a plain (non-career)
// night run first: the career checks above leave a career free-drive run active.
await page.evaluate(() => {
  const career = document.getElementById("career");
  if (career && career.checked) career.click();
  document.getElementById("sc-chase").click();
});
await page.waitForTimeout(1500);
// Garages (lead): every repair shop has a marker; stopping inside one holds the car and opens the
// garage panel with the shop's quote (a pristine car: nothing to repair); "Leave" lets it go.
{
  await page.evaluate(() => window.__wp.startFreeDrive("debug_sedan", "Arcade", true));
  await page.waitForTimeout(500);
  const placed = await page.evaluate(() => {
    const wp = window.__wp;
    const garages = wp.sim.garages();
    let markers = 0;
    wp.scene.world.traverse((o) => (markers += o.name.startsWith("garage:") ? 1 : 0));
    const g = garages[0];
    if (g) wp.sim.debugPlace(wp.app.player, g.pos[0], g.pos[1], g.heading_rad, 0);
    return { count: garages.length, markers, name: g?.name ?? "" };
  });
  const opened = await page
    .waitForFunction(() => !!window.__wp.garageView.offer && !!document.querySelector('#ui-garage-visit [data-testid="garage-visit"]'), null, { timeout: 30000, polling: 100 })
    .then(() => true)
    .catch(() => false);
  const visit = await page.evaluate(() => {
    const wp = window.__wp;
    return {
      shop: wp.garageView.offer?.site.name ?? null,
      text: document.querySelector('#ui-garage-visit [data-testid="garage-visit"]')?.textContent ?? "",
      held: !wp.sim.setInput(wp.app.player, { ...wp.app.input, throttle: 1 }),
    };
  });
  await shot("garage_visit.png");
  await page.click("#ui-garage-visit button:not(.wp-btn--primary)").catch(() => {});
  await page.waitForTimeout(300);
  const left = await page.evaluate(() => {
    const wp = window.__wp;
    return {
      offer: wp.sim.garageVisit(),
      panel: !!document.querySelector('#ui-garage-visit [data-testid="garage-visit"]'),
      live: wp.sim.setInput(wp.app.player, { ...wp.app.input, throttle: 0.5 }),
    };
  });
  check(
    placed.count > 0 && placed.markers === placed.count && opened && visit.shop === placed.name && visit.held && /Nothing to repair/.test(visit.text),
    `garage: ${placed.count} shops / ${placed.markers} markers; stopping in ${placed.name} opens the panel (${visit.shop}), input ${visit.held ? "held" : "live"}`,
  );
  check(!left.offer && !left.panel && left.live, `garage "Leave": panel closed, input ${left.live ? "live" : "held"}`);
  // Back to the night run: the selector check below restarts the current mode.
  await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
}
// The selector keeps the old flat grid: the chase restarts there.
await page.evaluate(() => {
  const sel = document.getElementById("city-map");
  sel.value = "procedural_12x12";
  sel.dispatchEvent(new Event("change"));
});
await page.waitForTimeout(1500);
const grid = await page.evaluate(() => ({ id: window.__wp.sim.cityId(), name: window.__wp.sim.scenario().name }));
check(grid.id === "procedural_12x12" && grid.name === "night_run", `city selector: chase restarted in ${grid.id}`);
await page.evaluate(() => {
  const sel = document.getElementById("city-map");
  sel.value = "vesper_v2";
  document.getElementById("city").checked = false;
  window.__wp.startFreeDrive("debug_sedan", "Arcade", false);
});

// 4b. Mods you can see (render/mod_parts.ts): the shop installs a build, the sim drives the
// modified car, every visible part is fitted on the panel mesh, and front parts leave with the
// front bumper in a crash while the trunk wing stays.
{
  await page.evaluate(() => window.__wp.startFreeDrive("kestrel_ts", "Arcade", false, undefined, []));
  await page.click("#mods-btn");
  for (const id of ["aero_splitter", "aero_wing_2", "wheels_forged", "tires_wide", "brakes_kit_1"]) {
    await page.click(`#mod-shop button[data-mod="${id}"]`);
    await page.waitForTimeout(150);
  }
  const shop = await page.evaluate(() => ({
    mods: window.__wp.sim.scenario().entities[0].mods ?? [],
    status: document.querySelector(".mod-shop-status").textContent,
  }));
  // The GT wing needs the splitter: clicked in that order the whole build is legal.
  check(shop.mods.length === 5 && !shop.status, `mod shop installs a build (${shop.mods.join(", ")})`);
  await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.fitted, null, { timeout: 20000 });
  const fit = await page.evaluate(() => {
    const v = window.__wp.app.views.get(window.__wp.app.player);
    return { drawn: v.fitted.drawn, width: v.info.wheels[0].width_m, base: v.build.wheels[0].base_width_m };
  });
  check(
    ["BrakeKit", "FrontSplitter", "RearWing", "Rims"].every((k) => fit.drawn.includes(k)) && Math.abs(fit.width - fit.base - 0.02) < 1e-4,
    `build drawn on the car (${fit.drawn.join(", ")}) and the sim drives +2 cm tires (${fit.base.toFixed(3)} → ${fit.width.toFixed(3)} m)`,
  );
  await shot("mods_build.png");
  // Remove everything again (and leave the viewer's saved build empty for the next checks).
  await page.click('#mod-shop .mod-shop-head button[title="remove every mod"]');
  await page.click('#mod-shop .mod-shop-head button[title="close"]');
  const stock = await page.evaluate(() => (window.__wp.sim.scenario().entities[0].mods ?? []).length);
  check(stock === 0, "mod shop: back to stock");

  // Crash: a 40 km/h barrier hit dents the front parts with the bumper; the wing stays put.
  await page.evaluate(async () => {
    const wp = window.__wp;
    const sc = JSON.parse(wp.sim.scenarioFromRon(await (await fetch("scenarios/crash/barrier_frontal.ron")).text()));
    sc.panels = true;
    sc.entities[0].car = "kestrel_ts";
    sc.entities[0].mods = ["aero_splitter", "aero_wing_1", "prot_bumper_front"];
    sc.entities[0].driver = { cruise: { target_mps: 11 } };
    wp.runScenarioJson(JSON.stringify(sc));
  });
  await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 240 * 6.8, null, { timeout: 120000 });
  await page.waitForTimeout(300);
  // Largest vertex displacement from rest per mod piece (Infinity: it left with its region).
  const moved = await page.evaluate(() => {
    const v = window.__wp.app.views.get(window.__wp.app.player);
    const out = {};
    for (const name of v.panels.partNames()) {
      if (!name.startsWith("mod_")) continue;
      const m = v.panels.partMotion(name);
      out[name] = m.hidden ? Infinity : m.disp;
    }
    return out;
  });
  check(
    moved.mod_splitter > 0.05 && moved.mod_bull_bar > 0.05 && moved.mod_wing_street < 0.02,
    `crash: splitter / bull bar follow the crushed front (${moved.mod_splitter?.toFixed(2)} / ${moved.mod_bull_bar?.toFixed(2)} m), wing stays (${moved.mod_wing_street?.toFixed(3)} m)`,
  );
  await shot("mods_crash.png");
}

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
  const agentsNow = await page.evaluate(() => window.__wp.sim.trafficAgents().length / 7);
  console.log(`BENCH sim-only city + ${agentsNow} traffic agents: ${(await timeSim()).toFixed(3)} ms per 60 Hz frame`);
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
stopServer();

// 6. Weather (lead): selector, rain, wet roads, spray, fog, wipers, rainy-night chase, a rainy
// replay verified by simcli (e2e/weather.mjs, its own server and browser).
const wx = spawnSync("node", [join(here, "weather.mjs")], {
  cwd: root,
  encoding: "utf8",
  env: { ...process.env, WEATHER_SHOTS: shots ? join(shots, "weather") : "" },
});
process.stdout.write(wx.stdout);
check(wx.status === 0, `weather checks (e2e/weather.mjs)${wx.status === 0 ? "" : ": " + (wx.stderr || "").trim().split("\n").pop()}`);

// 7. Browser perf (browser perf pass): night run + rainy-night chase at 1280×720 must keep
// near real time and stay within generous draw-call / triangle / allocation budgets
// (e2e/perf_check.mjs; PERF=0 skips it).
if (process.env.PERF !== "0") {
  const perf = spawnSync("node", [join(here, "perf_check.mjs")], { cwd: root, encoding: "utf8", env: process.env });
  process.stdout.write(perf.stdout);
  check(perf.status === 0, `perf checks (e2e/perf_check.mjs)${perf.status === 0 ? "" : ": " + (perf.stderr || "").trim().split("\n").pop()}`);
}
// 8. Gamepad + rebindable controls (e2e/gamepad.mjs, its own server and browser): fake pad
// drives and steers, assists follow the device, disconnect releases, rebind survives a reload,
// input latency one frame.
const gp = spawnSync("node", [join(here, "gamepad.mjs")], {
  cwd: root,
  encoding: "utf8",
  env: { ...process.env, GAMEPAD_SHOTS: shots ?? "" },
});
process.stdout.write(gp.stdout);
check(gp.status === 0, `gamepad + controls checks (e2e/gamepad.mjs)${gp.status === 0 ? "" : ": " + (gp.stderr || "").trim().split("\n").pop()}`);

// 8. Reset in place (e2e/reset.mjs, its own server and browser): R after a crash → upright,
// within 15 m, repaired, replayable; night run keeps heat; career run does not repair.
const rs = spawnSync("node", [join(here, "reset.mjs")], { cwd: root, encoding: "utf8", env: process.env });
process.stdout.write(rs.stdout);
check(rs.status === 0, `reset-in-place checks (e2e/reset.mjs)${rs.status === 0 ? "" : ": " + (rs.stderr || "").trim().split("\n").pop()}`);
// 9. Crash effects (e2e/crash_fx.mjs, its own server and browser): a scripted head-on throws
// sparks, glass and debris, a Simulation barrier crash smokes, a burnout lays skid marks, the
// effects allocate nothing in steady state and switch quality live.
const cfx = spawnSync("node", [join(here, "crash_fx.mjs")], {
  cwd: root,
  encoding: "utf8",
  env: { ...process.env, FX_SHOTS: shots ? join(shots, "fx") : "" },
});
process.stdout.write(cfx.stdout);
check(cfx.status === 0, `crash effects checks (e2e/crash_fx.mjs)${cfx.status === 0 ? "" : ": " + (cfx.stderr || "").trim().split("\n").pop()}`);
// 10. Instant crash replay (lead, row 70; e2e/crash_replay.mjs, its own server and browser): a
// scripted crash offers the replay, accepting plays it and returns to live play with the same
// tick and hash, auto mode and skip, no replays on benches.
const cr = spawnSync("node", [join(here, "crash_replay.mjs")], {
  cwd: root,
  encoding: "utf8",
  env: { ...process.env, CRASH_SHOTS: shots ? join(shots, "crash_replay") : "" },
});
process.stdout.write(cr.stdout);
check(cr.status === 0, `crash replay checks (e2e/crash_replay.mjs)${cr.status === 0 ? "" : ": " + (cr.stderr || "").trim().split("\n").pop()}`);
// 11. Touch controls (e2e/touch.mjs, its own server and browser): an emulated phone shows the
// controls, a swipe on the throttle accelerates, steering (buttons + tilt), edge buttons, no
// HUD overlaps at phone sizes, rotate hint in portrait; a desktop shows no touch UI.
const tc = spawnSync("node", [join(here, "touch.mjs")], {
  cwd: root,
  encoding: "utf8",
  env: { ...process.env, TOUCH_SHOTS: shots ? join(shots, "touch") : "" },
});
process.stdout.write(tc.stdout);
check(tc.status === 0, `touch controls checks (e2e/touch.mjs)${tc.status === 0 ? "" : ": " + (tc.stderr || "").trim().split("\n").pop()}`);
// 12. Game menu + HUD layout (UI kit, e2e/menu.mjs, its own server and browser): no HUD overlaps
// at 1280×720 / 1366×768 / 1920×1080 with the developer panels on and off, Esc pauses and
// resumes, keyboard + gamepad navigation, a setting survives a reload, ?menu=1 title screen.
const mn = spawnSync("node", [join(here, "menu.mjs")], { cwd: root, encoding: "utf8", env: { ...process.env, MENU_SHOTS: shots ?? "" } });
process.stdout.write(mn.stdout);
check(mn.status === 0, `game menu + layout checks (e2e/menu.mjs)${mn.status === 0 ? "" : ": " + (mn.stderr || "").trim().split("\n").pop()}`);
// 13. Street racers (e2e/street.mjs, its own server and browser; lead, rows 149–154): the
// roster by day / night, honk to challenge, no challenge while wanted, the A→B picker with
// waypoints, side-by-side start, a race to the finish (day keyboard + mouse, night gamepad),
// the night run's racers, touch. Then no physical traffic car without a collider
// (e2e/traffic_bodies.mjs, row 149).
const st = spawnSync("node", [join(here, "street.mjs")], { cwd: root, encoding: "utf8", env: { ...process.env, SHOTS: shots ? join(shots, "street") : "" } });
process.stdout.write(st.stdout);
check(st.status === 0, `street racer checks (e2e/street.mjs)${st.status === 0 ? "" : ": " + (st.stderr || "").trim().split("\n").pop()}`);
const tb = spawnSync("node", [join(here, "traffic_bodies.mjs")], { cwd: root, encoding: "utf8", env: process.env });
process.stdout.write(tb.stdout);
check(tb.status === 0, `traffic bodies (e2e/traffic_bodies.mjs)${tb.status === 0 ? "" : ": " + (tb.stderr || "").trim().split("\n").pop()}`);
console.log(failures ? `${failures} check(s) failed` : "all checks passed");
process.exit(failures ? 1 : 0);
