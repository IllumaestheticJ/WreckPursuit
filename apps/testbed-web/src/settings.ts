// Debug layer toggles and other per-viewer settings, persisted in localStorage.
// To add a debug layer: add an entry to LAYERS (key + label + default), then read
// `layers.on("<id>")` where you draw it. The help overlay and hotkeys pick it up automatically.

export interface LayerDef {
  id: string;
  key: string; // KeyboardEvent.code
  label: string;
  default: boolean;
}

export const LAYERS: LayerDef[] = [
  { id: "body", key: "KeyB", label: "body box / skinned body", default: true },
  { id: "lattice", key: "KeyL", label: "lattice wireframe", default: false },
  { id: "stress", key: "KeyH", label: "lattice stress heat-map", default: false },
  { id: "tireForces", key: "KeyT", label: "tire force arrows (Fx/Fy est.)", default: true },
  { id: "suspension", key: "KeyU", label: "suspension travel bars", default: false },
  { id: "slipColour", key: "KeyY", label: "slip-angle wheel colouring", default: true },
  { id: "contacts", key: "KeyC", label: "contact points & impulses", default: true },
  { id: "cgVelocity", key: "KeyG", label: "CG + velocity vector", default: true },
  { id: "intents", key: "KeyI", label: "traffic/police intent arrows", default: false },
  { id: "components", key: "KeyK", label: "component health panel", default: true },
  { id: "regions", key: "KeyE", label: "region energy panel", default: false },
  { id: "telemetry", key: "KeyJ", label: "telemetry plots", default: true },
  { id: "inputs", key: "KeyN", label: "input display + rack torque", default: true },
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

export const layers = {
  on: (id: string): boolean => state[id] ?? false,
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
