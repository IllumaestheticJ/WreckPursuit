// Detached-panel debris shading check (lead, owner report "parts falling off are glowing";
// docs/integration/debris-glow.md).
// Usage: node e2e/debris_glow.mjs   (needs `npm run wasm`; own Vite + headless Chromium)
// Env: DEBRIS_SHOTS=dir writes screenshots (docs/media/debris-glow/), CHROMIUM=path,
//      ONLY=lowpoly_night (one style_when), KIND=headon|tbone (one crash).
//
// For both art styles (low-poly dark, greybox) by day and by night, with every car's
// headlights and the police car's light bar forced on:
//  1. A head-on (hatch vs. police sedan, panels on) tears the front bumper off; a T-bone
//     plus repeated side hits (sim.debugPlace) on the standing hatch tear a door off.
//  2. Every debris mesh shades like the panel it came from: it is lit, and it either has
//     a normal attribute with no zero-length normal or a flat-shaded material.
//  3. Nothing on a debris piece glows (emissive 0): lamp lenses go dark (broken-lamp
//     material) once their panel has left the car.
//  4. No page errors.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = Number(process.env.PORT ?? 5231);
const shots = process.env.DEBRIS_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

// Own process group, so the exit handler also stops the vite process npx starts.
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe", detached: true });
process.on("exit", () => {
  try {
    process.kill(-server.pid, "SIGTERM");
  } catch {}
});
for (let i = 0; ; i++) {
  try {
    if ((await fetch(`http://localhost:${PORT}/`)).ok) break;
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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(`http://localhost:${PORT}/?gfx=high`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
// Clean frame: no HUD, no debug arrows / markers.
await page.evaluate(() => {
  document.getElementById("overlay").style.display = "none";
  // No crash-replay offer over the debris.
  const st = document.createElement("style");
  st.textContent = "#crash-offer { display: none !important; }";
  document.head.appendChild(st);
  for (const l of ["contacts", "cgVelocity", "tireForces", "slipColour"]) window.__wp.toggleLayer(l);
});

/**
 * Run the head-on scenario file rebuilt as `kind` ("headon" | "tbone") at `hour`, with panels
 * on and the lights forced on (headlights on every car, siren on the police sedan).
 */
const crash = (kind, hour) =>
  page.evaluate(
    async ([kind, hour]) => {
      const wp = window.__wp;
      const withSpeed = wp.sim.scenarioWithSpeed;
      wp.sim.scenarioWithSpeed = (json, mps) => {
        const o = JSON.parse(withSpeed(json, mps));
        o.time_of_day_h = hour;
        o.panels = true;
        o.expect = [];
        const [a, b] = o.entities;
        a.car = "kestrel_ts";
        b.car = "sentinel_warden";
        if (kind === "tbone") {
          // The hatch stands still; the police sedan comes in along +X into its left front door.
          a.driver = { cruise: { target_mps: 0, from_s: 0 } };
          b.pos = [-70, 0.4, 0];
          b.yaw = -Math.PI / 2;
          b.driver = { cruise: { target_mps: 45, from_s: 0 } };
        } else {
          a.driver = { cruise: { target_mps: 30, from_s: 0 } };
          b.driver = { cruise: { target_mps: 30, from_s: 0 } };
        }
        return JSON.stringify(o);
      };
      try {
        await wp.runScenario("crash/head_on.ron", 100);
      } finally {
        wp.sim.scenarioWithSpeed = withSpeed;
      }
      // Lights on: headlights on both cars, light bar + wig-wag on the police sedan.
      for (const v of wp.app.views.values()) {
        const set = v.setMeta.bind(v);
        const police = v.info?.car_id?.startsWith("sentinel");
        v.setMeta = (flags, t) => set(flags | 2 | (police ? 1 : 0), t);
      }
    },
    [kind, hour],
  );

const debris = () => page.evaluate(() => window.__wp.debris());

/** Wait (up to `ms`) until a debris mesh whose name matches `re` exists, then let it land. */
const waitDebris = async (re, ms) => {
  const t0 = Date.now();
  let d = [];
  while (Date.now() - t0 < ms) {
    d = await debris();
    if (d.some((m) => re.test(m.name))) break;
    await page.waitForTimeout(200);
  }
  await page.waitForTimeout(2500);
  return debris();
};

/**
 * Screenshot staging: a close side view of the debris piece of the first hatch (entity 1)
 * mesh matching `re`, with the police sedan parked 8 m behind it and facing it (headlights and
 * light bar on it) and the hatch moved well away. The police AI drives on, so the shot is
 * taken right after placing it.
 */
const stage = async (d, re) => {
  const m = d.find((x) => x.vehicle === 1 && re.test(x.name)) ?? d.find((x) => re.test(x.name));
  if (!m) return;
  const [x, y, z] = m.at;
  const place = ([x, y, z]) => {
    const wp = window.__wp;
    wp.sim.debugPlace(1, x + 60, y + 60, 0, 0);
    wp.sim.debugPlace(2, x - 0.8, y - 8, 0, 0);
    wp.lookFrom([x + 2.8, y - 1.6, z + 1.4], [x, y, z]);
  };
  await page.evaluate(place, [x, y, z]);
  await page.waitForTimeout(600);
  await page.evaluate(place, [x, y, z]);
  await page.waitForTimeout(150);
};

/** The shading checks on one debris list. */
const audit = (label, d) => {
  const bad = d.filter((m) => !m.lit || (!m.normals && !m.flat));
  check(bad.length === 0, `${label}: every debris mesh is lit with usable normals (${d.length} meshes${bad.length ? `; bad: ${bad.map((m) => m.name).join(", ")}` : ""})`);
  const glow = d.filter((m) => m.emissive > 0);
  check(glow.length === 0, `${label}: no debris mesh glows${glow.length ? ` (${glow.map((m) => `${m.name} ${m.emissive.toFixed(2)}`).join(", ")})` : ""}`);
  const lamps = d.filter((m) => m.kind === "light");
  check(lamps.every((m) => m.brokenLamp), `${label}: detached lamp lenses are dark (${lamps.length} lenses${lamps.length ? `: ${lamps.map((m) => m.name).join(", ")}` : ""})`);
};

/**
 * Side hits on the standing hatch (entity 1) by the police sedan (entity 2): the first hit
 * unlatches the left doors, repeats slam them off their hinges. Up to `n` hits until a door
 * debris piece exists.
 */
const sideHits = async (n) => {
  for (let i = 0; i < n; i++) {
    if ((await debris()).some((m) => /^door_/.test(m.name))) return;
    await page.evaluate(() => {
      const sim = window.__wp.sim;
      sim.debugPlace(1, 0, 0, 0, 0);
      sim.debugPlace(2, -9, 0.4, -Math.PI / 2, 40);
    });
    await page.waitForTimeout(1800);
  }
};

// ONLY=lowpoly_night (style_when) runs one combination; KIND=headon|tbone one crash.
const only = process.env.ONLY;
const kinds = process.env.KIND ? [process.env.KIND] : ["headon", "tbone"];
for (const style of ["lowpoly", "greybox"]) {
  await page.evaluate((s) => window.__wp.setStyle(s), style);
  for (const [when, hour] of [
    ["day", 13],
    ["night", 23],
  ]) {
    if (only && only !== `${style}_${when}`) continue;
    const tag = `${style} ${when}`;
    // 1. Head-on: the front bumper comes off.
    if (kinds.includes("headon")) {
      await crash("headon", hour);
      const d = await waitDebris(/^bumper_front/, 25000);
      check(d.some((m) => /^bumper_front/.test(m.name)), `${tag}: head-on tears the front bumper off (${d.length} debris meshes)`);
      audit(`${tag} bumper`, d);
      await stage(d, /^bumper_front/);
      if (shots) await page.screenshot({ path: join(shots, `${style}_${when}_bumper.png`) });
    }
    // 2. T-bone (repeated side hits): a door comes off.
    if (kinds.includes("tbone")) {
      await crash("tbone", hour);
      await waitDebris(/^door_/, 12000);
      await sideHits(6);
      const d = await waitDebris(/^door_/, 3000);
      const doors = [...new Set(d.map((m) => m.name))].filter((n) => /^door_[a-z]+$/.test(n));
      check(doors.length > 0, `${tag}: side hits tear a door off (${doors.join(", ") || "none"})`);
      audit(`${tag} door`, d);
      await stage(d, /^door_[a-z]+$/);
      if (shots) await page.screenshot({ path: join(shots, `${style}_${when}_door.png`) });
    }
  }
}

check(errors.length === 0, `no page errors${errors.length ? `: ${errors.slice(0, 3).join(" | ")}` : ""}`);
await browser.close();
console.log(failures ? `${failures} check(s) failed` : "all checks passed");
process.exit(failures ? 1 : 0);
