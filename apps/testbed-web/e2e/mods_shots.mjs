// Screenshots of modified cars (render/mod_parts.ts) for review and docs.
// Usage: node e2e/mods_shots.mjs <out dir>   (needs `npm run wasm` first)
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = process.argv[2] ?? join(root, "mods-shots");
mkdirSync(out, { recursive: true });
const PORT = 5198;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(`http://localhost:${PORT}/`)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1200, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

const ONLY = process.env.ONLY;
const BUILDS = {
  stock: [],
  street: ["wheels_forged", "tires_semi_slick", "tires_wide", "aero_splitter", "aero_wing_2", "susp_coilovers_2", "brakes_pads_1", "brakes_kit_2", "body_side_skirts", "paint_metallic", "vinyl_stripes", "engine_exhaust_2"],
  widebody: ["wheels_mesh", "body_front_widebody", "body_side_widebody", "body_rear_widebody", "body_hood_scoop", "aero_diffuser", "aero_wing_1", "vinyl_flames", "brakes_kit_1"],
  demo: ["wheels_steel", "tires_all_terrain", "prot_cage_2", "prot_bumper_front", "prot_bumper_rear", "prot_armor_plate", "prot_radiator_guard", "susp_offroad_lift", "engine_fuel_system", "fi_supercharger_2", "body_roof_rack", "vinyl_sponsor", "cooling_intercooler_1"],
};
const car = process.env.CAR ?? "kestrel_ts";
for (const [name, mods] of Object.entries(BUILDS)) {
  if (ONLY && name !== ONLY) continue;
  await page.goto(`http://localhost:${PORT}/?mods=${mods.join(",") || "none"}`);
  await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
  const carId = name === "demo" ? (process.env.DEMO_CAR ?? "hauler_1500") : car;
  await page.evaluate(([c, m]) => window.__wp.startFreeDrive(c, "Arcade", false, undefined, m), [carId, mods]);
  await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.panels.ready, null, { timeout: 20000 });
  await page.waitForTimeout(800);
  await page.evaluate(() => {
    document.getElementById("overlay").style.display = "none";
    for (const id of ["cgVelocity", "tireForces", "slipColour", "telemetry", "components", "chase"]) {
      if (window.__wp.layers?.on(id)) window.__wp.toggleLayer(id);
    }
  });
  const info = await page.evaluate(() => {
    const v = window.__wp.app.views.get(window.__wp.app.player);
    return { fitted: v.fitted, mods: window.__wp.sim.scenario().entities[0].mods ?? [] };
  });
  console.log(name, carId, JSON.stringify(info));
  for (const [view, eye] of [["rear", [-3.4, -4.4, 1.7]], ["front", [3.2, 4.4, 1.2]], ["side", [5.0, 0.3, 0.9]], ["low", [2.4, 4.6, 0.35]]]) {
    await page.evaluate((e) => {
      const r = window.__wp.app.views.get(window.__wp.app.player).root.position;
      const x = r.x, y = r.y;
      window.__wp.lookFrom([x + e[0], y + e[1], e[2]], [x, y, 0.6]);
    }, eye);
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(out, `${name}_${view}.png`) });
  }
}
console.log(errors.length ? `ERRORS:\n${errors.join("\n")}` : "no page errors");
await browser.close();
process.exit(errors.length ? 1 : 0);
