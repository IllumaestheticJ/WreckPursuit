// Cosmetic looks end to end (headless Chromium): every visible mod's pieces build on several car
// models without errors, bolt-ons appear on the car, lamps light, and screenshots of themed
// builds for a look check. Usage: node e2e/cosmetics.mjs (needs `npm run wasm` first).
// SHOTS=dir writes screenshots.
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5208;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const visuals = JSON.parse(readFileSync(join(root, "../../data/cosmetics/visuals.json"), "utf8")).mods;
const visible = Object.keys(visuals).filter((id) => Array.isArray(visuals[id]));

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
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
const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
const errors = [];
const warnings = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text());
  if (m.type() === "warning" && m.text().startsWith("cosmetics:")) warnings.push(m.text());
});
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });

// Full resolution for the screenshots (software GL would drop to a quarter).
await page.goto(`${URL}?gfx=high`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
// Clean shots: no HUD panels, no debug drawing on the car.
await page.evaluate(() => {
  for (const id of ["tireForces", "slipColour", "contacts", "cgVelocity"]) window.__wp.toggleLayer(id);
});

/** Free drive in `car`, paused, with the car's panel mesh ready. */
async function drive(car) {
  await page.evaluate((c) => {
    window.__wp.startFreeDrive(c);
    window.__wp.app.paused = true;
    document.body.classList.add("wp-workshop");
  }, car);
  await page.waitForFunction(() => {
    const w = window.__wp;
    return w.app.views.get(w.app.player)?.panels.ready;
  }, null, { timeout: 30000 });
}

/** Put `mods`' look on the player car and wait for it. Returns piece counts. */
async function look(mods, showcase = true) {
  await page.evaluate(([m, s]) => window.__wp.app.views.get(window.__wp.app.player).setLook(m, s), [mods, showcase]);
  await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player).look !== null, null, { timeout: 15000 });
  await page.waitForTimeout(300);
  return page.evaluate(() => {
    const v = window.__wp.app.views.get(window.__wp.app.player);
    let meshes = 0;
    v.look.root.traverse((o) => (meshes += o.isMesh ? 1 : 0));
    return { pieces: v.look.root.children.length, meshes };
  });
}

/** Camera from car-space eye to car-space target. */
async function cam(eye, at) {
  await page.evaluate(([e, a]) => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    const V = v.root.position.constructor;
    const toSim = (p) => {
      const q = v.root.localToWorld(new V(p[0], p[1], p[2]));
      return [q.x, -q.z, q.y];
    };
    w.rig.lookFrom(toSim(e), toSim(a));
  }, [eye, at]);
  await page.waitForTimeout(500);
}

const BUILDS = {
  tuner: ["aero_canards", "aero_swan_wing", "body_side_widebody", "body_rear_widebody", "body_lights_halo", "body_fog_led", "body_underglow_purple", "engine_tip_quad", "wheels_deep_dish", "tires_raised_letters", "vinyl_flames", "paint_pearl", "interior_tint", "body_hood_vented", "brakes_kit_2"],
  brawler: ["prot_bull_bar_ram", "prot_bumper_rear", "prot_armor_plate", "prot_cage_2", "prot_skid_plate", "prot_radiator_guard", "prot_hubs", "body_roof_led_bar", "wheels_black_steel", "tires_all_terrain", "body_mud_flaps", "susp_offroad_lift", "vinyl_camo_digital", "paint_matte", "glass_shatterproof", "interior_window_net", "elec_scanner_2"],
  muscle: ["fi_supercharger_2", "engine_v8_swap", "aero_ducktail", "wheels_turbofan", "tires_drag_radial", "tires_whitewall", "vinyl_stripes", "paint_metallic", "engine_tip_dual", "body_lights_xenon", "body_fog_lights", "prot_bull_bar_tube"],
  rally: ["body_roof_rack", "body_mud_flaps", "wheels_mesh", "tires_red_line", "vinyl_race_number", "aero_wing_1", "body_lights_amber", "prot_bull_bar_guard", "elec_radar_2", "weight_carbon_hood", "weight_carbon_trunk", "interior_window_net", "prot_cage_1"],
};

// Every visible mod builds on every model checked, with no errors.
for (const car of ["junkyard_beater", "kestrel_ts", "hauler_1500", "vireo_xs"]) {
  await drive(car);
  const before = errors.length;
  const r = await look(visible);
  check(r.meshes > 100 && errors.length === before, `${car}: all ${visible.length} visible mods build (${r.pieces} pieces, ${r.meshes} meshes)`);
}
check(warnings.length === 0, `every piece has a builder (${warnings.slice(0, 3).join(" | ")})`);

// Themed builds on two models, from the front three-quarter and the rear three-quarter.
for (const car of ["kestrel_ts", "junkyard_beater"]) {
  await drive(car);
  for (const [name, mods] of Object.entries(BUILDS)) {
    const r = await look(mods);
    check(r.pieces > 0, `${car} ${name}: ${r.pieces} pieces`);
    await cam([-4.2, 4.6, 1.9], [0, 0.3, 0.6]);
    await shot(`${car}_${name}_front.png`);
    await cam([3.8, -4.8, 1.8], [0, -0.3, 0.6]);
    await shot(`${car}_${name}_rear.png`);
  }
}

// Lamps: the halo rings glow with the lights on and go dark with them off.
await drive("kestrel_ts");
await look(["body_lights_halo", "body_underglow_blue"], false);
const lampsOff = await page.evaluate(() => {
  const l = window.__wp.app.views.get(window.__wp.app.player).look;
  l.setLights(false);
  let lit = 0;
  l.root.traverse((o) => o.isMesh && o.material.emissiveIntensity > 0 && lit++);
  l.setLights(true);
  let on = 0;
  l.root.traverse((o) => o.isMesh && o.material.emissiveIntensity > 0 && on++);
  return { lit, on };
});
check(lampsOff.lit === 0 && lampsOff.on > 0, `lamp pieces follow the headlights (off ${lampsOff.lit}, on ${lampsOff.on})`);

// A vinyl gallery on the side of one car.
const vinyls = visible.filter((id) => id.startsWith("vinyl_"));
for (const v of vinyls) {
  await look([v]);
  await cam([-4.8, 0.2, 1.2], [0, 0, 0.6]);
  await shot(`vinyl_${v.slice(6)}.png`);
}
check(vinyls.length >= 18, `${vinyls.length} vinyl designs`);

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "cosmetics e2e: all passed");
process.exit(failures ? 1 : 0);
