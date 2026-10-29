// Landmark place screenshots for docs/media/landmarks/ (lead, 2026-10-29; headless Chromium via
// Playwright): one chase-camera shot of each place type in the career world (Ashpeak County)
// by day, the dealership and police station again at night, and the city map / minimap with
// the place icons. Usage (after `npm run wasm`): node e2e/landmark_shots.mjs [outDir].
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/landmarks"));
mkdirSync(out, { recursive: true });
const PORT = 5217;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 820 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
const save = async (name) => {
  await page.screenshot({ path: join(out, name) });
  console.log("wrote", name);
};

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await page.evaluate(() => {
  const s = window.__wp.sim;
  const v = s.careerNew("Arcade");
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  s.careerBuy(d.def_id, "Worn");
});

/** A career Free Roam in the combined world at `time` ("day" / "night"); waits for the city. */
async function roam(time) {
  await page.evaluate((t) => window.__wp.careerApi.startRun("free", { time: t, weather: "clear" }), time);
  await page.waitForFunction(
    () => {
      const w = window.__wp;
      return w.sim.careerRunActive() && w.app.views.get(w.app.player)?.panels?.ready !== false && (w.scene.cities.get("ashpeak")?.children.length ?? 0) > 0;
    },
    null,
    { timeout: 180000 },
  );
  await page.waitForTimeout(3000);
}

/** Put the car on the ring facing the main building and let the chase camera settle. */
async function visit(l) {
  // A few metres back across the road from the ring, so the frame holds the whole frontage.
  const p = [l.ring[0] - l.out[0] * 4, l.ring[1] - l.out[1] * 4];
  const c = l.main.reduce((s, q) => [s[0] + q[0] / l.main.length, s[1] + q[1] / l.main.length], [0, 0]);
  const yaw = Math.atan2(-(c[0] - p[0]), c[1] - p[1]);
  await page.evaluate(([x, y, a]) => window.__wp.sim.debugPlace(window.__wp.app.player, x, y, a, 0), [p[0], p[1], yaw]);
  await page.waitForTimeout(3500);
}

await roam("day");
const hideOverlay = () => page.addStyleTag({ content: "#overlay { display: none !important; }" });
let hidden = await hideOverlay();
const layouts = await page.evaluate(() => window.__wp.placeSigns.layouts);
console.log(`${layouts.length} places`);
if (layouts.length === 0) throw new Error("no place layouts");
const pick = (style, id) => layouts.find((l) => l.style === style && (!id || l.id === id)) ?? layouts.find((l) => l.style === style);
const shots = [
  ["dealership.png", pick("Dealership", "harbourline_auto")],
  ["used_lot.png", pick("UsedLot", "lucky_lou_used")],
  ["junkyard.png", pick("Junkyard", "dockside_salvage")],
  ["repair_shop.png", pick("RepairShop", "Meridian Motorworks")],
  ["police_station.png", pick("PoliceStation", "Central Precinct")],
  ["safehouse.png", pick("Safehouse", "The Coldroom")],
  ["home_dockside_lockup.png", pick("LockUp")],
  ["home_spire_basement.png", pick("TowerGarage")],
  ["home_harnell_barn.png", pick("Barn")],
];
for (const [name, l] of shots) {
  if (!l) {
    console.log("no place for", name);
    continue;
  }
  await visit(l);
  await save(name);
}

// The map: zoomed on the Port Vesper places around the dealership, then the minimap there.
await visit(pick("Dealership", "harbourline_auto"));
await hidden.evaluate((el) => el.remove());
await page.waitForTimeout(800);
const r = await page.evaluate(() => {
  const b = document.getElementById("wp-minimap")?.getBoundingClientRect();
  return b && b.width > 0 ? { x: b.x, y: b.y, width: b.width, height: b.height } : null;
});
if (r) {
  await page.screenshot({ path: join(out, "minimap.png"), clip: r });
  console.log("wrote minimap.png");
}
await page.evaluate(() => window.__wp.map.hud.open());
await page.waitForTimeout(1200);
await save("map_full.png");
for (let i = 0; i < 5; i++) await page.keyboard.press("Equal");
await page.keyboard.press("KeyF");
await page.waitForTimeout(1200);
await save("map_zoomed.png");
await page.evaluate(() => window.__wp.map.hud.close());
await page.waitForTimeout(500);
// Night: signs, glazing and the blue lamp glow.
await roam("night");
hidden = await hideOverlay();
await visit(pick("Dealership", "harbourline_auto"));
await save("dealership_night.png");
await visit(pick("PoliceStation", "Central Precinct"));
await save("police_station_night.png");
await visit(pick("RepairShop", "Meridian Motorworks"));
await save("repair_shop_night.png");

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 3));
  process.exit(1);
}
process.exit(0);
