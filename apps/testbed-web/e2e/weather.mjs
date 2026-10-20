// Weather check + screenshots (headless Chromium, Playwright). Run by `npm run e2e` (run.mjs)
// at its end, or alone: `npm run e2e:weather` (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome, WEATHER_SHOTS=dir (default ../../docs/media/weather; "" = none).
//
// Checks: the weather selector restarts the session with the chosen weather (it lands in the
// scenario, so replays carry it); heavy rain shows rain streaks, wet roads (grid texture
// uploaded), spray behind the moving car and the HUD's weather / road line; fog sets the scene
// fog from the visibility; wipers sweep in the cockpit camera; the rainy-night city chase
// starts at night in a downpour; a recording made in the rain verifies headlessly in simcli;
// and rain rendering stays cheap (frame-time delta measured with the rain on vs off).
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5197;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.WEATHER_SHOTS ?? join(root, "../../docs/media/weather");
if (shots) mkdirSync(shots, { recursive: true });

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
const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const state = () =>
  page.evaluate(() => {
    const w = window.__wp;
    const sc = w.sim.scenario();
    const info = Array.from(w.sim.weatherInfo());
    const s = w.scene.scene;
    return {
      name: sc.name,
      weather: sc.weather ?? null,
      info,
      hud: document.getElementById("weather-hud")?.textContent ?? "",
      hudShown: document.getElementById("weather-hud")?.style.display !== "none",
      fog: s.fog ? s.fog.density : 0,
      // The art style's own distance fog (low-poly dark, render/style.ts); weather adds to it.
      baseFog: w.style.id === "lowpoly" ? w.style.baseFogDensity : 0,
      rain: s.children.some((o) => o.isLineSegments && o.geometry.isInstancedBufferGeometry && o.visible && o.geometry.instanceCount > 0),
      spray: s.children.some((o) => o.isPoints && o.visible),
      wipers: document.getElementById("wipers")?.style.display !== "none",
      speed: w.sim.states()[14],
      hour: w.sim.worldInfo()[1],
    };
  });

// 1. Heavy rain on the flat world (free drive): selector → restart with weather.
await page.goto(URL);
await ready();
await page.evaluate(() => window.__wp.setWeather("heavy_rain"));
await page.keyboard.down("KeyW");
await page.waitForTimeout(4000);
let st = await state();
check(
  st.weather?.kind === "HeavyRain" && st.info[0] === 1 && st.info[1] === 3 && st.info[3] > 1,
  `selector restarts with heavy rain (scenario weather ${JSON.stringify(st.weather)}, ${st.info[3].toFixed(1)} mm/h)`,
);
check(st.rain && st.hudShown && /heavy rain/.test(st.hud) && /road/.test(st.hud), `rain streaks + HUD "${st.hud}"`);
check(st.spray, `spray behind the car at ${st.speed.toFixed(1)} m/s on a ${st.info[14].toFixed(2)} mm film`);
const wet = await page.evaluate(() => {
  const u = window.__wp.weather.wetUniforms;
  return { on: u.uWetOn.value, w: u.uWetGrid.value.image.width, h: u.uWetGrid.value.image.height };
});
check(wet.on === 1 && wet.w > 10 && wet.h > 10, `wet-road grid texture uploaded (${wet.w}×${wet.h} cells)`);
await page.evaluate(() => window.__wp.setCamera("chase"));
await page.waitForTimeout(500);
await shot("heavy_rain_flat_chase.png");

// 2. Wipers in the cockpit camera.
await page.evaluate(() => window.__wp.setCamera("cockpit"));
await page.waitForTimeout(700);
st = await state();
check(st.wipers, "wipers sweep in the cockpit camera");
await shot("heavy_rain_cockpit_wipers.png");
await page.evaluate(() => window.__wp.setCamera("chase"));
await page.keyboard.up("KeyW");

// 3. A recording in the rain verifies headlessly (weather rides in the replay's scenario).
await page.evaluate(() => window.__wp.sim.recordStart());
await page.keyboard.down("KeyW");
await page.keyboard.down("KeyA");
await page.waitForTimeout(1500);
await page.keyboard.up("KeyA");
await page.keyboard.up("KeyW");
const rec = await page.evaluate(() => Array.from(window.__wp.stopRecording()));
const replayFile = join(tmpdir(), `wp-weather-${process.pid}.replay.bin`);
writeFileSync(replayFile, Buffer.from(rec));
const verify = spawnSync("cargo", ["run", "-q", "--release", "-p", "simcli", "--", "replay", replayFile], {
  cwd: join(root, "../.."),
  encoding: "utf8",
});
check(verify.status === 0, `simcli replay verifies a rainy recording: ${(verify.stdout + verify.stderr).trim().split("\n").pop()}`);

// 4. Rain rendering cost: scene.render with the rain streaks and spray drawn vs hidden, same
// frame, GPU synced with gl.finish() (swiftshader is a software GPU: a relative number).
const perf = await page.evaluate(() => {
  const w = window.__wp;
  const gl = w.scene.renderer.getContext();
  const s = w.scene.scene;
  const extras = s.children.filter((o) => (o.isLineSegments && o.geometry.isInstancedBufferGeometry) || o.isPoints);
  const time = (on, which = () => true) => {
    for (const o of extras) o.visible = on && which(o);
    const px = new Uint8Array(4);
    // readPixels waits for the GPU (gl.finish alone may return early).
    w.scene.render(w.rig.camera);
    gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    const t0 = performance.now();
    for (let i = 0; i < 20; i++) {
      w.scene.render(w.rig.camera);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    }
    return (performance.now() - t0) / 20;
  };
  const off0 = time(false);
  const on = time(true);
  const rainOnly = time(true, (o) => o.isLineSegments);
  const sprayOnly = time(true, (o) => o.isPoints);
  const rainGeo = extras.find((o) => o.isLineSegments)?.geometry;
  const n0 = rainGeo?.instanceCount ?? 0;
  if (rainGeo) rainGeo.instanceCount = 1;
  const oneStreak = time(true, (o) => o.isLineSegments);
  if (rainGeo) rainGeo.instanceCount = n0;
  const off = time(false);
  const t0 = performance.now();
  for (let i = 0; i < 200; i++) w.sim.weatherInfo();
  const info = (performance.now() - t0) / 200;
  const t1 = performance.now();
  for (let i = 0; i < 20; i++) w.sim.weatherGrid();
  const grid = (performance.now() - t1) / 20;
  return { oneStreak, off0, on, rainOnly, sprayOnly, off, info, grid, streaks: extras.find((o) => o.isLineSegments)?.geometry.instanceCount ?? 0 };
});
console.log(
  `PERF weather render (swiftshader, 1280×720): scene.render ${perf.on.toFixed(2)} ms with rain + spray ` +
    `(${perf.streaks} streaks; rain only ${perf.rainOnly.toFixed(2)}, spray only ${perf.sprayOnly.toFixed(2)}, one streak ${perf.oneStreak.toFixed(2)}) vs ${perf.off0.toFixed(2)} / ${perf.off.toFixed(2)} ms without (before / after); weather_info ${perf.info.toFixed(3)} ms, ` +
    `weather_grid ${perf.grid.toFixed(2)} ms (1 Hz)`,
);
// Sim cost in the browser (wasm): 240 steps of 1/60 s, rain vs no weather, same car.
const timeSim = () =>
  page.evaluate(() => {
    const w = window.__wp;
    w.app.paused = true;
    for (let i = 0; i < 30; i++) w.sim.step(1 / 60);
    const t0 = performance.now();
    for (let i = 0; i < 240; i++) w.sim.step(1 / 60);
    const ms = (performance.now() - t0) / 240;
    w.app.paused = false;
    return ms;
  });
const simRain = await timeSim();
await page.evaluate(() => window.__wp.setWeather("none"));
await page.waitForTimeout(500);
const simDry = await timeSim();
console.log(`PERF weather sim (wasm, flat world, 1 car + 2 km grid): ${simRain.toFixed(3)} ms per 60 Hz frame in rain vs ${simDry.toFixed(3)} ms without weather`);
st = await state();
check(st.info[0] === 0 && !st.rain && !st.hudShown && st.fog === st.baseFog, `weather off: no rain, no weather fog (style fog ${st.baseFog.toFixed(4)}), no weather HUD`);
// 5. Fog: visibility drives the scene fog.
await page.evaluate(() => window.__wp.setWeather("fog"));
await page.waitForFunction(() => window.__wp.sim.scenario().weather?.kind === "Fog", null, { timeout: 30000 });
await page.waitForTimeout(1500);
st = await state();
check(st.info[1] === 4 && st.info[5] < 300 && st.fog > 0.01, `fog (${st.name}, ${JSON.stringify(st.weather)}): visibility ${st.info[5].toFixed(0)} m → FogExp2 density ${st.fog.toFixed(4)}`);
await shot("fog_flat.png");

// 6. City: rain in Port Vesper by day, then the rainy-night chase.
await page.evaluate(() => {
  window.__wp.setWeather("rain");
  document.getElementById("city").checked = true;
  window.__wp.startFreeDrive("lindwurm_4t", "Arcade", true, 15);
});
await page.waitForFunction(() => window.__wp.sim.trafficAgents().length > 0, null, { timeout: 120000 });
await page.waitForTimeout(3000);
st = await state();
check(st.weather?.kind === "LightRain" && st.info[0] === 1, `city free drive in rain (${st.hud})`);
await page.evaluate(() => window.__wp.setCamera("chaseFar"));
await page.waitForTimeout(800);
await shot("rain_city_day.png");
await page.evaluate(() => {
  window.__wp.setWeather("auto");
  document.getElementById("city").checked = false;
  window.__wp.startCityChase("lindwurm_4t", "Arcade", true);
});
await page.waitForFunction(() => window.__wp.sim.scenario().name === "city_chase_rainy_night", null, { timeout: 120000 });
await page.waitForTimeout(5000);
st = await state();
check(
  st.weather?.kind === "HeavyRain" && st.hour >= 23 && st.rain,
  `rainy-night city chase: ${st.hour.toFixed(2)} h, ${st.info[3].toFixed(1)} mm/h, HUD "${st.hud}"`,
);
await page.evaluate(() => window.__wp.setCamera("chase"));
await page.waitForTimeout(800);
await shot("rainy_night_chase.png");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `${failures} weather check(s) failed` : "all weather checks passed");
process.exit(failures ? 1 : 0);
