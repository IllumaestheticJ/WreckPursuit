// Game menu + HUD layout check (headless Chromium, Playwright; playtest rows 67 / 68). Run by
// `npm run e2e` (run.mjs) at its end, or alone: `npm run e2e:menu` (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome, MENU_SHOTS=dir to write menu_*.png and layout_*.png.
//
// Checks: no HUD element overlaps another (bounding boxes) at 1280×720, 1366×768 and 1920×1080,
// developer panels shown and hidden, in a rainy night run (pursuit banner, weather line, feed)
// and a street race (race HUD); Esc pauses the sim and Esc resumes it; the menu is keyboard
// navigable (pause → settings → units → back); a setting survives a reload; gamepad Start opens
// the pause menu and the d-pad / B navigate it; Controls opens the rebinding panel and returns to
// the menu; ?menu=1 opens the title screen with the developer panels off and starts a chase;
// pause → Garage only mentions ending a career run during one; the race picker, the touge picker
// and the garage opened from the title close on Esc and return to the title.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";
import { hudBoxes, overlaps } from "./hud_layout.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5197;
const shots = process.env.MENU_SHOTS;
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
// Fake standard gamepad (as e2e/gamepad.mjs): `window.__pad.set({ buttons: {9: 1} })`.
await context.addInitScript(() => {
  const pad = {
    id: "WreckPursuit fake pad (STANDARD GAMEPAD)",
    index: 0,
    connected: true,
    mapping: "standard",
    timestamp: 0,
    axes: [0, 0, 0, 0],
    buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })),
  };
  Object.defineProperty(Navigator.prototype, "getGamepads", { configurable: true, value: () => [pad, null, null, null] });
  window.__pad = {
    set(buttons) {
      for (const [i, v] of Object.entries(buttons)) pad.buttons[i] = { pressed: v > 0.5, touched: v > 0, value: v };
      pad.timestamp++;
    },
  };
});
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));

const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const frames = (n) => page.evaluate((k) => new Promise((r) => { let i = 0; const f = () => (++i >= k ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }), n);
const tick = () => page.evaluate(() => window.__wp.sim.worldInfo()[0]);
const menu = () => page.evaluate(() => ({ screen: window.__wp.menu.screen, paused: window.__wp.app.paused, focus: document.activeElement?.id ?? "", visible: !document.getElementById("wp-game-menu").hidden }));
/** Wait until the layout pass ran twice more (it runs at most every 100 ms of wall time). */
const settle = async () => {
  const p0 = await page.evaluate(() => window.__wp.layout.passes);
  await page.waitForFunction((p) => window.__wp.layout.passes >= p + 2, p0, { timeout: 30000 });
  await frames(2);
};
const shot = async (name) => shots && (await page.screenshot({ path: join(shots, name) }));
const press = async (key, n = 1) => {
  for (let i = 0; i < n; i++) await page.keyboard.press(key);
  await frames(1);
};
const padTap = async (button) => {
  await page.evaluate((b) => window.__pad.set({ [b]: 1 }), button);
  await frames(3);
  await page.evaluate((b) => window.__pad.set({ [b]: 0 }), button);
  await frames(3);
};

// ----- 1. HUD layout (row 67) ----------------------------------------------------------------
const SIZES = [
  [1280, 720],
  [1366, 768],
  [1920, 1080],
];
/** Every HUD element that can show at once: banner on, a full event feed. */
const fillHud = () =>
  page.evaluate(() => {
    const b = document.getElementById("pursuit-banner");
    b.className = "";
    b.dataset.kind = "pursuit";
    b.innerHTML = "<b>PURSUIT</b><span>3 units on you</span>";
    const f = document.querySelector(".feed");
    if (f && !document.body.classList.contains("wp-player")) {
      f.style.display = "";
      f.innerHTML = "<b>events</b>" + Array.from({ length: 12 }, (_, i) => `<div>12.${i}s TrafficViolation #1 Speeding over_mps=12.4 limit 13.9</div>`).join("");
    }
  });
const layoutCheck = async (label) => {
  for (const [w, h] of SIZES) {
    await page.setViewportSize({ width: w, height: h });
    await settle();
    await fillHud();
    await settle();
    const boxes = await hudBoxes(page);
    const bad = overlaps(boxes, w, h);
    const names = [...new Set(boxes.map((b) => b.name))].join(", ");
    check(bad.length === 0, `layout ${label} ${w}×${h}: no overlaps among ${boxes.length} HUD boxes (${names})${bad.length ? " — " + bad.join("; ") : ""}`);
    await shot(`layout_${label.replace(/[^a-z0-9]+/gi, "_")}_${w}.png`);
  }
};
for (const dev of [1, 0]) {
  await page.goto(`${URL}?chase=rain&dev=${dev}`);
  await ready();
  await frames(4);
  const minimap = await page.evaluate(() => !!document.querySelector(".wp-minimap:not(.wp-hidden)"));
  check(minimap, `minimap shown (dev ${dev ? "on" : "off"})`);
  await layoutCheck(`chase dev ${dev ? "on" : "off"}`);
  await page.evaluate(() => {
    const w = window.__wp;
    w.startRace(w.sim.raceList("lindwurm_4t", "Arcade")[0].id);
  });
  await page.waitForFunction(() => !!document.querySelector(".wp-race-hud"), null, { timeout: 30000 });
  await layoutCheck(`race dev ${dev ? "on" : "off"}`);
}
await page.setViewportSize({ width: 1280, height: 720 });

// ----- 2. Pause menu: Esc pauses, Esc resumes (row 68) -----------------------------------------
await page.goto(`${URL}?dev=1`);
await ready();
await page.evaluate(() => localStorage.removeItem("wp.gameSettings.v1"));
await page.reload();
await ready();
await frames(3);
const t0 = await tick();
await frames(3);
check((await tick()) > t0, "sim runs before the menu");
await press("Escape");
let m = await menu();
const tp = await tick();
await page.waitForTimeout(400);
await frames(3);
const tp2 = await tick();
check(m.screen === "pause" && m.visible && m.paused && tp2 === tp, `Esc opens the pause menu and pauses the sim (screen ${m.screen}, tick ${tp} → ${tp2})`);
check(m.focus === "gm-resume", `Resume has the focus (${m.focus})`);
await shot("menu_pause.png");
// Driving keys do nothing while it is open.
await page.keyboard.down("F3");
await page.keyboard.up("F3");
const camBefore = await page.evaluate(() => window.__wp.rig.mode);
await press("Escape");
m = await menu();
const tr = await tick();
await frames(4);
check(m.screen === "closed" && !m.visible && !m.paused && (await tick()) > tr, `Esc closes it and the sim runs again (${m.screen}, paused ${m.paused})`);
check((await page.evaluate(() => window.__wp.rig.mode)) === camBefore, "a game key pressed in the menu did not reach the game");

// ----- 3. Keyboard navigation → a setting, then a reload --------------------------------------
await press("Escape");
await press("ArrowDown");
const afterDown = (await menu()).focus;
await press("ArrowUp");
check(afterDown === "gm-restart" && (await menu()).focus === "gm-resume", `↓ / ↑ move the focus (${afterDown})`);
for (let i = 0; i < 8 && (await menu()).focus !== "gm-settings"; i++) await press("ArrowDown");
await press("Enter");
m = await menu();
check(m.screen === "settings", `Enter on Settings opens settings (${m.screen}, focus ${m.focus})`);
for (let i = 0; i < 40 && (await menu()).focus !== "gm-units"; i++) await press("ArrowDown");
check((await menu()).focus === "gm-units", "↓ reaches Speed units");
await press("ArrowRight");
const s1 = await page.evaluate(() => window.__wp.menu.settings.units);
check(s1 === "mph", `→ switches the units (${s1})`);
// Volume slider by keyboard: master −2 steps applies to the audio mix.
for (let i = 0; i < 40 && (await menu()).focus !== "gm-vol-master"; i++) await press("ArrowUp");
const v0 = await page.evaluate(() => window.__wpAudio.mix().master);
await press("ArrowLeft", 2);
const v1 = await page.evaluate(() => [window.__wp.menu.settings.volumes.master, window.__wpAudio.mix().master]);
check(Math.abs(v1[0] - (v0 - 0.1)) < 1e-6 && v1[1] === v1[0], `← lowers the master volume and it reaches the audio mix (${v0} → ${v1.join(" / ")})`);
// TCS / ABS are Off / Half / Full cycles (owner rows 104/105): clicking TCS steps Off → Half and
// reaches the sim; the HUD tag shows the level.
const tcs0 = await page.evaluate(() => window.__wp.menu.settings.assists.tcs);
await page.click("#gm-tcs");
const tcs1 = await page.evaluate(() => ({ menu: window.__wp.menu.settings.assists.tcs, sim: window.__wp.sim.assists(window.__wp.app.player).tcs, sel: document.querySelector('#assist-row select[data-a="tcs"]')?.value }));
check(tcs0 === "Off" && tcs1.menu === "Half" && tcs1.sim === "Half" && tcs1.sel === "Half", `TCS Off → Half in the menu reaches the sim and the assist row (${tcs0} → ${JSON.stringify(tcs1)})`);
await page.click("#gm-abs");
const abs1 = await page.evaluate(() => ({ menu: window.__wp.menu.settings.assists.abs, sim: window.__wp.sim.assists(window.__wp.app.player).abs }));
check(abs1.menu === "Off" && abs1.sim === "Off", `ABS Full → Off (cycles round) (${JSON.stringify(abs1)})`);
await page.click("#gm-abs");
await page.click("#gm-tcs");
await page.click("#gm-tcs");
const back = await page.evaluate(() => [window.__wp.menu.settings.assists.abs, window.__wp.menu.settings.assists.tcs]);
check(back[0] === "Half" && back[1] === "Off", `cycles step through every level (${back})`);
// Park the mouse: a pointer left over the menu would take the focus of whatever renders there.
await page.mouse.move(0, 0);
const cstr = await page.evaluate(() => document.getElementById("gm-cstr")?.disabled);
check(cstr === false, "countersteer slider enabled (wasm set_countersteer, owner row 57)");
await shot("menu_settings.png");
await press("Escape");
m = await menu();
check(m.screen === "pause" && m.focus === "gm-settings", `Esc goes back to the pause menu (${m.screen}, focus ${m.focus})`);
await press("Escape");
await frames(8);
const speedo = await page.evaluate(() => document.querySelector(".hud-main .speed small")?.textContent?.trim());
check(speedo === "mph", `speedo shows the new units (${speedo})`);
await page.reload();
await ready();
await frames(8);
const kept = await page.evaluate(() => ({ units: window.__wp.menu.settings.units, master: window.__wp.menu.settings.volumes.master, speedo: document.querySelector(".hud-main .speed small")?.textContent?.trim() }));
check(kept.units === "mph" && Math.abs(kept.master - (v0 - 0.1)) < 1e-6 && kept.speedo === "mph", `settings survive a reload (${JSON.stringify(kept)})`);

// ----- 4. Gamepad: Start opens, d-pad moves, B resumes ----------------------------------------
await padTap(9);
m = await menu();
check(m.screen === "pause" && m.paused, `gamepad Start opens the pause menu (${m.screen})`);
await padTap(13);
const padFocus = (await menu()).focus;
check(padFocus === "gm-restart", `d-pad ↓ moves the focus (${padFocus})`);
await padTap(1);
m = await menu();
check(m.screen === "closed" && !m.paused, `B resumes (${m.screen})`);

// ----- 5. Controls opens the rebinding panel and comes back ------------------------------------
await press("Escape");
for (let i = 0; i < 8 && (await menu()).focus !== "gm-controls"; i++) await press("ArrowDown");
await press("Enter");
const ctl = await page.evaluate(() => ({ open: window.__wp.controls.isOpen, menuHidden: document.getElementById("wp-game-menu").hidden, paused: window.__wp.app.paused }));
check(ctl.open && ctl.menuHidden && ctl.paused, `Controls opens the rebinding panel over the paused game (${JSON.stringify(ctl)})`);
await press("Escape");
await frames(4);
m = await menu();
check(!(await page.evaluate(() => window.__wp.controls.isOpen)) && m.screen === "pause" && m.visible && m.paused, `closing it returns to the pause menu (${m.screen}, visible ${m.visible})`);
await press("Escape");

// ----- 6. Title screen (?menu=1, the demo's start page) ----------------------------------------
await page.evaluate(() => localStorage.removeItem("wp.gameSettings.v1"));
await page.goto(`${URL}?menu=1`);
await ready();
await frames(3);
m = await menu();
const player = await page.evaluate(() => ({ player: document.body.classList.contains("wp-player"), devPanel: getComputedStyle(document.getElementById("controls")).display }));
check(m.screen === "main" && m.visible && m.paused && m.focus === "gm-play-chase", `?menu=1 opens the main menu, paused, Night run focused (${m.screen}, ${m.focus})`);
check(player.player && player.devPanel === "none", `developer panels off by default there (${JSON.stringify(player)})`);
await shot("menu_main.png");
const keys = await page.evaluate(() => document.querySelector('[data-testid="gm-keys"]')?.textContent ?? "");
const live = await page.evaluate(() => {
  const b = window.__wp.bindings();
  return { cam: b.keys.cameraCycle[0], clutch: b.keys.clutch[0] };
});
check(/F3 · R · M/.test(keys) && /shift up/.test(keys) && !/^C · /.test(keys), `title key hints follow the bindings (camera ${live.cam}, clutch ${live.clutch}): "${keys}"`);
await press("Enter");
await frames(4);
m = await menu();
const sc = await page.evaluate(() => window.__wp.sim.scenario().name);
check(m.screen === "closed" && !m.paused && sc === "night_run", `Enter starts the night run (${sc}, menu ${m.screen})`);

// ----- 7. Pause outside a career run; pickers and the garage return to the title -----------------
await press("Escape");
const garageHint = await page.evaluate(() => document.querySelector("#gm-garage small")?.textContent ?? "(no Garage item)");
check(!/ends the current career run/.test(garageHint), `pause → Garage outside a career run does not claim to end one ("${garageHint.trim()}")`);
await page.click("#gm-quit");
await frames(3);
check((await menu()).screen === "main", "Quit to main menu shows the title");
const picker = () => page.evaluate(() => ({ races: !!window.__wp.raceView.cards, touge: !!document.querySelector('[data-testid="touge-picker"]') }));
for (const [item, label] of [
  ["#gm-play-races", "Street races picker"],
  ["#gm-play-touge", "Touge picker"],
  ["#gm-play-garage", "garage"],
]) {
  await page.click(item);
  await frames(4);
  const before = { ...(await menu()), ...(await picker()), garage: await page.evaluate(() => window.__wp.garage.open) };
  const shown = item === "#gm-play-races" ? before.races : item === "#gm-play-garage" ? before.garage : before.touge;
  await press("Escape");
  await frames(4);
  m = await menu();
  const after = { ...(await picker()), garage: await page.evaluate(() => window.__wp.garage.open) };
  check(
    shown && m.screen === "main" && m.visible && !after.races && !after.touge && !after.garage,
    `${label} from the title: Esc closes it and returns to the title (shown ${shown}, screen ${m.screen}, pickers ${after.races}/${after.touge}, garage ${after.garage})`,
  );
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "all menu checks passed");
process.exit(failures ? 1 : 0);
