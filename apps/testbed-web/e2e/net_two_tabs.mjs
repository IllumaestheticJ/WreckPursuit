// Two browser tabs on one locally run wp-server (headless Chromium, Playwright).
// Usage: npm run e2e:net   (needs `npm run wasm` first; builds and starts wp-server itself).
// Env: CHROMIUM=/path/to/chrome, SHOTS=dir for screenshots, WP_SERVER=/path/to/wp-server to
// skip the cargo build.
//
// Checks: both pages join the same session as racers and each sees 2 player cars; tab A
// drives off and stops, tab B steers into it (each page drives from what it sees); after the
// crash both cars are dented on both pages, the two pages hold identical damage (lattice
// hashes) for both cars; tab A's socket drops and it reconnects with its token, keeping its
// car; no page errors.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = join(root, "..", "..");
const PORT = 5198;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};
const children = [];
process.on("exit", () => children.forEach((c) => c.kill("SIGTERM")));

// 1. wp-server: flat world on the real sim, any free ports; the ws address comes from its
//    JSON "listening" log line.
let serverBin = process.env.WP_SERVER;
if (!serverBin) {
  const b = spawnSync("cargo", ["build", "--release", "-q", "-p", "wp_server"], { cwd: repo, stdio: "inherit" });
  if (b.status !== 0) throw new Error("cargo build -p wp_server failed");
  serverBin = join(repo, "target", "release", "wp-server");
}
const server = spawn(serverBin, ["--port", "0", "--admin-port", "0", "--time-of-day", "12", "--data", join(repo, "data")], {
  cwd: repo,
  stdio: ["ignore", "pipe", "inherit"],
});
children.push(server);
const wsUrl = await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error("wp-server did not start")), 60000);
  createInterface({ input: server.stdout }).on("line", (line) => {
    try {
      const j = JSON.parse(line);
      if (j.event === "listening") {
        clearTimeout(timer);
        resolve(j.ws.replace("0.0.0.0", "127.0.0.1"));
      }
    } catch {}
  });
  server.on("exit", (c) => reject(new Error(`wp-server exited (${c})`)));
});
console.log(`wp-server at ${wsUrl}`);

// 2. Vite.
const vite = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
children.push(vite);
for (let i = 0; ; i++) {
  try {
    if ((await fetch(`http://localhost:${PORT}/net.html`)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const errors = [];
async function openTab(name) {
  // Separate contexts: two players, as from two browsers.
  const context = await browser.newContext({ viewport: { width: 900, height: 600 } });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`${name}: ${e}`));
  page.on("console", (m) => m.type() === "error" && errors.push(`${name}: ${m.text()}`));
  const url = `http://localhost:${PORT}/net.html?server=${encodeURIComponent(wsUrl)}&name=${name}&role=racer`;
  await page.goto(url);
  await page.waitForFunction(
    () => window.__wpNet?.status().role === "racer" && window.__wpNet.status().vehicle !== null,
    null,
    { timeout: 60000 },
  );
  return page;
}
const a = await openTab("tabA");
const b = await openTab("tabB");
const shot = async (page, name) => shots && page.screenshot({ path: join(shots, name) });

const players = (page) =>
  page.evaluate(() => window.__wpNet.render().filter((v) => v.kind === "racer" || v.kind === "police"));
async function waitPlayers(page, n) {
  for (let i = 0; i < 100; i++) {
    if ((await players(page)).length >= n) return (await players(page)).length;
    await page.waitForTimeout(100);
  }
  return (await players(page)).length;
}
for (const [name, page] of [["A", a], ["B", b]]) {
  const n = await waitPlayers(page, 2);
  check(n === 2, `tab ${name} sees ${n} player cars (want 2)`);
}
const idA = await a.evaluate(() => window.__wpNet.status().vehicle);
const idB = await b.evaluate(() => window.__wpNet.status().vehicle);
check(idA !== idB, `two different cars (${idA}, ${idB})`);

// 3. Tab A drives forward ~2.5 s and stops; tab B then steers at A's car as B's page sees it.
await a.evaluate(() => window.__wpNet.setInput({ throttle: 0.8, headlights: true }));
await a.waitForTimeout(2500);
await a.evaluate(() => window.__wpNet.setInput({ brake: 1, handbrake: 1 }));
await a.waitForTimeout(1500);
const steerAt = (target) =>
  b.evaluate((target) => {
    const list = window.__wpNet.render();
    const me = list.find((v) => v.own);
    const it = list.find((v) => v.entity === target);
    if (!me || !it) return null;
    // Chassis +Y in world: rotate (0, 1, 0) by the quaternion [x, y, z, w].
    const [x, y, z, w] = me.rot;
    const fx = 2 * (x * y - w * z);
    const fy = 1 - 2 * (x * x + z * z);
    const dx = it.pos[0] - me.pos[0];
    const dy = it.pos[1] - me.pos[1];
    const dist = Math.hypot(dx, dy);
    const cross = fx * dy - fy * dx; // > 0: target on the left; steer +1 is right
    const steer = Math.max(-1, Math.min(1, (-2.5 * cross) / Math.max(dist, 1)));
    window.__wpNet.setInput({ steer, throttle: 0.9, headlights: true });
    return dist;
  }, target);
let closest = 1e9;
for (let i = 0; i < 120; i++) {
  const d = await steerAt(idA);
  if (d !== null) closest = Math.min(closest, d);
  await b.waitForTimeout(50);
  if (d !== null && d < 3.5) break;
}
await b.waitForTimeout(600);
await b.evaluate(() => window.__wpNet.setInput({ brake: 1, handbrake: 1 }));
check(closest < 6, `tab B reached tab A's car (closest ${closest.toFixed(1)} m between centres)`);

// 4. Damage replicated: both cars dented on both pages, same lattice hashes on both pages.
const damageOf = (page) =>
  page.evaluate(() => Object.fromEntries(window.__wpNet.damage().map((d) => [d.entity, d])));
let dA, dB;
for (let i = 0; i < 60; i++) {
  await a.waitForTimeout(250);
  dA = await damageOf(a);
  dB = await damageOf(b);
  const dented = [idA, idB].every((e) => dA[e]?.dented_nodes > 0 && dB[e]?.dented_nodes > 0);
  const same = [idA, idB].every((e) => dA[e]?.hash === dB[e]?.hash);
  if (dented && same) break;
}
for (const [name, d] of [["A", dA], ["B", dB]]) {
  for (const [who, e] of [["A", idA], ["B", idB]]) {
    const x = d[e];
    check(
      x && x.dented_nodes > 0,
      `tab ${name}: car ${who} deformed (${x ? `${x.dented_nodes} nodes, max ${(x.max_disp_m * 100).toFixed(1)} cm` : "no replica"})`,
    );
  }
}
for (const [who, e] of [["A", idA], ["B", idB]]) {
  check(dA[e] && dB[e] && dA[e].hash === dB[e].hash, `car ${who}: both pages hold the same damage (hash ${dA[e]?.hash} / ${dB[e]?.hash})`);
}
// 5. Reconnect: tab A's socket drops; the page reconnects with its token and keeps its car.
await a.evaluate(() => window.__wpNet.dropConnection());
await a.waitForFunction(() => window.__wpNet.connState() === "closed", null, { timeout: 10000 }).catch(() => {});
const back = await a
  .waitForFunction(
    (id) => window.__wpNet.connState() === "open" && window.__wpNet.status().vehicle === id,
    idA,
    { timeout: 20000 },
  )
  .then(() => true)
  .catch(() => false);
check(back, "tab A reconnected with its token and kept its car");
check((await waitPlayers(a, 2)) === 2, "tab A sees both player cars again after reconnecting");

await shot(a, "net_two_tabs_a.png");
await shot(b, "net_two_tabs_b.png");
const stats = await Promise.all([a, b].map((p) => p.evaluate(() => window.__wpNet.stats())));
console.log(
  `stats: ${stats.map((s, i) => `tab ${"AB"[i]} rtt ${s.rtt_ms.toFixed(0)} ms, rms ${s.correction_rms_m.toFixed(3)} m, hash ${s.hash_mismatches}/${s.hash_checks}, down ${(s.bps_in / 1000).toFixed(1)} kB/s`).join("; ")}`,
);
check(errors.length === 0, `no page errors${errors.length ? `: ${errors.join(" | ")}` : ""}`);

await browser.close();
console.log(failures === 0 ? "net two-tab e2e: all checks passed" : `net two-tab e2e: ${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
