// CPU profile of the night run in headless Chromium (Chrome DevTools Profiler): prints the
// top functions by inclusive and self time over 8 s. Usage: node e2e/cpu_profile.mjs
// (needs `npm run wasm`; starts its own Vite). Used to find the browser hot spots in
// docs/PLAYTEST_LOG.md row 3.
import { spawn } from "node:child_process";
import { chromium } from "playwright-core";
const server = spawn("npx", ["vite", "--port", "5196", "--strictPort"], { cwd: process.cwd(), stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (;;) { try { if ((await fetch("http://localhost:5196/")).ok) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
const browser = await chromium.launch({ executablePath: "/opt/pw-browsers/chromium-1194/chrome-linux/chrome", args: ["--use-angle=swiftshader","--enable-unsafe-swiftshader"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
await page.goto("http://localhost:5196/");
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await page.evaluate(() => window.__wp.startNightRun(undefined, "Arcade"));
await page.keyboard.down("KeyW");
await page.waitForTimeout(4000);
const cdp = await page.context().newCDPSession(page);
await cdp.send("Profiler.enable");
await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
await cdp.send("Profiler.start");
await page.waitForTimeout(8000);
const { profile } = await cdp.send("Profiler.stop");
const self = new Map(); const byId = new Map(profile.nodes.map((n) => [n.id, n]));
const dt = profile.timeDeltas; const counts = new Map();
profile.samples.forEach((id, i) => counts.set(id, (counts.get(id) ?? 0) + (dt[i] ?? 0)));
// inclusive by function name
const parent = new Map(); for (const n of profile.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
const incl = new Map();
for (const [id, t] of counts) {
  const n = byId.get(id); const key = `${n.callFrame.functionName || "(anon)"} ${n.callFrame.url.split("/").pop()}:${n.callFrame.lineNumber}`;
  self.set(key, (self.get(key) ?? 0) + t);
  const seen = new Set(); let cur = id;
  while (cur !== undefined) { const m = byId.get(cur); const k = `${m.callFrame.functionName || "(anon)"} ${m.callFrame.url.split("/").pop()}:${m.callFrame.lineNumber}`; if (!seen.has(k)) { incl.set(k, (incl.get(k) ?? 0) + t); seen.add(k); } cur = parent.get(cur); }
}
const top = (m) => [...m].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([k, v]) => `${(v / 1000).toFixed(0).padStart(6)} ms  ${k}`).join("\n");
console.log("== inclusive ==\n" + top(incl)); console.log("== self ==\n" + top(self));
await browser.close(); process.exit(0);
