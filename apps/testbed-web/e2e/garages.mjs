// End-to-end check of the city's garages and their map icons (garage audit) in headless
// Chromium. Usage: npm run e2e:garages (needs `npm run wasm` first). Env: CITY=vesper_v2 (default)
// | ashpeak | procedural_12x12, CHROMIUM=/path/to/chrome, SHOTS=dir for screenshots,
// DIST=dir to serve a `vite build --outDir dir` with `vite preview` instead of the dev server
// (no reloads when other files change).
//
// Checks, by driving (no teleport into the ring):
//  - quick play: every garage has a marker; rolling in from 30 m short of it and braking opens
//    the visit panel; Esc leaves it (keyboard, no mouse);
//  - the map: shops carry their legit / crooked kind, race meets come from race_spots() (no
//    RaceStartGrid icons), the legend lists helipads and rest stops;
//  - career: a career free drive has the shops too; stopping in one offers "Enter garage";
//    Enter opens the career garage priced at that shop; Close drives out of that shop in a new
//    career run.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5213;
const CITY = process.env.CITY ?? "vesper_v2";
const URL = `http://localhost:${PORT}/?city=${CITY}`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const args = process.env.DIST ? ["vite", "preview", "--outDir", process.env.DIST] : ["vite"];
const server = spawn("npx", [...args, "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
const stopServer = () => server.kill("SIGTERM");
process.on("exit", stopServer);

async function waitForServer() {
  for (let i = 0; i < 150; i++) {
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
const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 120000 });

/** Roll into garage `g` from 30 m short of it at 7 m/s and brake inside; true once a visit opens. */
async function driveInto(g) {
  const player = await page.evaluate(() => window.__wp.app.player);
  const back = 30;
  const x = g.pos[0] + Math.sin(g.heading_rad) * back;
  const y = g.pos[1] - Math.cos(g.heading_rad) * back;
  // Out of every ring first (a visit needs a fresh entry).
  await page.evaluate(([p, x, y]) => window.__wp.sim.debugPlace(p, x + 400, y + 400, 0, 0), [player, x, y]);
  await page.waitForTimeout(400);
  await page.evaluate(([p, x, y, yaw]) => window.__wp.sim.debugPlace(p, x, y, yaw, 7), [player, x, y, g.heading_rad]);
  let braking = false;
  let opened = false;
  const t0 = Date.now();
  while (Date.now() - t0 < 30000) {
    const st = await page.evaluate(([gx, gy]) => {
      const wp = window.__wp;
      const pos = wp.app.views.get(wp.app.player)?.root.position;
      return { visit: !!wp.sim.garageVisit(), d: pos ? Math.hypot(pos.x - gx, pos.y - gy) : -1 };
    }, [g.pos[0], g.pos[1]]);
    if (st.visit) {
      opened = true;
      break;
    }
    if (!braking && st.d >= 0 && st.d < 11) {
      braking = true;
      await page.keyboard.down("KeyS");
    }
    await page.waitForTimeout(100);
  }
  if (braking) await page.keyboard.up("KeyS");
  if (!opened) {
    const why = await page.evaluate(([gx, gy, gz]) => {
      const wp = window.__wp;
      const pos = wp.app.views.get(wp.app.player)?.root.position;
      return { pos: pos ? [pos.x, pos.y, pos.z].map((v) => Math.round(v)) : null, d: pos ? Math.hypot(pos.x - gx, pos.y - gy).toFixed(1) : null, dz: pos ? (pos.z - gz).toFixed(1) : null, police: wp.sim.policeStatus?.(wp.app.player)?.status ?? null };
    }, g.pos);
    console.log(`      no visit at ${g.name}: ${JSON.stringify(why)}`);
  }
  return opened;
}

// 1. Quick play: every shop can be driven into, and left with Esc.
await page.evaluate(() => window.__wp.startFreeDrive("debug_sedan", "Arcade", true, 13));
await page.waitForFunction((c) => window.__wp.sim.cityId() === c, CITY, { timeout: 180000 });
await page.waitForTimeout(1000);
const garages = await page.evaluate(() => window.__wp.sim.garages());
const markers = await page.evaluate(() => {
  let n = 0;
  window.__wp.scene.world.traverse((o) => (n += o.name.startsWith("garage:") ? 1 : 0));
  return n;
});
check(garages.length > 0 && markers === garages.length, `${CITY}: ${garages.length} shops, ${markers} markers`);
for (const g of garages) {
  const opened = await driveInto(g);
  const panel = await page.evaluate(() => document.querySelector('#ui-garage-visit [data-testid="garage-visit"]')?.textContent ?? "");
  check(opened && panel.includes(g.name) && /demo account/.test(panel), `drove into ${g.name} (${g.kind}): visit panel open`);
  if (shots) await shot(`visit_${g.name.replace(/[^a-z0-9]+/gi, "_")}.png`);
  if (opened) await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  const left = await page.evaluate(() => ({ visit: !!window.__wp.sim.garageVisit(), paused: window.__wp.app.paused }));
  check(!left.visit && !left.paused, `Esc left ${g.name} (no pause menu)`);
}

// 2. The map: shops with their kind, race meets from race_spots(), legend rows.
const map = await page.evaluate(() => {
  const wp = window.__wp;
  const pois = wp.map.hud.map?.pois ?? [];
  return {
    garages: pois.filter((p) => p.kind === "Garage").map((p) => p.shop ?? null),
    spots: pois.filter((p) => p.kind === "RaceSpot").length,
    grid: pois.filter((p) => p.kind === "RaceStartGrid").length,
    listed: wp.sim.raceSpots().length,
  };
});
check(map.garages.length === garages.length && map.garages.every((k) => k === "Legit" || k === "Crooked"), `map: ${map.garages.length} shops with their kind (${map.garages.join(", ")})`);
check(map.spots === map.listed && map.grid === 0, `map: ${map.spots} race meets = race_spots() (${map.listed}), no stale race-start icons`);
await page.keyboard.press("KeyM");
await page.waitForTimeout(800);
const legend = await page.evaluate(() => [...document.querySelectorAll(".wp-map-legend-row span")].map((s) => s.textContent));
check(["Police helipad", "Rest stop"].every((r) => legend.includes(r)) && legend.some((r) => r.startsWith("Race meet")), "legend: helipad, rest stop, race meet rows");
if (shots) await shot("fullmap.png");
await page.keyboard.press("KeyM");
await page.waitForTimeout(300);

// 3. Career: a new career with the floor car, a free drive on the city.
const career = await page.evaluate(async () => {
  const wp = window.__wp;
  wp.openGarage();
  wp.careerApi.newCareer("Arcade");
  if (!wp.sim.careerView().cars.length) wp.careerApi.floorCar();
  document.getElementById("city").checked = true;
  const ok = wp.startCareerRun("free");
  await new Promise((r) => setTimeout(r, 1500));
  return { ok, active: wp.sim.careerRunActive(), garages: wp.sim.garages().length };
});
check(career.ok && career.active && career.garages === garages.length, `career run has the shops (${career.garages})`);
const g0 = garages[0];
const opened = await driveInto(g0);
const offer = await page.evaluate(() => window.__wp.sim.garageVisit());
check(opened && offer?.career === true && !!(await page.$('[data-testid="garage-enter"]')), `career: stopping in ${g0.name} offers "Enter garage"`);
if (shots) await shot("career_visit.png");
await page.keyboard.press("Enter");
await page.waitForTimeout(800);
const inGarage = await page.evaluate(() => ({
  open: window.__wp.garage.open,
  shop: document.querySelector('[data-testid="repair-shop"]')?.textContent ?? "",
  active: window.__wp.sim.careerRunActive(),
}));
check(inGarage.open && !inGarage.active && inGarage.shop.includes(g0.name), `Enter: career garage at ${g0.name} ("${inGarage.shop}")`);
if (shots) await shot("career_garage_at_shop.png");
await page.evaluate(() => {
  const b = [...document.querySelectorAll("#ui-garage button")].find((x) => /^Close/.test(x.textContent ?? ""));
  b?.click();
});
await page.waitForTimeout(2000);
const after = await page.evaluate(([gx, gy]) => {
  const wp = window.__wp;
  const pos = wp.app.views.get(wp.app.player)?.root.position;
  return { active: wp.sim.careerRunActive(), open: wp.garage.open, paused: wp.app.paused, d: pos ? Math.hypot(pos.x - gx, pos.y - gy) : -1 };
}, [g0.pos[0], g0.pos[1]]);
check(after.active && !after.open && !after.paused && after.d >= 0 && after.d < 25, `Close: a new career drive out of ${g0.name} (${after.d.toFixed(1)} m from it)`);

check(errors.length === 0, `no page errors (${errors.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "all passed");
process.exit(failures ? 1 : 0);
