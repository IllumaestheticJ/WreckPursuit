// e2e: street racers (wp_wasm::street, src/ui/street.ts) in headless Chromium.
// Usage: node e2e/street.mjs (needs the wasm package). Env: SHOTS=dir for screenshots,
// CITY=vesper_v2|procedural_12x12 (default vesper_v2), CHROMIUM=/path/to/chrome.
// Checks, by day (13:00 free drive; keyboard + mouse) and by night (23:00 free drive; gamepad):
// the roster has 13 racers in 4 levels (colours from street_status().levels), 8 out by day and 13
// at night (row 152; the rest in the lockup, off the map), drawn as level-coloured markers on the UI kit's
// minimap and full city map (with a legend group) and wearing their level colour on their car;
// honking (the rebindable horn: Q / pad L3) right behind a racer opens the map as the finish picker;
// picking B on the map (mouse click / pad crosshair + A) previews a raceable route; the bet
// slider / pad LB-RB sets the stake; by day extra waypoints are added, re-ordered and removed on the
// map (row 151: numbered flags, the route through them, gates in the world); a wanted player cannot
// challenge (row 150); Race (Enter / pad Menu) pays it, puts both cars side by side at rest
// (row 150) and counts down with the
// race panel up and the route on the minimap; the street UI hides in photo mode, under the
// game menu and during a crash replay; reaching B wins even money and shows the result. Roaming
// racers are fleet-drawn past ~40 m and get their hero view near the camera; the opponent keeps
// its view while racing. (Draw calls / triangles: e2e/perf_check.mjs, night run incl. racers.) Then the late-night night run
// has its racers, and on touch (?touch=1) the 📯 button challenges and the picker takes taps.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5201;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
const CITY = process.env.CITY ?? "vesper_v2";
const CAR = "lindwurm_4t";
if (shots) mkdirSync(shots, { recursive: true });
// Own process group: the exit handler stops vite itself, not just npx (a leftover server on
// the port would serve the next run stale code).
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe", detached: true });
process.on("exit", () => {
  try {
    process.kill(-server.pid, "SIGTERM");
  } catch {}
});
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
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
// A fake standard-mapping gamepad (as e2e/gamepad.mjs), disconnected until the night run.
const fakePad = () => {
  const pad = {
    id: "fake pad (STANDARD GAMEPAD)",
    index: 0,
    connected: false,
    mapping: "standard",
    timestamp: 0,
    axes: [0, 0, 0, 0],
    buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })),
  };
  Object.defineProperty(Navigator.prototype, "getGamepads", { configurable: true, value: () => [pad.connected ? pad : null, null, null, null] });
  window.__pad = {
    set({ axes = {}, buttons = {} } = {}) {
      for (const [i, v] of Object.entries(axes)) pad.axes[i] = v;
      for (const [i, v] of Object.entries(buttons)) pad.buttons[i] = { pressed: v > 0.5, touched: v > 0, value: v };
      pad.timestamp++;
    },
    connect(on) {
      pad.connected = on;
      const ev = new Event(on ? "gamepadconnected" : "gamepaddisconnected");
      ev.gamepad = pad;
      window.dispatchEvent(ev);
    },
  };
};
const context = await browser.newContext({ viewport: { width: 1400, height: 860 } });
await context.addInitScript(fakePad);
const page = await context.newPage();
const errors = [];
const watch = (p) => {
  p.on("pageerror", (e) => errors.push(String(e)));
  p.on("console", (m) => m.type() === "error" && errors.push(m.text()));
};
watch(page);
const shot = async (p, name) => shots && p.screenshot({ path: join(shots, name) });
const wait = (p, ms) => p.waitForTimeout(ms);
const status = (p) => p.evaluate(() => window.__wp.sim.streetStatus());
const mapStats = (p) => p.evaluate(() => window.__wp.map.hud.stats());
const visible = (p, sel) =>
  p.evaluate((s) => {
    const el = document.querySelector(s);
    if (!el || el.closest(".hidden, [hidden]")) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden" && getComputedStyle(document.getElementById("overlay")).display !== "none";
  }, sel);
/** Frames rendered (slow headless frames: wait on the frame loop, not wall time). */
const frames = (p, n) =>
  p.evaluate(
    (n) =>
      new Promise((done) => {
        let k = 0;
        const f = () => (++k > n ? done() : requestAnimationFrame(f));
        requestAnimationFrame(f);
      }),
    n,
  );

await page.goto(`${URL}?city=${CITY}`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
// These races are quick play (practice cash): untick the career box, or a quick-play
// session takes no bets (row 169: with it ticked, bets must land in the career).
const untickCareer = (p) => p.evaluate(() => (document.getElementById("career").checked = false));
await untickCareer(page);

// Sim cost without street racers, then with.
const simMs = async () => {
  await wait(page, 6000);
  return page.evaluate(() => window.__wp.app.perf.simMs);
};
await page.evaluate((car) => {
  window.__wp.sim.setStreetRacers(false);
  window.__wp.startFreeDrive(car, "Arcade", true, 13);
}, CAR);
const off = await simMs();
check((await status(page)) === null, "no street racers when switched off");

/** Teleport the player 10 m behind racer `e` (it keeps moving). */
const placeBehind = (p, e) =>
  p.evaluate((e) => {
    const wp = window.__wp;
    const r = wp.sim.streetRacers();
    for (let o = 0; o < r.length; o += 6) {
      if (r[o] !== e) continue;
      const [x, y, yaw] = [r[o + 2], r[o + 3], r[o + 4]];
      wp.sim.debugPlaceVehicle(wp.app.player, x + Math.sin(yaw) * 10, y - Math.cos(yaw) * 10, yaw);
    }
  }, e);

/** World points ~0.5–1.5 km from the player whose preview is raceable and that sit on the open
 * part of the full map (not under the legend / challenge card). */
const raceablePoint = (p) =>
  p.evaluate(() => {
    const wp = window.__wp;
    const st = wp.sim.states();
    const [px, py] = [st[1], st[2]];
    const panel = document.querySelector(".wp-map-host").getBoundingClientRect();
    const legend = document.querySelector(".wp-map-legend").getBoundingClientRect();
    for (const d of [700, 1000, 500, 1400]) {
      for (let k = 0; k < 12; k++) {
        const a = (k / 12) * Math.PI * 2;
        const [x, y] = [px + Math.cos(a) * d, py + Math.sin(a) * d];
        let pv = null;
        try {
          pv = wp.sim.streetPreview(x, y);
        } catch {
          continue;
        }
        if (!pv.ok) continue;
        const c = wp.map.hud.clientAt(x, y);
        if (!c || c[0] < panel.right + 10 || c[0] > legend.left - 10 || c[1] < 60 || c[1] > innerHeight - 20) continue;
        return { x, y, cx: c[0], cy: c[1] };
      }
    }
    return null;
  });

/** Street racers' draw modes (render/lod.ts states): hero views, far ones (> 40 m from the
 * camera) and whether any far one is still on its view; `target`'s mode. */
const lodOf = (p, target = -1) =>
  p.evaluate((target) => {
    const wp = window.__wp;
    const ids = new Set(wp.sim.streetStatus().roster.map((r) => r.entity));
    const modes = {};
    let views = 0,
      far = 0,
      farOnView = 0,
      t = null,
      td = -1;
    for (const s of wp.lod.states) {
      if (!ids.has(s.entity) || !s.key.startsWith("v")) continue;
      modes[s.mode] = (modes[s.mode] ?? 0) + 1;
      if (s.mode === "view") views++;
      if (s.dist > 41) {
        far++;
        if (s.mode === "view" && s.entity !== target) farOnView++;
      }
      if (s.entity === target) [t, td] = [s.mode, s.dist];
    }
    return { views, far, farOnView, modes, target: t, targetDist: td };
  }, target);

/** One street race: challenge, pick B, bet, countdown, finish. `pad`: gamepad instead of
 * keyboard / mouse. */
async function race(label, pad) {
  const st = await status(page);
  const night = label === "night";
  const N = st?.roster.filter((r) => r.mode !== "OffDuty").length ?? 0;
  check(st?.roster.length === 13, `${label}: 13 street racers in the roster (${st?.roster.length})`);
  check(N === (night ? 13 : 8), `${label}: ${night ? 13 : 8} out (${N})`);
  check(st && st.levels.length === 4 && new Set(st.levels.map((l) => l.colour)).size === 4 && new Set(st.roster.map((r) => r.level)).size === 4, `${label}: 4 levels, 4 colours (${st?.levels.map((l) => l.name).join(", ")})`);
  check(st && st.roster.every((r) => st.levels.find((l) => l.level === r.level)?.colour === r.colour), `${label}: every racer in its level's colour`);
  // Map markers in level colours: minimap (always on) and the full map.
  await page.waitForFunction((n) => window.__wp.map.hud.stats().minimap.markers === n, N, { timeout: 30000 }).catch(() => {});
  check((await mapStats(page)).minimap.markers === N, `${label}: ${N} racer markers on the minimap`);
  // LOD: roaming racers away from the camera are fleet-drawn (level colour, own model).
  const lod = await lodOf(page);
  check(lod.far > 0 && lod.farOnView === 0, `${label}: racers beyond 40 m drawn by the fleet (${lod.far} far, ${lod.farOnView} of them on views; ${lod.views} hero views, modes ${JSON.stringify(lod.modes)})`);
  // …and a roaming racer next to the camera gets its hero view (VIEW_DRAW_M ~30 m), handed
  // back to the fleet once it is past ~40 m (VIEW_LOD_BAND_M of hysteresis).
  const near = st.roster[1];
  let nearMode = null;
  for (let i = 0; i < 40 && nearMode !== "view"; i++) {
    await placeBehind(page, near.entity);
    await wait(page, 250);
    nearMode = (await lodOf(page, near.entity)).target;
  }
  const nl = await lodOf(page, near.entity);
  check(nearMode === "view", `${label}: a roaming racer near the camera gets its hero view (${nl.target} at ${Math.round(nl.targetDist)} m)`);
  await page.evaluate((e) => {
    const wp = window.__wp;
    const r = wp.sim.streetRacers();
    for (let o = 0; o < r.length; o += 6) {
      if (r[o] !== e) continue;
      const [x, y, yaw] = [r[o + 2], r[o + 3], r[o + 4]];
      wp.sim.debugPlaceVehicle(wp.app.player, x - Math.sin(yaw) * 150, y + Math.cos(yaw) * 150, yaw);
    }
  }, near.entity);
  await page.waitForFunction((e) => window.__wp.lod.states.some((s) => s.entity === e && s.key.startsWith("v") && s.dist > 45 && s.mode !== "view"), near.entity, { timeout: 20000 }).catch(() => {});
  const fl = await lodOf(page, near.entity);
  check(fl.target !== "view" && fl.targetDist > 40, `${label}: …and goes back to the fleet when the camera leaves (${fl.target} at ${Math.round(fl.targetDist)} m)`);
  // Their cars wear the level colour (VehicleView livery, every art style).
  const paints = await page.evaluate(() => {
    const wp = window.__wp;
    const st = wp.sim.streetStatus();
    return st.roster.map((r) => {
      const v = wp.app.views.get(r.entity);
      return { want: r.colour.slice(1).toLowerCase(), got: v?.greyPaint.getHexString(), livery: v?.livery, body: v?.box.material.color.getHexString() };
    });
  });
  const style = await page.evaluate(() => window.__wp.style.id);
  check(
    paints.every((p) => p.got === p.want && p.livery),
    `${label}: every racer's car wears its level colour (${style}: ${paints.map((p) => p.got).join(" ")})`,
  );
  check(style !== "lowpoly" || paints.every((p) => p.body === p.want), `${label}: low-poly body paint keeps the level colour`);

  // The full map (the map binding: M / pad D-pad →) with the racers, legend group and roster.
  await page.keyboard.press("KeyM");
  await frames(page, 3);
  const ms = await mapStats(page);
  check(ms.open && ms.full.markers >= 1, `${label}: full city map shows racer markers (${ms.full.markers})`);
  const legendRows = await page.evaluate(() => document.querySelectorAll(".wp-map-legend-host .wp-map-legend-row canvas").length);
  check(legendRows === st.levels.length, `${label}: map legend lists the ${st.levels.length} levels (${legendRows})`);
  check(await visible(page, ".wp-map-host"), `${label}: roster card on the map`);
  await shot(page, `street_${label}_1_citymap.png`);
  await page.keyboard.press("KeyM");
  await frames(page, 2);

  // Honk next to the level-4 racer (roster index 5).
  const target = st.roster[5];
  const hornKey = await page.evaluate(() => window.__wp.bindings().keys.horn[0]);
  if (pad) await page.evaluate(() => window.__pad.set({ buttons: { 10: 1 } }));
  else await page.keyboard.down(hornKey);
  for (let i = 0; i < 40; i++) {
    await placeBehind(page, target.entity);
    await wait(page, 250);
    if ((await status(page))?.offer) break;
  }
  if (pad) await page.evaluate(() => window.__pad.set({ buttons: { 10: 0 } }));
  else await page.keyboard.up(hornKey);
  await page.waitForFunction(() => window.__wp.map.hud.stats().picking && window.__wp.map.isOpen, null, { timeout: 20000 }).catch(() => {});
  const offer = (await status(page))?.offer;
  check(offer?.entity === target.entity, `${label}: honk (${pad ? "pad L3" : hornKey}) challenged ${target.name} (Lv ${target.level})`);
  check((await mapStats(page)).picking && (await page.evaluate(() => window.__wp.map.isOpen)), `${label}: city map opened as the finish picker`);
  check(await visible(page, "#st-go"), `${label}: challenge card on the map`);
  await shot(page, `street_${label}_2_offer.png`);

  // Pick B.
  const pt = await raceablePoint(page);
  check(!!pt, `${label}: a raceable finish exists on screen`);
  if (pt && pad) {
    // The left stick pans the map under the crosshair.
    const centreWorld = () =>
      page.evaluate(() => {
        const r = document.querySelector("#wp-map-full > canvas").getBoundingClientRect();
        return window.__wp.map.hud.worldAt(r.left + r.width / 2, r.top + r.height / 2);
      });
    const c0 = await centreWorld();
    await page.evaluate(() => window.__pad.set({ axes: { 0: 0.8 } }));
    await frames(page, 3);
    await page.evaluate(() => window.__pad.set({ axes: { 0: 0 } }));
    await frames(page, 1);
    const c1 = await centreWorld();
    check(c1[0] > c0[0] + 1, `${label}: pad stick moves the crosshair east (${(c1[0] - c0[0]).toFixed(0)} m)`);
    // Bring the crosshair onto the point (as the stick would), then A.
    await page.evaluate(({ x, y }) => {
      const r = document.querySelector("#wp-map-full > canvas").getBoundingClientRect();
      const c = window.__wp.map.hud.clientAt(x, y);
      window.__wp.map.hud.panBy(r.left + r.width / 2 - c[0], r.top + r.height / 2 - c[1]);
    }, pt);
    await frames(page, 1);
    await page.evaluate(() => window.__pad.set({ buttons: { 0: 1 } }));
    await frames(page, 2);
    await page.evaluate(() => window.__pad.set({ buttons: { 0: 0 } }));
    await frames(page, 2);
  } else if (pt) {
    await page.mouse.click(pt.cx, pt.cy);
    await frames(page, 2);
  }
  const pv = await page.evaluate(() => window.__wp.street.preview);
  check(pv?.ok === true && !(await page.evaluate(() => document.querySelector("#st-go")?.disabled)), `${label}: picking B previews a raceable route (${pv ? Math.round(pv.length_m) : "?"} m)`);
  check((await mapStats(page)).full.route, `${label}: route drawn on the map`);
  let gates = 0;
  if (!pad && pt) {
    // Waypoints (row 151): clicks after B add them; ↓ re-orders, ✕ removes; numbered flags.
    const wps = await page.evaluate(({ x, y }) => {
      const wp = window.__wp;
      const st = wp.sim.states();
      const [px, py] = [st[1], st[2]];
      const panel = document.querySelector(".wp-map-host").getBoundingClientRect();
      const legend = document.querySelector(".wp-map-legend").getBoundingClientRect();
      const out = [];
      for (const f of [0.35, 0.65, 0.5, 0.25, 0.75]) {
        for (const side of [250, -250, 400, -400]) {
          const [dx, dy] = [x - px, y - py];
          const n = Math.hypot(dx, dy);
          const q = [px + dx * f - (dy / n) * side, py + dy * f + (dx / n) * side];
          const c = wp.map.hud.clientAt(q[0], q[1]);
          if (!c || c[0] < panel.right + 10 || c[0] > legend.left - 10 || c[1] < 60 || c[1] > innerHeight - 20) continue;
          try {
            const r = wp.sim.streetPreview(x, y, [...out.map((o) => o.q), q]);
            if (!r.ok) continue;
          } catch {
            continue;
          }
          out.push({ q, cx: c[0], cy: c[1] });
          if (out.length === 2) return out;
          break;
        }
      }
      return out;
    }, pt);
    check(wps.length === 2, `${label}: two waypoints on the map that make a raceable route (${wps.length})`);
    for (const w of wps) {
      await page.mouse.click(w.cx, w.cy);
      await frames(page, 2);
    }
    const s1 = await page.evaluate(() => ({ via: window.__wp.street.via, pv: window.__wp.street.preview, items: document.querySelectorAll(".wp-map-host .st-wp").length }));
    check(s1.via.length === 2 && s1.items === 2 && s1.pv?.gates.length === 2, `${label}: clicks after B add waypoints (${s1.via.length} listed ${s1.items}, gates ${s1.pv?.gates.length})`);
    check(s1.pv && s1.pv.length_m > pv.length_m, `${label}: the route runs through them (${Math.round(pv.length_m)} → ${Math.round(s1.pv?.length_m ?? 0)} m)`);
    const flags = await page.evaluate(() => window.__wp.map.hud.stats().full.flags ?? null);
    if (flags !== null) check(flags >= 4, `${label}: A, 1, 2 and B flags on the map (${flags})`);
    await page.click('.wp-map-host [data-wp-down="0"]');
    await frames(page, 2);
    const s2 = await page.evaluate(() => window.__wp.street.via);
    check(s2[0][0] === s1.via[1][0] && s2[1][0] === s1.via[0][0], `${label}: ↓ swaps the waypoint order`);
    await page.click('.wp-map-host [data-wp-del="1"]');
    await frames(page, 2);
    const s3 = await page.evaluate(() => ({ via: window.__wp.street.via, pv: window.__wp.street.preview }));
    check(s3.via.length === 1 && s3.pv?.ok && s3.pv.gates.length === 1, `${label}: ✕ removes a waypoint (${s3.via.length} left, route ${Math.round(s3.pv?.length_m ?? 0)} m)`);
    gates = s3.via.length;
    await shot(page, `street_${label}_3b_waypoints.png`);
  }

  // Bet.
  const o = (await status(page)).offer;
  let want;
  if (pad) {
    const s0 = await page.evaluate(() => window.__wp.street.stake);
    await page.evaluate(() => window.__pad.set({ buttons: { 5: 1 } }));
    await frames(page, 2);
    await page.evaluate(() => window.__pad.set({ buttons: { 5: 0 } }));
    await frames(page, 2);
    want = await page.evaluate(() => window.__wp.street.stake);
    check(want > s0 || s0 === o.bet_max, `${label}: pad RB raised the stake (${s0} → ${want})`);
  } else {
    want = Math.min(o.bet_max, Math.max(o.bet_min, Math.round((o.bet_min + o.bet_max) / 2 / 50) * 50));
    await page.evaluate((v) => {
      const r = document.querySelector("#st-range");
      r.value = String(v);
      r.dispatchEvent(new Event("input"));
    }, want);
    want = await page.evaluate(() => window.__wp.street.stake);
    check(want >= o.bet_min && want <= o.bet_max, `${label}: bet slider set the stake to ${want}`);
  }
  await shot(page, `street_${label}_3_route.png`);
  const cash0 = await page.evaluate(() => window.__wp.sim.runAccount().cash);
  const crashBefore = await page.evaluate(() => window.__wp.crash.phase);
  if (pad) {
    await page.evaluate(() => window.__pad.set({ buttons: { 9: 1 } }));
    await frames(page, 2);
    await page.evaluate(() => window.__pad.set({ buttons: { 9: 0 } }));
  } else await page.keyboard.press("Enter");
  await frames(page, 2);
  const r = (await status(page))?.race;
  check(r && r.countdown_s > 0, `${label}: Race (${pad ? "pad Menu" : "Enter"}) → countdown (${r?.countdown_s.toFixed(1)} s)`);
  check(!(await page.evaluate(() => window.__wp.map.isOpen)), `${label}: picker closed`);
  check(!pad || !(await page.evaluate(() => window.__wp.menu.isOpen)), `${label}: pad Menu did not open the pause menu`);
  check(pad || (await page.evaluate(() => window.__wp.crash.phase)) === crashBefore, `${label}: Enter did not trigger the crash replay`);
  const lineup = await page.evaluate((e) => {
    const wp = window.__wp;
    const st = wp.sim.states();
    const meta = wp.sim.vehicleMeta();
    const stride = st.length / (meta.length / 3);
    const at = (id) => {
      for (let o = 0; o < st.length; o += stride) if (st[o] === id) return [st[o + 1], st[o + 2]];
      return null;
    };
    const a = at(wp.app.player);
    const b = at(e);
    return a && b ? Math.hypot(a[0] - b[0], a[1] - b[1]) : -1;
  }, r?.entity);
  check(lineup > 2 && lineup < 8, `${label}: both cars side by side at the start (${lineup.toFixed(1)} m apart)`);
  const cash1 = await page.evaluate(() => window.__wp.sim.runAccount().cash);
  check(r && r.stake === want && cash1 === cash0 - r.stake, `${label}: stake ${r?.stake} paid (${cash0} → ${cash1})`);
  await page.waitForFunction(() => window.__wp.map.hud.stats().minimap.route, null, { timeout: 10000 }).catch(() => {});
  check(await visible(page, "#street-hud"), `${label}: race panel visible`);
  check(await visible(page, "#street-count"), `${label}: countdown visible`);
  check((await mapStats(page)).minimap.route, `${label}: route on the minimap`);
  const overlap = await page.evaluate(() => {
    const a = document.querySelector("#street-hud").getBoundingClientRect();
    return ["#radio-strip", ".wp-minimap", ".left-col"].filter((s) => {
      const e = document.querySelector(s);
      if (!e || e.closest(".hidden")) return false;
      const b = e.getBoundingClientRect();
      return b.width > 0 && a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
    });
  });
  check(overlap.length === 0, `${label}: race panel clear of ${overlap.join(", ") || "the radio strip, minimap and speedo"}`);
  await shot(page, `street_${label}_4_countdown.png`);
  const rl = await lodOf(page, r.entity);
  check(rl.target === "view", `${label}: the opponent keeps its hero view during the race (${rl.target}, ${Math.round(rl.targetDist)} m)`);
  // Wait on sim time (slow headless frames): green light + 4 s.
  await page.waitForFunction(() => (window.__wp.sim.streetStatus()?.race?.elapsed_s ?? 0) > 4, null, { timeout: 180000 }).catch(() => {});
  await shot(page, `street_${label}_5_racing.png`);
  {
    // Photo mode and the game menu hide the street UI.
    await page.keyboard.press("F7");
    await frames(page, 2);
    check(!(await visible(page, "#street-hud")), `${label}: street UI hidden in photo mode`);
    await page.evaluate(() => window.__wp.photo.exit());
    await frames(page, 2);
    check(await visible(page, "#street-hud"), `${label}: street UI back after photo mode`);
    await page.evaluate(() => window.__wp.menu.openPause());
    await frames(page, 2);
    check(await page.evaluate(() => document.getElementById("street-ui").classList.contains("hidden")), `${label}: street UI hidden under the game menu`);
    await page.evaluate(() => window.__wp.menu.close());
    await frames(page, 2);
  }
  if (gates) {
    const g = await page.evaluate(() => ({ shown: window.__wp.streetMarker.shown, visible: window.__wp.streetMarker.group.visible, gates: window.__wp.sim.streetStatus().race.gates }));
    check(g.visible && g.shown === 0, `${label}: the waypoint shows as a gate in the world (gate ${g.shown})`);
    // B before the gate does not count; through the gate, then B wins.
    await page.evaluate(([x, y]) => window.__wp.sim.debugPlaceVehicle(window.__wp.app.player, x, y, 0), r.finish);
    await wait(page, 1500);
    check(!(await status(page))?.result, `${label}: B before the gate does not finish`);
    await page.evaluate(([x, y]) => window.__wp.sim.debugPlaceVehicle(window.__wp.app.player, x, y, 0), g.gates[0]);
    await page.waitForFunction(() => window.__wp.sim.streetStatus()?.race?.next_gate === 1, null, { timeout: 30000 }).catch(() => {});
    check((await status(page))?.race?.next_gate === 1, `${label}: passing the gate counts it`);
  }
  // Teleport to B: win.
  await page.evaluate(([x, y]) => window.__wp.sim.debugPlaceVehicle(window.__wp.app.player, x, y, 0), r.finish);
  await page.waitForFunction(() => window.__wp.sim.streetStatus()?.result, null, { timeout: 60000 }).catch(() => {});
  const res = (await status(page))?.result;
  check(res?.won === true, `${label}: race finished, won (${res?.reason})`);
  const cash2 = await page.evaluate(() => window.__wp.sim.runAccount().cash);
  check(cash2 === cash0 + r.stake, `${label}: even money ${cash0} → ${cash2}`);
  await page.waitForSelector("#street-result:not(.hidden)", { timeout: 20000 }).catch(() => {});
  check(await visible(page, "#street-result"), `${label}: result banner visible`);
  await shot(page, `street_${label}_6_result.png`);
  // A crash replay hides the street UI (the result banner is still up): knock a parked car with
  // auto replay on, then skip the replay.
  await page.evaluate(() => {
    const w = window.__wp;
    w.setAutoCrashReplay(true);
    w.sim.debugKnockProp(w.app.player, -1, 14);
  });
  await page.waitForFunction(() => window.__wp.crash.phase === "playing", null, { timeout: 30000 }).catch(() => {});
  const phase = await page.evaluate(() => window.__wp.crash.phase);
  await frames(page, 2);
  check(phase === "playing" && (await page.evaluate(() => document.getElementById("street-ui").classList.contains("hidden"))), `${label}: street UI hidden during a crash replay (${phase})`);
  await page.evaluate(() => window.__wp.setAutoCrashReplay(false));
  if (phase === "playing") {
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => window.__wp.crash.phase === "idle", null, { timeout: 30000 }).catch(() => {});
  }
  await frames(page, 3);
  check(!(await page.evaluate(() => document.getElementById("street-ui").classList.contains("hidden"))), `${label}: street UI back after the replay`);
  if (!pad) {
    // Last: wanted, a honk challenges nobody and the hint says to lose the police (row 150).
    await page.evaluate(() => window.__wp.sim.debugSetHeat(window.__wp.app.player, 40));
    await page.keyboard.down(hornKey);
    let blocked = null;
    for (let i = 0; i < 12; i++) {
      await placeBehind(page, st.roster[1].entity);
      await wait(page, 250);
      blocked = (await status(page))?.blocked ?? blocked;
    }
    await page.keyboard.up(hornKey);
    const ws = await status(page);
    const hint = await page.evaluate(() => document.querySelector("#street-hint")?.textContent ?? "");
    check(ws.pursued && !ws.offer && !!blocked, `${label}: wanted, a honk opens no challenge (blocked: ${blocked?.name})`);
    check(/Lose the police/.test(hint), `${label}: hint says to lose the police first ("${hint.trim()}")`);
    // (The pursuit outlives the heat; the session ends here.)
  }
}

// Day: 13:00 free drive, keyboard + mouse.
await page.evaluate((car) => {
  window.__wp.sim.setStreetRacers(true);
  window.__wp.startFreeDrive(car, "Arcade", true, 13);
}, CAR);
const on = await simMs();
const frameMs = await page.evaluate(() => window.__wp.app.perf.frameMs);
console.log(`sim ms per frame: without street racers ${off.toFixed(2)}, with ${on.toFixed(2)} (frame ${frameMs.toFixed(0)} ms)`);
await race("day", false);

// Night: 23:00 free drive, gamepad.
await page.evaluate((car) => {
  window.__pad.connect(true);
  window.__wp.startFreeDrive(car, "Arcade", true, 23);
}, CAR);
await wait(page, 3000);
const hours = await page.evaluate(() => window.__wp.sim.worldInfo()[1]);
check(hours >= 22.9 || hours < 1, `night session (${hours.toFixed(2)} h)`);
await race("night", true);
await page.evaluate(() => window.__pad.connect(false));

// Row 153: a night loop racer idling at a Loop 9 rest stop takes a challenge there; the card
// offers a Loop 9 lap back to it; the race starts side by side on the parking road.
if (CITY === "vesper_v2") {
  await page.evaluate((car) => window.__wp.startFreeDrive(car, "Arcade", true, 23), CAR);
  await wait(page, 3000);
  const stops = await page.evaluate(() => window.__wp.sim.restStops());
  check(stops.length === 3, `rest stops on Loop 9 (${stops.map((s) => s.name).join(", ")})`);
  const rs = stops[0];
  const lr = (await status(page)).roster.find((r) => r.shift === "Loop" && r.mode === "Roam");
  check(!!lr, `a loop racer out at night (${lr?.name})`);
  if (rs && lr) {
    await page.evaluate(([e, n]) => window.__wp.sim.debugStreetParkAtRestStop(e, n), [lr.entity, rs.name]);
    await wait(page, 1500);
    const hornKey = await page.evaluate(() => window.__wp.bindings().keys.horn[0]);
    await page.keyboard.down(hornKey);
    for (let i = 0; i < 30; i++) {
      await placeBehind(page, lr.entity);
      await wait(page, 250);
      if ((await status(page))?.offer) break;
    }
    await page.keyboard.up(hornKey);
    await page.waitForFunction(() => window.__wp.map.isOpen, null, { timeout: 15000 }).catch(() => {});
    const o = (await status(page))?.offer;
    check(o?.entity === lr.entity && o?.rest_stop === rs.name && !!o?.loop_lap, `challenge at ${rs.name}: rest-stop meet with a Loop 9 lap (${o?.rest_stop})`);
    await page.waitForSelector("#st-lap", { timeout: 10000 }).catch(() => {});
    await page.click("#st-lap").catch(() => {});
    await frames(page, 2);
    const pv = await page.evaluate(() => window.__wp.street.preview);
    check(pv?.ok && pv.length_m > 9000, `"Loop 9 lap back here" plans the lap (${Math.round(pv?.length_m ?? 0)} m)`);
    await shot(page, "street_reststop_offer.png");
    await page.keyboard.press("Enter");
    await frames(page, 2);
    const race = (await status(page))?.race;
    const gap = await page.evaluate((e) => {
      const wp = window.__wp;
      const st = wp.sim.states();
      const meta = wp.sim.vehicleMeta();
      const stride = st.length / (meta.length / 3);
      const at = (id) => {
        for (let o = 0; o < st.length; o += stride) if (st[o] === id) return [st[o + 1], st[o + 2]];
        return null;
      };
      const a = at(wp.app.player);
      const b = at(e);
      return a && b ? Math.hypot(a[0] - b[0], a[1] - b[1]) : -1;
    }, lr.entity);
    check(race && race.countdown_s > 0 && gap > 3.5 && gap < 4.5, `rest-stop start: countdown, side by side ${gap.toFixed(1)} m apart`);
    await shot(page, "street_reststop_start.png");
  }
}

// Late-night night run: racers roam there too and show on the map.
await page.evaluate((car) => window.__wp.startNightRun(car, "Arcade"), CAR);
await wait(page, 4000);
const chase = await status(page);
check(chase?.roster.length === 13 && chase.roster.every((r) => r.mode !== "OffDuty"), `night run (23:00): all 13 street racers out (${chase?.roster.length})`);
check(chase?.roster.filter((r) => r.shift === "Loop").length === 4, "night run: 4 of them lap Loop 9");
await page.waitForFunction(() => window.__wp.map.hud.stats().minimap.markers > 0, null, { timeout: 30000 }).catch(() => {});
check((await mapStats(page)).minimap.markers === 13, "night run: racer markers on the minimap");
const cl = await lodOf(page);
check(cl.farOnView === 0, `night run: far racers fleet-drawn (${cl.views} hero views, ${JSON.stringify(cl.modes)})`);
await page.keyboard.press("KeyM");
await frames(page, 3);
await shot(page, "street_chase_citymap.png");
await page.keyboard.press("KeyM");

// Touch: the 📯 button challenges, a tap on the map picks B, Decline closes the picker.
{
  const tctx = await browser.newContext({ viewport: { width: 932, height: 430 }, hasTouch: true, isMobile: true, deviceScaleFactor: 1 });
  const tp = await tctx.newPage();
  watch(tp);
  await tp.goto(`${URL}?city=${CITY}&touch=1`);
  await tp.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
  await tp.evaluate((car) => window.__wp.startFreeDrive(car, "Arcade", true, 13), CAR);
  await wait(tp, 4000);
  const ts = await status(tp);
  const t = ts?.roster[0];
  const horn = await tp.evaluate(() => {
    const r = document.querySelector('#wp-touch [data-touch="horn"]')?.getBoundingClientRect();
    return r && r.width > 0 ? [r.left + r.width / 2, r.top + r.height / 2] : null;
  });
  check(!!horn, "touch: horn button shown");
  if (horn && t) {
    await tp.evaluate(() => {
      const el = document.querySelector('#wp-touch [data-touch="horn"]');
      el.dispatchEvent(new PointerEvent("pointerdown", { pointerId: 7, pointerType: "touch", bubbles: true }));
    });
    for (let i = 0; i < 40; i++) {
      await placeBehind(tp, t.entity);
      await wait(tp, 250);
      if ((await status(tp))?.offer) break;
    }
    await tp.evaluate(() => {
      const el = document.querySelector('#wp-touch [data-touch="horn"]');
      el.dispatchEvent(new PointerEvent("pointerup", { pointerId: 7, pointerType: "touch", bubbles: true }));
    });
  }
  await tp.waitForFunction(() => window.__wp.map.hud.stats().picking, null, { timeout: 20000 }).catch(() => {});
  check((await status(tp))?.offer?.entity === t?.entity, `touch: 📯 challenged ${t?.name}`);
  const tpt = await raceablePoint(tp);
  if (tpt) await tp.touchscreen.tap(tpt.cx, tpt.cy);
  await frames(tp, 2);
  check((await tp.evaluate(() => window.__wp.street.preview))?.ok === true, "touch: a tap on the map picks B");
  await shot(tp, "street_touch_offer.png");
  const no = await tp.evaluate(() => {
    const r = document.querySelector("#st-no")?.getBoundingClientRect();
    return r ? [r.left + r.width / 2, r.top + r.height / 2] : null;
  });
  if (no) await tp.touchscreen.tap(no[0], no[1]);
  await frames(tp, 2);
  check(!(await status(tp))?.offer && !(await tp.evaluate(() => window.__wp.map.isOpen)), "touch: Decline closes the picker");
  await tctx.close();
}

// Row 169: street bets follow the career. Main menu → Free drive with the career box ticked
// (and a career car) is a career run: the bet slip says "Career cash". Career box off: quick
// play, and the slip says the bet is practice cash. (Settlement on the career account:
// wp_wasm street_bets_settle_on_the_career.)
// Third case: career box ticked but no career car (the menu falls back to quick play): the
// bet is refused, so nothing is bet outside the career while it is ticked.
for (const [careerBox, buyCar] of [[true, true], [false, true], [true, false]]) {
  const label = !careerBox ? "career off" : buyCar ? "career on" : "career on, no car";
  const cp = await context.newPage();
  watch(cp);
  await cp.goto(`${URL}?city=${CITY}&menu=1`);
  await cp.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 120000 });
  await cp.evaluate((on) => {
    const wp = window.__wp;
    wp.careerApi.newCareer("Arcade");
    if (on[1]) wp.careerApi.buy("junkyard_beater", "Worn");
    document.getElementById("career").checked = on[0];
  }, [careerBox, buyCar]);
  await cp.click("#gm-play-free");
  await cp.waitForFunction(() => window.__wp.sim.streetStatus() !== null, null, { timeout: 60000 }).catch(() => {});
  await wait(cp, 1500);
  const st0 = await status(cp);
  const active = await cp.evaluate(() => window.__wp.sim.careerRunActive());
  const career = careerBox && buyCar;
  check(active === career && st0?.career === career, `${label}: menu Free drive is ${career ? "a career run" : "quick play"} (career run ${active}, street career ${st0?.career})`);
  const target = st0?.roster.find((r) => r.mode !== "OffDuty");
  const hornKey = await cp.evaluate(() => window.__wp.bindings().keys.horn[0]);
  if (target) {
    await cp.keyboard.down(hornKey);
    for (let i = 0; i < 40; i++) {
      await placeBehind(cp, target.entity);
      await wait(cp, 250);
      if ((await status(cp))?.offer) break;
    }
    await cp.keyboard.up(hornKey);
  }
  await cp.waitForSelector('[data-testid="st-wallet"]', { timeout: 20000 }).catch(() => {});
  const wallet = await cp.evaluate(() => {
    const el = document.querySelector('[data-testid="st-wallet"]');
    return el ? { career: el.getAttribute("data-career"), text: el.textContent } : null;
  });
  check(wallet?.career === (career ? "1" : "0"), `${label}: the bet slip says ${career ? "career cash" : "quick play"} ("${wallet?.text ?? "no offer"}")`);
  const refused = await cp.evaluate(() => ({
    note: !!document.querySelector('[data-testid="st-career-locked"]'),
    goDisabled: document.querySelector("#st-go")?.disabled ?? null,
  }));
  const locked = careerBox && !career;
  check(refused.note === locked && (!locked || refused.goDisabled === true), `${label}: ${locked ? "the bet is refused (career is ticked, the drive is quick play)" : "betting allowed"} (${JSON.stringify(refused)})`);
  await cp.close();
}

check(errors.length === 0, `no page errors ${errors.slice(0, 3).join(" | ")}`);
await browser.close();
process.exit(failures ? 1 : 0);
