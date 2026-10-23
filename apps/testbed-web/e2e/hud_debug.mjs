// Debug tools switch + player HUD (owner, 2026-10-23: "clean up the HUD to be more player
// friendly, keep the debugging tools but hide the buttons when not being used"). Headless
// Chromium (Playwright). Run alone: `npm run e2e:hud` (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome, HUD_SHOTS=dir to write hud_*.png.
//
// Checks (`?dev=auto` turns off the automation default, so the page behaves as for a player):
//  - default: player mode, no debug element visible (drawer, telemetry, inputs, feed, component /
//    region panels, perf lines, big silhouette, ⚙ until the mouse moves); speed cluster bottom-
//    right, minimap bottom-left, no "CLEAR" chase box while nobody is after the player;
//  - debug-layer keys do nothing with the tools off (X keeps the traffic, K the saved toggle);
//    F6 (reduced flashing) still works;
//  - the ⚙ button shows on mouse move, hides again after ~3 s, and turns the tools on;
//  - ` toggles the tools; the choice is saved and survives a reload; the drawer folds to its
//    header (also saved) and the layer checkboxes toggle layers;
//  - a stored `developer: true` from before this change is reset once;
//  - no HUD overlaps at 1280×720 and 1920×1080 in player mode (free drive and a race).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";
import { hudBoxes, overlaps } from "./hud_layout.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5193;
const shots = process.env.HUD_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const URL = `http://localhost:${PORT}/`;

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
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));

const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const frames = (n) => page.evaluate((k) => new Promise((r) => { let i = 0; const f = () => (++i >= k ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }), n);
const settle = async () => {
  const p0 = await page.evaluate(() => window.__wp.layout.passes);
  await page.waitForFunction((p) => window.__wp.layout.passes >= p + 2, p0, { timeout: 30000 });
  await frames(2);
};
const shot = async (name) => shots && (await page.screenshot({ path: join(shots, name) }));
/** Selectors that are visible right now (display, visibility, size, not inside .hidden). */
const visible = (sels) =>
  page.evaluate((list) => {
    const out = [];
    for (const s of list) {
      for (const el of document.querySelectorAll(s)) {
        const cs = getComputedStyle(el);
        const r = el.getBoundingClientRect();
        if (cs.display === "none" || cs.visibility === "hidden" || Number(cs.opacity) === 0 || el.closest(".hidden, [hidden], .wp-hidden") || r.width < 2 || r.height < 2) continue;
        out.push(s);
        break;
      }
    }
    return out;
  }, sels);
const DEBUG = ["#controls", "canvas.telemetry", ".inputs", ".feed", ".comps", ".regions", "#audio-panel", ".hud-main .hud-dbg", "canvas.silhouette", "#dev-toggle"];
const rect = (sel) => page.evaluate((s) => document.querySelector(s)?.getBoundingClientRect().toJSON() ?? null, sel);

// ----- 1. One-time reset of a stored developer: true; player mode by default ------------------
await page.goto(`${URL}?dev=auto`);
await page.evaluate(() => {
  localStorage.removeItem("wp.testbed.devReset.v1");
  localStorage.setItem("wp.gameSettings.v1", JSON.stringify({ version: 1, developer: true }));
});
await page.goto(`${URL}?city=vesper_v2&dev=auto`);
await ready();
await frames(4);
await settle();
let st = await page.evaluate(() => ({
  player: document.body.classList.contains("wp-player"),
  on: window.__wp.devSwitch.on,
  saved: JSON.parse(localStorage.getItem("wp.gameSettings.v1") ?? "{}").developer,
}));
check(st.player && !st.on && st.saved === false, `a saved developer: true is reset once: player mode (${JSON.stringify(st)})`);
let dbg = await visible(DEBUG);
check(dbg.length === 0, `no debug element visible by default${dbg.length ? ` (${dbg.join(", ")})` : ""}`);
const speedo = await rect(".hud-main");
const mini = await rect(".wp-minimap");
check(speedo && speedo.right > 1280 - 40 && speedo.bottom > 720 - 40, `speed cluster bottom-right (${speedo && `${Math.round(speedo.left)},${Math.round(speedo.top)}`})`);
check(mini && mini.left < 40 && mini.bottom > 720 - 40, `minimap bottom-left (${mini && `${Math.round(mini.left)},${Math.round(mini.top)}`})`);
const chase = await page.evaluate(() => ({ status: window.__wp.ui?.status?.status ?? null, hidden: document.getElementById("ui-chase").classList.contains("hidden") }));
check(chase.hidden, `no chase readout while nobody is after the player (${JSON.stringify(chase)})`);
const text = await page.evaluate(() => document.querySelector(".hud-main").innerText);
check(!/fps|tick|hash|THR-F|CSTR|A-CLU/.test(text) && /km\/h/.test(text) && /car \d+%/.test(text), `player cluster shows speed + car health only (${JSON.stringify(text.replace(/\s+/g, " "))})`);
await shot("hud_player_default.png");

// ----- 2. Debug-layer keys are inert with the tools off; F6 stays live ------------------------
const saved = () => page.evaluate(() => JSON.parse(localStorage.getItem("wp.testbed.layers.v1") ?? "{}"));
const before = await saved();
await page.keyboard.press("KeyX");
await page.keyboard.press("KeyK");
await frames(2);
const after = await saved();
check(after.traffic === before.traffic && after.components === before.components, `X / K do nothing with the debug tools off (${JSON.stringify({ before: [before.traffic, before.components], after: [after.traffic, after.components] })})`);
await page.keyboard.press("F6");
await frames(2);
const f6 = await saved();
check(f6.reducedFlashing === !before.reducedFlashing, `F6 (reduced flashing) still toggles (${before.reducedFlashing} → ${f6.reducedFlashing})`);
await page.keyboard.press("F6");

// ----- 3. The ⚙ button: shows on mouse move, hides when idle, turns the tools on -------------
await page.mouse.move(640, 300);
await page.mouse.move(660, 320);
await frames(3);
check((await visible(["#dev-toggle"])).length === 1, "⚙ shows while the mouse moves");
await shot("hud_gear_button.png");
await page.waitForTimeout(3500);
await frames(3);
check((await visible(["#dev-toggle"])).length === 0, "⚙ hides again after ~3 s without mouse movement");
await page.mouse.move(700, 300);
await frames(3);
await page.click("#dev-toggle");
await frames(3);
await settle();
st = await page.evaluate(() => ({ on: window.__wp.devSwitch.on, player: document.body.classList.contains("wp-player"), saved: window.__wp.menu.settings.developer }));
check(st.on && !st.player && st.saved, `⚙ turns the debug tools on and saves it (${JSON.stringify(st)})`);
dbg = await visible(["#controls", ".hud-main .hud-dbg", "canvas.silhouette", "#layer-row"]);
check(dbg.length === 4, `drawer, perf lines, silhouette and the layer row are visible (${dbg.join(", ")})`);

// ----- 4. Layer checkboxes and keys with the tools on -----------------------------------------
const box = '#layer-row input[data-layer="regions"]';
const r0 = (await saved()).regions ?? false;
await page.click(box);
await frames(2);
check(((await saved()).regions ?? false) === !r0, `layer checkbox toggles the layer (regions ${r0} → ${!r0})`);
// Keys typed into a focused form control are not game keys: leave the checkbox first.
await page.evaluate(() => document.activeElement?.blur());
await page.keyboard.press("KeyE");
await page.waitForTimeout(400);
check(((await saved()).regions ?? false) === r0 && (await page.isChecked(box)) === r0, "E (layer key) works with the tools on and the checkbox follows");

// ----- 5. Fold, the debug key, persistence ------------------------------------------------------
await page.click("#dev-fold");
await frames(2);
let fold = await page.evaluate(() => ({ folded: document.body.classList.contains("wp-dev-folded"), scenario: getComputedStyle(document.getElementById("sc-select")).display, head: getComputedStyle(document.querySelector("#controls .dev-head")).display }));
check(fold.folded && fold.head !== "none" && (await visible(["#sc-select"])).length === 0, `the drawer folds to its header (${JSON.stringify(fold)})`);
await shot("hud_debug_folded.png");
await page.click("#dev-fold");
await page.keyboard.press("Backquote");
await frames(3);
check(!(await page.evaluate(() => window.__wp.devSwitch.on)), "` turns the debug tools off");
await page.keyboard.press("Backquote");
await frames(3);
check(await page.evaluate(() => window.__wp.devSwitch.on), "` turns them on again");
await shot("hud_debug_on.png");
await page.goto(`${URL}?city=vesper_v2&dev=auto`);
await ready();
await frames(3);
check(await page.evaluate(() => window.__wp.devSwitch.on && !document.body.classList.contains("wp-player")), "the debug tools stay on after a reload (saved)");
await page.keyboard.press("Backquote");
await frames(3);
await page.goto(`${URL}?city=vesper_v2&dev=auto`);
await ready();
await frames(3);
check(await page.evaluate(() => !window.__wp.devSwitch.on && document.body.classList.contains("wp-player")), "…and off after the next reload once turned off");

// ----- 6. Help: the debug section only with the tools on ---------------------------------------
await page.keyboard.press("F1");
await frames(2);
const help = await visible([".help", ".help .help-dbg"]);
check(help.includes(".help") && !help.includes(".help .help-dbg"), `F1 help without the debug section in player mode (${help.join(", ")})`);
await page.keyboard.press("F1");

// ----- 7. Player layout: no overlaps (free drive and a race) ------------------------------------
for (const [w, h] of [[1280, 720], [1920, 1080]]) {
  await page.setViewportSize({ width: w, height: h });
  await settle();
  const bad = overlaps(await hudBoxes(page), w, h);
  check(bad.length === 0, `player free drive ${w}×${h}: no overlaps${bad.length ? " — " + bad.join("; ") : ""}`);
}
await page.evaluate(() => {
  const w = window.__wp;
  w.startRace(w.sim.raceList("lindwurm_4t", "Arcade")[0].id);
});
await page.waitForFunction(() => !!document.querySelector(".wp-race-hud"), null, { timeout: 30000 });
for (const [w, h] of [[1280, 720], [1920, 1080]]) {
  await page.setViewportSize({ width: w, height: h });
  await settle();
  const bad = overlaps(await hudBoxes(page), w, h);
  check(bad.length === 0, `player race ${w}×${h}: no overlaps${bad.length ? " — " + bad.join("; ") : ""}`);
  await shot(`hud_player_race_${w}.png`);
}
check((await visible([".wp-minimap"])).length === 0, "one minimap in a race (the race panel's)");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "all HUD checks passed");
process.exit(failures ? 1 : 0);
