// Touch controls check (owner request row 71, docs/integration/touch.md). Headless Chromium
// emulating a phone (Playwright hasTouch + isMobile, landscape viewports) and a desktop.
// Run by `npm run e2e` (run.mjs) at its end, or alone: `npm run e2e:touch` (needs `npm run wasm`).
// Env: CHROMIUM=/path/to/chrome, TOUCH_SHOTS=dir to write phone screenshots.
//
// Real touch events go through CDP Input.dispatchTouchEvent (multi-touch: a pedal finger and a
// steering finger at once), so the page sees pointer events with pointerType "touch".
// Checks: the controls appear on a touch device (and the touch graphics profile is used); a
// swipe up the throttle pedal accelerates; steering works and is analogue; release lets go of
// everything; source → Touch with the touch assists (auto gears, TCS, steer help); edge buttons
// (camera, map, pause); tilt falls back to buttons when no orientation events arrive and steers
// when they do; HUD + controls do not overlap at 640×360, 740×360 and 932×430 in the city chase
// (dev panels hidden); portrait shows the rotate hint; a desktop shows no touch UI.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5196;
const shots = process.env.TOUCH_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe", detached: true });
process.on("exit", () => {
  try {
    process.kill(-server.pid, "SIGTERM");
  } catch {}
});
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
const errors = [];
const watch = (page) => {
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
};

// ---------- phone ----------
const phone = await browser.newContext({ viewport: { width: 740, height: 360 }, hasTouch: true, isMobile: true, deviceScaleFactor: 3 });
const page = await phone.newPage();
watch(page);
const cdp = await phone.newCDPSession(page);
const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const frames = (n) => page.evaluate((k) => new Promise((r) => { let i = 0; const f = () => (++i >= k ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }), n);
const speed = () => page.evaluate(() => window.__wp.sim.states()[14]);
const input = () => page.evaluate(() => ({ ...window.__wp.app.input }));
const rect = (sel) => page.evaluate((s) => {
  const e = document.querySelector(s);
  if (!e) return null;
  const r = e.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
}, sel);
// Touch points: { id, x, y }. Every call sends the full list of fingers down.
const touch = (type, points) =>
  cdp.send("Input.dispatchTouchEvent", { type, touchPoints: points.map((p) => ({ x: p.x, y: p.y, id: p.id, radiusX: 8, radiusY: 8, force: 1 })) });

await page.goto(URL);
await ready();
await page.evaluate(() => localStorage.removeItem("wp.touch.v1"));
await frames(5);

// 1. Controls appear on a touch device; the touch graphics profile is in use.
const shown = await page.evaluate(() => ({
  root: !document.getElementById("wp-touch").hidden,
  cls: document.documentElement.classList.contains("wp-touch"),
  q: window.__wp.scene.quality.label(),
  ratio: window.__wp.scene.renderer.getPixelRatio(),
  traffic: window.__wp.lod.trafficDrawM,
  devHidden: getComputedStyle(document.getElementById("controls")).display === "none",
}));
check(shown.root && shown.cls, `touch controls shown on a touch device`);
check(/touch/.test(shown.q) && shown.ratio <= 1 && shown.traffic <= 300, `touch graphics profile (${shown.q}, pixel ratio ${shown.ratio}, traffic draw ${shown.traffic} m)`);
check(shown.devHidden, `dev panels hidden on a small screen`);

// 2. Swipe up the throttle pedal: accelerates; source Touch; touch assists.
const thr = await rect("#wp-touch .throttle");
const sx = thr.x + thr.w / 2;
await touch("touchStart", [{ id: 1, x: sx, y: thr.y + thr.h - 6 }]);
await frames(2);
const low = await input();
for (let i = 1; i <= 6; i++) {
  await touch("touchMove", [{ id: 1, x: sx, y: thr.y + thr.h - 6 - (i * (thr.h - 12)) / 6 }]);
  await frames(1);
}
const high = await input();
check(low.throttle > 0.25 && low.throttle < 0.45 && high.throttle > 0.95, `throttle is analogue by finger height (bottom ${low.throttle.toFixed(2)}, swiped to top ${high.throttle.toFixed(2)})`);
await page.waitForTimeout(2500);
const v = await speed();
const st = await page.evaluate(() => ({ source: window.__wp.app.source, a: window.__wp.sim.assists(window.__wp.app.player) }));
check(v > 1.5, `swipe on the throttle accelerates (${v.toFixed(2)} m/s)`);
check(st.source === "Touch" && st.a.auto_gears && st.a.tcs && st.a.countersteer_help >= 0.5 && st.a.steer_speed_sensitivity >= 0.9, `source → Touch with auto gears + TCS + steer help (${JSON.stringify({ auto: st.a.auto_gears, tcs: st.a.tcs, cs: st.a.countersteer_help, ss: st.a.steer_speed_sensitivity })})`);

// 3. Steering (second finger while the throttle is held): outer right = full, analogue drag.
const steer = await rect("#wp-touch .steer");
const cy = steer.y + steer.h / 2;
await touch("touchMove", [{ id: 1, x: sx, y: thr.y + 10 }]);
await touch("touchStart", [{ id: 1, x: sx, y: thr.y + 10 }, { id: 2, x: steer.x + steer.w - 6, y: cy }]);
await page.waitForTimeout(400);
const full = await input();
const app1 = await page.evaluate(() => window.__wp.sim.telemetry(window.__wp.app.player)[11]);
// Drag to a quarter of the way left of centre: partial left.
await touch("touchMove", [{ id: 1, x: sx, y: thr.y + 10 }, { id: 2, x: steer.x + steer.w * 0.35, y: cy }]);
await page.waitForTimeout(400);
const part = await input();
check(full.steer > 0.99 && full.throttle > 0.9 && app1 > 0.2, `steer right with a second finger while on the throttle (input ${full.steer.toFixed(2)}, sim applied ${app1.toFixed(2)})`);
check(part.steer < -0.2 && part.steer > -0.8, `dragging across the buttons steers analogue (${part.steer.toFixed(2)})`);

// 4. Release: everything back to 0.
await touch("touchEnd", []);
await frames(3);
const rel = await input();
check(rel.throttle === 0 && rel.brake === 0 && rel.handbrake === 0 && Math.abs(rel.steer) < 0.2, `release lets go (thr ${rel.throttle}, brk ${rel.brake}, steer ${rel.steer.toFixed(2)})`);
await frames(10);
// Brake + handbrake.
const brk = await rect("#wp-touch .brake");
const hb = await rect("#wp-touch .hbrake");
await touch("touchStart", [{ id: 3, x: brk.x + brk.w / 2, y: brk.y + 8 }, { id: 4, x: hb.x + hb.w / 2, y: hb.y + hb.h / 2 }]);
await frames(3);
const bh = await input();
await touch("touchEnd", []);
await frames(2);
check(bh.brake > 0.9 && bh.handbrake === 1, `brake pedal + handbrake (brake ${bh.brake.toFixed(2)}, handbrake ${bh.handbrake})`);

// 5. Edge buttons: camera cycles; map opens (driving controls hide) and closes; pause.
const tap = async (sel) => {
  const r = await rect(sel);
  await touch("touchStart", [{ id: 9, x: r.x + r.w / 2, y: r.y + r.h / 2 }]);
  await touch("touchEnd", []);
  await frames(3);
};
const cam0 = await page.evaluate(() => window.__wp.rig.mode);
await tap('#wp-touch [data-touch="cameraCycle"]');
const cam1 = await page.evaluate(() => window.__wp.rig.mode);
check(cam1 !== cam0, `camera button cycles the camera (${cam0} → ${cam1})`);
await page.evaluate(() => window.__wp.setCamera("chase"));
await page.evaluate(() => window.__wp.startCityChase(undefined, "Arcade"));
await page.waitForTimeout(1500);
// Phones draw fewer parked cars (350 m, compacted proxies) but every prop near the camera
// still has exactly one drawn instance (no invisible collisions).
const ps = await page.evaluate(() => {
  const s = window.__wp.props.stats(window.__wp.rig.camera, 150);
  return { inRange: s.inRange, drawn: s.drawnInRange, proxies: s.drawnProxy, parked: s.parked };
});
check(ps.inRange > 0 && ps.drawn === ps.inRange && ps.proxies < ps.parked, `props within 150 m all drawn (${ps.drawn}/${ps.inRange}), far parked cars not instanced (${ps.proxies} of ${ps.parked} proxies)`);
await tap('#wp-touch [data-touch="map"]');
const mapOpen = await page.evaluate(() => ({ open: window.__wp.map.isOpen, noDrive: document.getElementById("wp-touch").classList.contains("no-drive") }));
await tap('#wp-touch [data-touch="map"]');
const mapClosed = await page.evaluate(() => window.__wp.map.isOpen);
check(mapOpen.open && mapOpen.noDrive && !mapClosed, `map button opens the map (driving controls hidden) and closes it`);
await tap('#wp-touch [data-touch="pause"]');
const paused = await page.evaluate(() => window.__wp.app.paused);
await tap('#wp-touch [data-touch="pause"]');
const unpaused = await page.evaluate(() => !window.__wp.app.paused);
check(paused && unpaused, `pause button pauses and resumes`);

// 6. Tilt: no orientation events → falls back to buttons; with events it steers.
const fell = await page.evaluate(async () => {
  const m = await window.__wp.setTouchSteerMode("tilt");
  await new Promise((r) => setTimeout(r, 1800));
  return { m, after: window.__wp.touchSteerMode() };
});
check(fell.m === "tilt" && fell.after === "buttons", `tilt with no orientation events falls back to buttons (${fell.m} → ${fell.after})`);
const tilt = await page.evaluate(async () => {
  await window.__wp.setTouchSteerMode("tilt");
  const send = (beta, gamma) => window.dispatchEvent(Object.assign(new Event("deviceorientation"), { alpha: 0, beta, gamma, absolute: false }));
  // Phone upright in landscape (screen angle from the page), tipped like a wheel by ±25°.
  const ang = screen.orientation?.angle ?? 0;
  const pose = (deg) => {
    // Device "down" for a wheel angle `deg` in screen space, inverted to beta / gamma.
    const t = (ang * Math.PI) / 180;
    const w = (deg * Math.PI) / 180;
    const right = Math.sin(w);
    const down = Math.cos(w);
    const dx = right * Math.cos(t) - down * Math.sin(t);
    const dy = -right * Math.sin(t) - down * Math.cos(t);
    const b = Math.asin(-dy);
    const cb = Math.cos(b);
    const g = Math.abs(cb) < 1e-6 ? 0 : Math.asin(Math.max(-1, Math.min(1, dx / cb)));
    return [(b * 180) / Math.PI, (g * 180) / Math.PI];
  };
  const out = {};
  for (const [k, deg] of [["right", 25], ["left", -25], ["level", 0]]) {
    for (let i = 0; i < 20; i++) {
      send(...pose(deg));
      await new Promise((r) => requestAnimationFrame(r));
    }
    out[k] = window.__wp.app.input.steer;
  }
  out.mode = window.__wp.touchSteerMode();
  out.cls = document.getElementById("wp-touch").classList.contains("tilt");
  await window.__wp.setTouchSteerMode("buttons");
  return out;
});
check(tilt.mode === "tilt" && tilt.cls && tilt.right > 0.6 && tilt.left < -0.6 && Math.abs(tilt.level) < 0.05, `tilt steers (right ${tilt.right.toFixed(2)}, left ${tilt.left.toFixed(2)}, level ${tilt.level.toFixed(2)})`);

// 7. Layout: HUD and controls inside the screen and not overlapping, at three phone sizes, in
// the city chase with the pursuit banner showing.
const layout = () =>
  page.evaluate(() => {
    const banner = document.getElementById("pursuit-banner");
    banner.classList.remove("hidden");
    banner.dataset.kind = "pursuit";
    banner.innerHTML = "<b>PURSUIT</b><span>lose them</span>";
    const sel = {
      speed: ".hud-main",
      minimap: ".wp-minimap",
      chase: ".wp-chase-col",
      banner: "#pursuit-banner",
      steer: "#wp-touch .steer",
      brake: "#wp-touch .brake",
      throttle: "#wp-touch .throttle",
      handbrake: "#wp-touch .hbrake",
      top: "#wp-touch .row.top",
      side: "#wp-touch .row.side",
    };
    const boxes = {};
    for (const [k, s] of Object.entries(sel)) {
      const e = document.querySelector(s);
      if (!e) continue;
      const r = e.getBoundingClientRect();
      if (r.width && r.height && getComputedStyle(e).display !== "none") boxes[k] = [r.left, r.top, r.right, r.bottom];
    }
    const W = innerWidth;
    const H = innerHeight;
    const out = [];
    const ks = Object.keys(boxes);
    for (const k of ks) {
      const [l, t, r, b] = boxes[k];
      if (l < 0 || t < 0 || r > W + 0.5 || b > H + 0.5) out.push(`${k} off-screen`);
    }
    for (let i = 0; i < ks.length; i++)
      for (let j = i + 1; j < ks.length; j++) {
        const a = boxes[ks[i]];
        const c = boxes[ks[j]];
        if (a[0] < c[2] - 0.5 && c[0] < a[2] - 0.5 && a[1] < c[3] - 0.5 && c[1] < a[3] - 0.5) out.push(`${ks[i]} × ${ks[j]}`);
      }
    return { problems: out, seen: ks };
  });
for (const [w, h] of [[640, 360], [740, 360], [932, 430]]) {
  await page.setViewportSize({ width: w, height: h });
  await frames(20);
  const l = await layout();
  if (shots) await page.screenshot({ path: join(shots, `touch_${w}x${h}.png`) });
  const need = ["speed", "minimap", "chase", "banner", "steer", "brake", "throttle", "handbrake", "top", "side"];
  const missing = need.filter((k) => !l.seen.includes(k));
  check(l.problems.length === 0 && missing.length === 0, `layout ${w}×${h}: no overlaps / off-screen${l.problems.length ? ": " + l.problems.join(", ") : ""}${missing.length ? " missing " + missing.join(", ") : ""}`);
}

// 8. Portrait: rotate hint.
await page.setViewportSize({ width: 360, height: 640 });
await frames(5);
const rot = await page.evaluate(() => getComputedStyle(document.getElementById("wp-rotate")).display);
if (shots) await page.screenshot({ path: join(shots, "touch_portrait.png") });
check(rot === "flex", `portrait shows the rotate hint (${rot})`);
await phone.close();

// ---------- desktop ----------
const desk = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const dp = await desk.newPage();
watch(dp);
await dp.goto(URL);
await dp.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await dp.mouse.click(640, 360);
await dp.keyboard.down("KeyW");
await dp.waitForTimeout(800);
await dp.keyboard.up("KeyW");
const d = await dp.evaluate(() => ({
  hidden: document.getElementById("wp-touch")?.hidden ?? true,
  cls: document.documentElement.classList.contains("wp-touch"),
  q: window.__wp.scene.quality.label(),
  source: window.__wp.app.source,
}));
check(d.hidden && !d.cls && !/touch/.test(d.q) && d.source === "Keyboard", `desktop shows no touch UI (${d.q}, source ${d.source})`);
await desk.close();

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} touch check(s) failed` : "all touch checks passed");
process.exit(failures ? 1 : 0);
