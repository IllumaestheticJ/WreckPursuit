// End-to-end check of the police radio (owner playtest row 79) in headless Chromium.
// Usage: node e2e/radio.mjs  (needs the wasm package built first, like e2e/run.mjs).
//
// Checks: in the night run, police transmissions reach the radio strip with a coloured call
// sign chip — straight from the police events while the scanner hears nothing (tier "None"),
// and through the audio scanner (RadioSay) once it does, where the line is also spoken with
// speechSynthesis when the browser has it; the strip never shows more than 3 lines; the game
// menu has the "Police radio voice" switch; lines carry their priority tier (row 144) and the
// "Radio: Important only / All" setting reaches the scanner; no page errors.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5197;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(URL)).ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("vite did not start");
}

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

await waitForServer();
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: [
    "--use-gl=angle",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--ignore-gpu-blocklist",
    "--autoplay-policy=no-user-gesture-required",
  ],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

// 1. Night run, wanted, scanner off: the strip shows police events directly.
await page.evaluate(() => {
  window.__wpAudio.setTier("None");
  window.__wp.startNightRun("debug_sedan", "Arcade");
});
await page.evaluate(() => window.__wp.sim.debugSetHeat(window.__wp.app.player, 30));
await page.keyboard.down("KeyW");
await page.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 }).catch(() => {});
await page
  .waitForFunction(() => window.__wp.radioStrip.count > 0, null, { timeout: 180000 })
  .catch(() => {});
const a = await page.evaluate(() => ({
  count: window.__wp.radioStrip.count,
  hears: window.__wpAudio.hearsRadio(),
  recent: window.__wp.radioStrip.recent.slice(),
  chips: [...document.querySelectorAll("#radio-strip .radio-line")].map((l) => ({
    cs: l.querySelector(".radio-cs")?.textContent ?? "",
    colour: l.querySelector(".radio-cs")?.style.color ?? "",
    text: l.textContent ?? "",
  })),
  simS: window.__wp.sim.worldInfo()[0] / 240,
}));
console.log(`      ${a.count} line(s) by ${a.simS.toFixed(1)} s of sim time: ${JSON.stringify(a.recent)}`);
check(!a.hears && a.count > 0, `radio strip shows police events while the scanner is off (${a.count})`);
check(
  a.chips.length > 0 && a.chips.every((c) => /^(UNITS? [\d ,AND]+|AIR \d+|DISPATCH|CAPTAIN|RADIO)$/.test(c.cs) && c.colour !== ""),
  `every strip line has a coloured call-sign chip (${a.chips.map((c) => c.cs).join(", ")})`,
);

// 2. Scanner on (Elite, 3 s delay): lines come through the scanner, spoken when supported.
const speech0 = await page.evaluate(() => window.__wpAudio.speech());
const count0 = a.count;
await page.evaluate(() => window.__wpAudio.setTier("Elite"));
await page.evaluate(() => window.__wp.sim.debugSetHeat(window.__wp.app.player, 45));
await page
  .waitForFunction((n) => window.__wpAudio.radioLog().length > 0 && window.__wp.radioStrip.count > n, count0, { timeout: 180000 })
  .catch(() => {});
await page.keyboard.up("KeyW");
const b = await page.evaluate(() => ({
  hears: window.__wpAudio.hearsRadio(),
  log: window.__wpAudio.radioLog().map((l) => l.text),
  count: window.__wp.radioStrip.count,
  speech: window.__wpAudio.speech(),
  visible: window.__wp.radioStrip.visible(),
  simS: window.__wp.sim.worldInfo()[0] / 240,
}));
console.log(`      scanner said ${JSON.stringify(b.log.slice(0, 3))} by ${b.simS.toFixed(1)} s; speech ${JSON.stringify(b.speech)}`);
check(b.hears && b.log.length > 0 && b.count > count0, `scanner lines reach the strip (${b.count - count0} new)`);
const heard = await page.evaluate(() => {
  const said = window.__wpAudio.radioLog().map((l) => l.text);
  return said.every((t) => window.__wp.radioStrip.recent.includes(t) || window.__wp.radioStrip.recent.length >= 8);
});
check(heard, "the strip shows exactly what the scanner said");
if (b.speech.supported) {
  check(b.speech.attempts > speech0.attempts, `speechSynthesis attempted for heard lines (${b.speech.attempts})`);
} else {
  console.log("      speechSynthesis not available in this browser: squelch + text only");
}
check(b.visible <= 3, `strip keeps at most 3 lines (${b.visible})`);

// 3. Settings: the game menu has the "Police radio voice" switch and it reaches the scanner.
const sw = await page.evaluate(async () => {
  const audio = window.__wpAudio;
  audio.setRadioVoice(false);
  const off = audio.speech().voice;
  audio.setRadioVoice(true);
  return { off, on: audio.speech().voice, menu: window.__wp.menu?.settings?.radioVoice };
});
check(sw.off === false && sw.on === true && sw.menu !== undefined, `radio voice switch works (${JSON.stringify(sw)})`);

// 4. Priority tiers (row 144): every strip line carries its tier (1 actionable … 5 routine),
// the heard lines carry one from the scanner, and "Radio: Important only" reaches the scanner.
const t = await page.evaluate(() => ({
  strip: [...document.querySelectorAll("#radio-strip .radio-line")].map((l) => Number(l.dataset.tier ?? -1)),
  log: window.__wpAudio.radioLog().map((l) => l.tier),
}));
check(
  t.strip.length > 0 && t.strip.every((n) => n >= 1 && n <= 5) && t.log.every((n) => n >= 1 && n <= 5),
  `strip and scanner lines carry a tier (strip ${JSON.stringify(t.strip)}, scanner ${JSON.stringify(t.log)})`,
);
const imp = await page.evaluate(() => {
  const audio = window.__wpAudio;
  audio.setRadioImportantOnly(true);
  const on = audio.radioImportantOnly();
  audio.setRadioImportantOnly(false);
  return { on, off: audio.radioImportantOnly(), menu: window.__wp.menu?.settings?.radioFilter };
});
check(imp.on === true && imp.off === false && imp.menu === "all", `radio filter switch works, default All (${JSON.stringify(imp)})`);

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 5).join(" | ") : ""}`);
await browser.close();
process.exit(failures ? 1 : 0);
