// Chromium executable for the e2e scripts: `$CHROMIUM` when set, else the newest Playwright
// Chromium installed under `$PLAYWRIGHT_BROWSERS_PATH` (default /opt/pw-browsers), whatever its
// build number (`chromium-<n>/chrome-linux{,64}/chrome`), so the scripts keep working when the
// container's browser build changes.
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

/** Path of the Chromium binary the e2e scripts launch. */
export function chromiumPath() {
  if (process.env.CHROMIUM) return process.env.CHROMIUM;
  const root = process.env.PLAYWRIGHT_BROWSERS_PATH ?? "/opt/pw-browsers";
  const builds = existsSync(root)
    ? readdirSync(root)
        .map((d) => /^chromium-(\d+)$/.exec(d))
        .filter((m) => m !== null)
        .map((m) => ({ dir: m[0], n: Number(m[1]) }))
        .sort((a, b) => b.n - a.n)
    : [];
  for (const b of builds) {
    for (const sub of ["chrome-linux64", "chrome-linux"]) {
      const p = join(root, b.dir, sub, "chrome");
      if (existsSync(p)) return p;
    }
  }
  // Nothing found: the old default, so the launch error names a concrete path.
  return join(root, "chromium-1194", "chrome-linux", "chrome");
}
