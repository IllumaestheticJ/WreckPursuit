// Photo mode check (headless Chromium, Playwright).
// Usage: npm run e2e:photo  (needs `npm run wasm` first). Env: CHROMIUM=/path/to/chrome,
// SHOTS=dir writes the captured photos there (default: none written).
//
// Checks, live and from a replay: F3 freezes the game and hides the HUD and debug layers, WASD
// fly the photo camera without driving the car, filters change the pixels (noir is grey),
// Space saves a PNG download, C copies a PNG to the clipboard, 2× capture doubles the size,
// "." steps a frozen replay, and Esc gives back the game exactly as it was.
import { spawn } from "node:child_process";
import { mkdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5199;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS ?? "";
if (shots) mkdirSync(shots, { recursive: true });

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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 960, height: 540 }, acceptDownloads: true });
await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: URL });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

/** Game state that must not change while frozen. */
const snap = () =>
  page.evaluate(() => {
    const { sim, app, photo } = window.__wp;
    const s = sim.states();
    return {
      tick: sim.worldInfo()[0],
      car: [s[1], s[2], s[3]],
      speed: s[14],
      paused: app.paused,
      active: photo.active,
      overlay: document.getElementById("overlay").style.display,
      panel: !document.getElementById("photo-panel").classList.contains("hidden"),
      cam: photo.camera.position.toArray(),
    };
  });
const dist = (a, b) => Math.hypot(...a.map((v, i) => v - b[i]));

/** Capture in the page; returns size plus colour statistics of a pixel grid. */
const capture = (scale) =>
  page.evaluate(async (scale) => {
    const { blob, width, height } = await window.__wp.photo.capture(scale);
    const bmp = await createImageBitmap(blob);
    const c = new OffscreenCanvas(bmp.width, bmp.height);
    const g = c.getContext("2d");
    g.drawImage(bmp, 0, 0);
    const px = g.getImageData(0, 0, bmp.width, bmp.height).data;
    let grey = 0;
    let n = 0;
    const sample = [];
    for (let y = 0; y < bmp.height; y += Math.floor(bmp.height / 24)) {
      for (let x = 0; x < bmp.width; x += Math.floor(bmp.width / 32)) {
        const i = (y * bmp.width + x) * 4;
        grey += Math.max(px[i], px[i + 1], px[i + 2]) - Math.min(px[i], px[i + 1], px[i + 2]);
        sample.push(px[i], px[i + 1], px[i + 2]);
        n++;
      }
    }
    const bytes = new Uint8Array(await blob.arrayBuffer());
    let bin = "";
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return { width, height, size: blob.size, type: blob.type, chroma: grey / n, sample, b64: btoa(bin) };
  }, scale);
const meanDiff = (a, b) => a.reduce((s, v, i) => s + Math.abs(v - b[i]), 0) / a.length;
const keep = (name, shot) => shots && writeFileSync(join(shots, name), Buffer.from(shot.b64, "base64"));

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
await page.evaluate(() => window.__wp.startFreeDrive("lindwurm_4t", "Arcade", false, 12));
await page.keyboard.down("KeyW");
await page.waitForTimeout(2500);

// ----- live session -------------------------------------------------------------------------
await page.keyboard.press("F3");
await page.keyboard.up("KeyW");
await page.waitForTimeout(200);
const a = await snap();
await page.waitForTimeout(1000);
const b = await snap();
check(a.active && a.paused && a.overlay === "none" && a.panel, `F3 opens photo mode: paused, HUD hidden, panel shown`);
check(a.speed > 3, `car was moving when the photo was taken (${(a.speed * 3.6).toFixed(0)} km/h)`);
check(b.tick === a.tick && dist(a.car, b.car) < 1e-6, `game frozen (tick ${a.tick} → ${b.tick})`);
const hidden = await page.evaluate(() => ["tireForces", "cgVelocity", "contacts", "lattice"].every((id) => !window.__wp.layersOn(id)));
check(hidden, "debug layers hidden in photo mode");

await page.keyboard.down("KeyW");
await page.keyboard.down("KeyE");
await page.waitForTimeout(600);
await page.keyboard.up("KeyW");
await page.keyboard.up("KeyE");
await page.waitForTimeout(300);
const c = await snap();
check(dist(a.cam, c.cam) > 1.5 && c.cam[1] > a.cam[1], `W/E fly the photo camera (${dist(a.cam, c.cam).toFixed(1)} m)`);
check(c.tick === a.tick && dist(a.car, c.car) < 1e-6, "flying the camera does not drive the car");

// Touch screens: the hide button hides the panel and the chip brings it back.
await page.click("#photo-panel .ph-hide");
const chipShown = await page.evaluate(() => !document.getElementById("photo-chip").classList.contains("hidden"));
check(!(await snap()).panel && chipShown, "hide button hides the panel, chip shown");
await page.click("#photo-chip");
check((await snap()).panel, "chip brings the panel back");

const natural = await capture(1);
check(natural.type === "image/png" && natural.width > 0 && natural.size > 5000, `capture: ${natural.width}×${natural.height} PNG, ${natural.size} bytes`);
keep("01_natural.png", natural);
await page.keyboard.press("Digit2");
const noir = await capture(1);
keep("02_noir.png", noir);
check(noir.chroma < 3 && natural.chroma > noir.chroma + 3, `noir filter is black & white (chroma ${natural.chroma.toFixed(1)} → ${noir.chroma.toFixed(1)})`);
await page.keyboard.press("Digit4");
const golden = await capture(1);
keep("03_golden.png", golden);
check(meanDiff(golden.sample, natural.sample) > 3, `golden-hour filter changes the image (mean Δ ${meanDiff(golden.sample, natural.sample).toFixed(1)})`);
const big = await capture(2);
check(big.width === natural.width * 2 && big.height === natural.height * 2, `2× capture: ${big.width}×${big.height}`);

const [download] = await Promise.all([page.waitForEvent("download", { timeout: 10000 }), page.keyboard.press("Space")]);
const dlPath = await download.path();
const dlName = download.suggestedFilename();
check(/^wreckpursuit-.+-t\d+\.png$/.test(dlName) && statSync(dlPath).size > 5000, `Space saves ${dlName} (${statSync(dlPath).size} bytes)`);

await page.keyboard.press("KeyC");
const copied = await page
  .waitForFunction(() => window.__wp.photo.copied > 0, null, { timeout: 10000 })
  .then(() => page.evaluate(async () => (await navigator.clipboard.read()).flatMap((i) => i.types)), (e) => [`error ${e}`]);
check(copied.includes("image/png"), `C copies a PNG to the clipboard (${copied.join(",")})`);

await page.keyboard.press("Escape");
await page.waitForTimeout(800);
const d = await snap();
check(!d.active && !d.paused && d.overlay === "" && !d.panel && d.tick > c.tick, `Esc leaves: game runs again (tick ${c.tick} → ${d.tick}), HUD back`);
const restored = await page.evaluate(() => ["tireForces", "cgVelocity", "contacts"].map((id) => window.__wp.layersOn(id)));
check(restored.every(Boolean), "debug layers restored on leaving");

// A paused game stays paused after photo mode.
await page.keyboard.press("KeyP");
await page.keyboard.press("F3");
await page.keyboard.press("Escape");
const e = await snap();
check(e.paused && !e.active, "a game paused before photo mode is still paused after it");
await page.keyboard.press("KeyP");

// ----- replay -------------------------------------------------------------------------------
await page.evaluate(() => window.__wp.startFreeDrive("lindwurm_4t", "Arcade", false, 18));
await page.evaluate(() => window.__wp.sim.recordStart());
await page.keyboard.down("KeyW");
await page.keyboard.down("KeyD");
await page.waitForTimeout(2500);
await page.keyboard.up("KeyD");
await page.keyboard.up("KeyW");
await page.evaluate(() => window.__wp.loadReplay(window.__wp.stopRecording()));
await page.waitForTimeout(800);
await page.keyboard.press("F3");
await page.waitForTimeout(150);
const r1 = await snap();
await page.waitForTimeout(800);
const r2 = await snap();
check(r1.active && r2.tick === r1.tick && r1.tick > 0, `photo mode from a replay holds it at tick ${r1.tick}`);
await page.keyboard.press("Period");
await page.waitForTimeout(150);
const r3 = await snap();
check(r3.tick === r1.tick + 4 && r3.active, `"." steps the frozen replay (tick ${r1.tick} → ${r3.tick})`);
const rshot = await capture(1);
keep("04_replay.png", rshot);
check(rshot.size > 5000, `capture from the replay (${rshot.width}×${rshot.height})`);
await page.keyboard.press("Escape");
await page.waitForTimeout(600);
const r4 = await snap();
check(!r4.active && r4.tick > r3.tick, `Esc resumes the replay (tick ${r3.tick} → ${r4.tick})`);

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} check(s) failed` : "all photo mode checks passed");
process.exit(failures ? 1 : 0);
