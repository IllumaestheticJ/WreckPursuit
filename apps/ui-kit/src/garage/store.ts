// Career persistence in the browser (machine G): localStorage with a versioned key, a backup
// of any save that failed to load, and export / import as a file.
//
// The save itself is wp_economy's `CareerProfile` JSON (it carries its own `format` and
// `version`; wp_economy::profile migrates older versions). This file only stores text.
// Every storage call is wrapped: private windows, quota errors and disabled storage must never
// break the game (the career then simply lives for this page only).

/** Current storage key. Bump the suffix only if the storage layout itself changes. */
export const CAREER_KEY = "wp.career.v1";
/** Where a save that failed to load is kept (so it is not lost when a new career starts). */
export const CAREER_BAD_KEY = "wp.career.v1.unreadable";
/** Older keys to migrate from, newest first (none yet: v1 is the first). */
const LEGACY_KEYS: string[] = [];

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** The stored save, or null. Migration stub: a save under a legacy key is moved to
 * [`CAREER_KEY`] (the JSON inside is migrated by wp_economy when it is loaded). */
export function loadCareerText(): string | null {
  const s = storage();
  if (!s) return null;
  try {
    const text = s.getItem(CAREER_KEY);
    if (text) return text;
    for (const k of LEGACY_KEYS) {
      const old = s.getItem(k);
      if (old) {
        s.setItem(CAREER_KEY, old);
        s.removeItem(k);
        return old;
      }
    }
  } catch {
    /* storage unavailable */
  }
  return null;
}

/** Store the save. Returns false when storage is unavailable or full. */
export function saveCareerText(text: string): boolean {
  const s = storage();
  if (!s) return false;
  try {
    s.setItem(CAREER_KEY, text);
    return true;
  } catch {
    return false;
  }
}

/** Keep a save that could not be loaded under [`CAREER_BAD_KEY`] before it is replaced. */
export function keepUnreadable(text: string): void {
  const s = storage();
  try {
    s?.setItem(CAREER_BAD_KEY, text);
  } catch {
    /* ignore */
  }
}

/** File name for an exported save, by in-game day. */
export function saveFileName(day: number): string {
  return `wreckpursuit-career-night${day}.json`;
}

/** Offer `text` as a file download. */
export function downloadText(name: string, text: string): void {
  const blob = new Blob([text], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

/** Ask the player for a save file; resolves with its text (rejects if none was picked). */
export function pickTextFile(): Promise<string> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,application/json";
    input.onchange = () => {
      const f = input.files?.[0];
      if (!f) return reject(new Error("no file"));
      f.text().then(resolve, reject);
    };
    input.click();
  });
}
