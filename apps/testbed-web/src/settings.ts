// Debug layer toggles and other per-viewer settings, persisted in localStorage.
// To add a debug layer: add an entry to LAYERS (key + label + default), then read
// `layers.on("<id>")` where you draw it. The help overlay and hotkeys pick it up automatically.
// Keys 1–8 are camera angles (cameras.ts) and the player actions (driving, camera cycle /
// look-back, map, pause, mute, ...) are rebindable (bindings.ts; their defaults include V, M,
// Tab, ` and F8/F9): keep layer keys off the defaults. A bound action wins over a layer key.

export interface LayerDef {
  id: string;
  key: string; // KeyboardEvent.code
  label: string;
  default: boolean;
}

export const LAYERS: LayerDef[] = [
  { id: "body", key: "KeyB", label: "body box / skinned body", default: true },
  { id: "panels", key: "Digit0", label: "panel meshes / lattice view (car models, workstream R)", default: true },
  { id: "lattice", key: "KeyL", label: "lattice wireframe", default: false },
  { id: "stress", key: "KeyH", label: "lattice stress heat-map", default: false },
  { id: "tireForces", key: "KeyT", label: "tire force arrows (applied Fx/Fy)", default: true },
  { id: "suspension", key: "KeyU", label: "suspension travel bars", default: false },
  { id: "slipColour", key: "KeyY", label: "slip-angle wheel colouring", default: true },
  { id: "contacts", key: "F2", label: "contact points & impulses", default: true },
  { id: "cgVelocity", key: "KeyG", label: "CG + velocity vector", default: true },
  { id: "traffic", key: "KeyX", label: "civilian traffic agents", default: true },
  { id: "signals", key: "Digit9", label: "traffic signals + stop lines", default: true },
  { id: "intents", key: "KeyI", label: "traffic/police intent arrows", default: false },
  { id: "components", key: "KeyK", label: "component health panel", default: true },
  { id: "regions", key: "KeyE", label: "region energy panel", default: false },
  { id: "events", key: "F4", label: "event feed (damage, glass, parts, violations)", default: true },
  { id: "telemetry", key: "KeyJ", label: "telemetry plots", default: true },
  { id: "inputs", key: "KeyN", label: "input display + rack torque", default: true },
  { id: "minimap", key: "Quote", label: "minimap (UI kit; M or Tab opens the city map)", default: true },
  { id: "mapSignals", key: "Semicolon", label: "minimap: traffic signals", default: false },
  { id: "chase", key: "KeyZ", label: "chase readout: wanted / heat / bounty / pursuit (UI kit)", default: true },
  // Accessibility (not a debug layer): slow police light pattern everywhere, same as `?flash=slow`.
  { id: "reducedFlashing", key: "F6", label: "reduced flashing: slow police lights (also ?flash=slow)", default: false },
];

const STORE_KEY = "wp.testbed.layers.v1";

function load(): Record<string, boolean> {
  try {
    return JSON.parse(localStorage.getItem(STORE_KEY) ?? "{}");
  } catch {
    return {};
  }
}

const state: Record<string, boolean> = { ...Object.fromEntries(LAYERS.map((l) => [l.id, l.default])), ...load() };
/** Layers hidden for now (photo mode) regardless of their saved toggle. */
let hidden: ReadonlySet<string> = new Set();

/** The debug drawing layers (player mode and the garage workshop hide these). */
export const DEBUG_LAYERS = ["lattice", "stress", "tireForces", "suspension", "slipColour", "contacts", "cgVelocity", "intents", "components", "regions", "events", "telemetry", "inputs"];

/** Layers forced off regardless of their toggle (player mode hides the debug drawing). */
let suppressed = new Set<string>();
/** Layers a screen holds off on top of that (the garage workshop). */
let held = new Set<string>();

export const layers = {
  on: (id: string): boolean => !suppressed.has(id) && !hidden.has(id) && !held.has(id) && (state[id] ?? false),
  /** Hold these layers off while a screen is open (developer mode included); `[]` releases. */
  hold(ids: Iterable<string>): void {
    held = new Set(ids);
  },
  /** Force these layers off (player mode, game menu); `[]` lifts it. Toggles keep their state. */
  suppress(ids: Iterable<string>): void {
    suppressed = new Set(ids);
  },
  /** Hide these layers until the next call (`[]` shows them again); the saved toggles stay. */
  hide(ids: string[]): void {
    hidden = new Set(ids);
  },
  toggle(id: string): void {
    state[id] = !state[id];
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify(state));
    } catch {
      // Storage unavailable (private mode): toggles still work for this page.
    }
  },
  byKey: (code: string): LayerDef | undefined => LAYERS.find((l) => l.key === code),
};

/** Small localStorage helpers that never throw. */
export const store = {
  get(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: string, value: string): boolean {
    try {
      localStorage.setItem(key, value);
      return true;
    } catch {
      return false;
    }
  },
};

export function bytesToBase64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

export function base64ToBytes(b64: string): Uint8Array {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}
