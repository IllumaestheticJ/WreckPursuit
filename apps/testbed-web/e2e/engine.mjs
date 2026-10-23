// Engine internals end to end (headless Chromium): pull the engine out whole, strip it on the
// engine stand in order, build it back and fit it. Usage: node e2e/engine.mjs (needs
// `npm run wasm` first). SHOTS=dir writes screenshots.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5209;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

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
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const screen = () => page.evaluate(() => window.__wp.workshop.current());
const slot = async (id) => (await screen()).slots.find((s) => s.id === id);
const settle = (ms = 900) => page.waitForTimeout(ms);
/** Wait until the car is built and the bay placed. */
const placed = () =>
  page.waitForFunction(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return w.workshop.active && v?.panels.ready && w.scene.world.getObjectByName("workshop-room");
  }, null, { timeout: 30000 });
/** Click the middle of a slot's part on screen. */
async function clickSlot(id) {
  const pt = await page.evaluate((s) => window.__wp.workshop.pointOfSlot(s), id);
  if (!pt) return false;
  await page.mouse.click(pt.x, pt.y);
  await settle(200);
  return true;
}
const button = (text) => page.locator(`[data-testid="workshop"] button`, { hasText: text }).first();

const stand = () => page.locator('[data-testid="ws-engine-stand"]');
const row = (id) => stand().locator(`[data-testid="ws-stand-slot"][data-slot="${id}"]`);
const STRIP = ["intake_manifold", "throttle_body", "exhaust_manifold", "valve_cover", "timing", "camshafts", "valvetrain", "cylinder_head", "head_gasket", "pistons"];

await page.goto(shots ? `${URL}?gfx=high` : URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

const uid = await page.evaluate(() => {
  const s = window.__wp.sim;
  let v = s.careerNew("Arcade");
  const d = [...v.dealer].sort((a, b) => a.price_worn - b.price_worn)[0];
  v = s.careerBuy(d.def_id, "Worn");
  const u = v.cars[v.cars.length - 1].uid;
  // Clear the engine bay (each part's own removal is covered by e2e/workshop.mjs).
  for (const slot of ["front_bumper", "radiator", "intake", "exhaust", "fuel_system", "driveshaft", "gearbox"]) s.careerRemovePart(u, slot);
  return u;
});
await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
await placed();
await settle(1200);

// The engine comes out whole through the part list and its card.
await button("Part list").click();
await page.locator('[data-testid="ws-partlist"] button', { hasText: "Engine block" }).click();
await page.locator('[data-testid="ws-card"] button', { hasText: "Remove" }).click();
await placed();
await settle();
let s = await screen();
check(s.missing.includes("Engine block"), `the engine is out: missing ${s.missing.join(", ")}`);
check(!s.missing.includes("Pistons and rings"), "its internals are not listed as missing (they are in the engine)");
check(s.stands.length === 1 && s.stands[0].slots.length >= 14, `the loose engine is on the stand with ${s.stands[0]?.slots.length} parts`);
const listed = await page.locator('[data-testid="ws-partlist"] button', { hasText: "Pistons" }).count();
check(listed === 0, "the part list no longer shows the engine's internals");
await button("Part list").click();

// The engine stands beside the car in 3D; clicking a part on it opens the stand list there.
await button("Engine stand").click();
await settle(1500);
await shot("engine_stand_3d.png");
const pt = await page.evaluate(() => window.__wp.workshop.pointOfStandPart("valve_cover"));
check(!!pt, "the engine is drawn on the stand");
if (pt) {
  await page.mouse.click(pt.x, pt.y);
  await settle(300);
}
check(await stand().isVisible(), "clicking the engine opens the stand parts");
check((await stand().locator('[aria-selected="true"]').getAttribute("data-slot")) === "valve_cover", "the clicked part is picked in the list");

// Strip it on the stand, top down.
check(await row("pistons").locator("button", { hasText: "Remove" }).isDisabled(), "the pistons cannot come out with the head on");
await shot("engine_stand_full.png");
for (const id of STRIP) await row(id).locator("button", { hasText: "Remove" }).click();
s = await screen();
check(s.stands[0].missing.includes("Pistons and rings") && s.stands[0].missing.length === STRIP.length, `stripped to the crank: ${s.stands[0].missing.length} parts out`);
const next = s.inventory.filter((p) => p.stand_fits.length > 0).map((p) => p.part.kind);
check(next.length === 1 && next[0] === "pistons", `only the pistons can go back first (${next.join(", ")})`);
await settle(600);
await shot("engine_stand_stripped.png");
const pistons = await page.evaluate(() => window.__wp.workshop.pointOfStandPart("pistons"));
check(pistons === null, "the pistons are gone from the 3D engine too");
const crank = await page.evaluate(() => window.__wp.workshop.pointOfStandPart("crankshaft"));
check(!!crank, "the crank still shows in the block");

// The head will not go on before the gasket.
const head = await row("cylinder_head").locator("button", { hasText: "Fit" }).count();
check(head === 0, "the cylinder head has no Fit button before the gasket is in");

// Build it back, bottom up.
for (const id of [...STRIP].reverse()) await row(id).locator("button", { hasText: "Fit" }).first().click();
s = await screen();
check(s.stands[0].missing.length === 0, "the engine is complete again");
await settle(600);
await shot("engine_stand_rebuilt.png");
await stand().locator("button", { hasText: "Close" }).click();

// Back in the car.
const eng = s.inventory.find((p) => p.part.kind === "engine");
await page.locator(`[data-testid="ws-tile"][data-uid="${eng.part.uid}"]`).click();
await button("Fit: Engine block").click();
await placed();
await settle();
s = await screen();
check(!s.missing.includes("Engine block") && !s.missing.some((m) => /Pistons|Crankshaft/.test(m)), `the engine is back with its internals (missing: ${s.missing.join(", ")})`);
check(s.stands.length === 0, "the stand is empty");

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "engine e2e: all passed");
process.exit(failures ? 1 : 0);
