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

// 1b. Informant tips (F4): a wanted car at a crooked shop buys one (key 2 = the second tier);
// the heat drops by at most one wanted level.
{
  const crooked = garages.find((g) => g.kind === "Crooked");
  if (crooked) {
    const player = await page.evaluate(() => window.__wp.app.player);
    await page.evaluate(([p, g]) => {
      const wp = window.__wp;
      wp.sim.debugPlace(p, g.pos[0] + 400, g.pos[1] + 400, 0, 0);
    }, [player, crooked]);
    await page.waitForTimeout(400);
    await page.evaluate(([p, g]) => {
      const wp = window.__wp;
      wp.sim.debugSetHeat(p, 35);
      wp.sim.debugPlace(p, g.pos[0], g.pos[1], g.heading_rad, 0);
    }, [player, crooked]);
    const offer = await page
      .waitForFunction(() => window.__wp.sim.garageVisit(), null, { timeout: 20000, polling: 100 })
      .then((h) => h.jsonValue())
      .catch(() => null);
    const tiers = offer?.tips?.map((t) => t.tier) ?? [];
    await page.waitForTimeout(300);
    await page.keyboard.press("Digit2");
    await page.waitForTimeout(500);
    const after = await page.evaluate(() => ({ text: document.querySelector('[data-testid="garage-tipped"]')?.textContent ?? "", tips: window.__wp.sim.garageVisit()?.tips?.length ?? -1 }));
    check(tiers.length >= 2 && /Heat 3\d → 10/.test(after.text) && after.tips === 0, `informant tip at ${crooked.name}: ${tiers.join("/")} → "${after.text}"`);
    if (shots) await shot("tip.png");
    await page.keyboard.press("Escape");
    await page.evaluate((p) => window.__wp.sim.debugSetHeat(p, 0), player);
    await page.waitForTimeout(300);
  }
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

// 2b. Safehouses (F39): a marker at each; inside one while wanted, the hide timer shows.
{
  const n = await page.evaluate(() => {
    let k = 0;
    window.__wp.scene.world.traverse((o) => (k += o.name.startsWith("safehouse:") ? 1 : 0));
    return { markers: k, listed: JSON.parse(window.__wp.sim.safehousesJson())?.sites.length ?? 0 };
  });
  check(n.listed > 0 && n.markers === n.listed, `safehouses: ${n.listed} listed, ${n.markers} markers`);
  const site = await page.evaluate(() => JSON.parse(window.__wp.sim.safehousesJson()).sites[0]);
  const player = await page.evaluate(() => window.__wp.app.player);
  await page.evaluate(([p, s]) => {
    const wp = window.__wp;
    wp.sim.debugSetHeat(p, 15);
    wp.sim.debugPlace(p, s.pos[0], s.pos[1], 0, 0);
  }, [player, site]);
  const hud = await page
    .waitForFunction(() => /hiding|safehouse/i.test(document.querySelector('[data-testid="safehouse-hud"]')?.textContent ?? ""), null, { timeout: 20000, polling: 200 })
    .then(() => page.evaluate(() => document.querySelector('[data-testid="safehouse-hud"]').textContent))
    .catch(() => "");
  check(/hiding/i.test(hud), `safehouse HUD while wanted inside ${site.name}: "${hud}"`);
  if (shots) await shot("safehouse.png");
  await page.evaluate((p) => window.__wp.sim.debugSetHeat(p, 0), player);
}

// 2c. GPS (F40): a click on a garage icon on the full map sets a route there; a second clears it.
{
  await page.keyboard.press("KeyM");
  await page.waitForTimeout(600);
  const g = garages[garages.length - 1];
  const at = await page.evaluate((g) => window.__wp.map.hud.clientAt(g.pos[0], g.pos[1]), g);
  if (at) await page.mouse.click(at[0], at[1]);
  await page.waitForTimeout(500);
  const gps = await page.evaluate(() => ({ gps: window.__wp.map.gps, tip: document.querySelector('[data-testid="map-tip"]')?.textContent ?? "" }));
  check(!!gps.gps && gps.gps.name === g.name && gps.gps.length_m > 0 && gps.tip.includes(g.name), `GPS: clicking ${g.name} on the map routes there (${Math.round(gps.gps?.length_m ?? 0)} m; tip "${gps.tip.slice(0, 60)}")`);
  if (shots) await shot("gps.png");
  if (at) await page.mouse.click(at[0], at[1]);
  await page.waitForTimeout(300);
  check(!(await page.evaluate(() => window.__wp.map.gps)), "GPS: a second click clears it");
  await page.keyboard.press("KeyM");
  await page.waitForTimeout(300);
}

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
// F36: the shop fits mods (its speciality's groups); the blurb says what it does.
{
  await page.click('[data-testid="career-garage"] button[role="tab"]:has-text("Cars")').catch(() => {});
  const mods = await page.evaluate(() => {
    const b = document.querySelector('[data-testid="cars-mods"]');
    return b ? !b.disabled : null;
  });
  check(mods === true, `at ${g0.name} the Mods button is enabled`);
}
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
// F36: the menu garage is home: no mods there (they are fitted at a city garage).
{
  await page.evaluate(() => window.__wp.openGarage());
  await page.waitForTimeout(500);
  await page.click('[data-testid="career-garage"] button[role="tab"]:has-text("Cars")').catch(() => {});
  const home = await page.evaluate(() => {
    const b = document.querySelector('[data-testid="cars-mods"]');
    return b ? b.disabled : null;
  });
  check(home === true, "home garage: Mods only at a city garage");
  await page.evaluate(() => window.__wp.startCareerRun("free"));
  await page.waitForTimeout(1500);
}

// 4. Car lots on the map (F37): a "$" icon per lot; stopping at one with a career, Enter opens
// the lot visit.
{
  const lots = await page.evaluate(() => ({ sites: window.__wp.lotSpots?.sites ?? [], icons: (window.__wp.map.hud.map?.pois ?? []).filter((p) => p.kind === "CarLot").length }));
  check(lots.sites.length > 0 && lots.icons === lots.sites.length, `car lots: ${lots.sites.length} on the city, ${lots.icons} map icons`);
  const lot = lots.sites[0];
  if (lot) {
    const player = await page.evaluate(() => window.__wp.app.player);
    await page.evaluate(([p, l]) => window.__wp.sim.debugPlace(p, l.pos[0], l.pos[1], l.heading_rad, 0), [player, lot]);
    const prompt = await page
      .waitForFunction(() => /browse/.test(document.querySelector('[data-testid="lot-prompt"]')?.textContent ?? "") && !!window.__wp.lotSpots.here, null, { timeout: 20000, polling: 200 })
      .then(() => true)
      .catch(() => false);
    if (shots) await shot("lot_prompt.png");
    await page.keyboard.press("Enter");
    const visit = await page
      .waitForSelector('[data-testid="lot-visit"]', { timeout: 30000 })
      .then(() => page.evaluate(() => window.__wp.sim.lotVisit()?.lot ?? null))
      .catch(() => null);
    check(prompt && visit === lot.id, `stopping at ${lot.name} and pressing Enter opens its lot visit (${visit})`);
    if (shots) await shot("lot_visit.png");
  }
}

check(errors.length === 0, `no page errors (${errors.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "all passed");
process.exit(failures ? 1 : 0);
