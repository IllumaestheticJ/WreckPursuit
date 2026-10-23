// What every mod looks like on the car: data/cosmetics/visuals.json, mod id → pieces (or a
// reason it cannot be seen). The pieces are drawn by the builders in this folder; the piece
// vocabulary is listed in data/cosmetics/README.md.

/** One visual piece of a mod (`piece` names the builder; the other fields are its options). */
export interface Piece {
  piece: string;
  style?: string;
  colour?: string;
  level?: number;
  size?: number;
  bottles?: number;
  axle?: "front" | "rear";
  end?: "front" | "rear";
  panel?: "hood" | "trunk";
  finish?: string;
  design?: string;
}

/** data/cosmetics/visuals.json. */
export interface VisualsJson {
  format: number;
  mods: Record<string, Piece[] | { none: string }>;
}

let loading: Promise<VisualsJson | null> | null = null;
let loaded: VisualsJson | null = null;

/** Load the visuals table once (null when it is missing: no looks, nothing breaks). */
export function loadVisuals(): Promise<VisualsJson | null> {
  return (loading ??= fetch("data/cosmetics/visuals.json")
    .then((r) => (r.ok ? (r.json() as Promise<VisualsJson>) : null))
    .catch(() => null)
    .then((v) => (loaded = v)));
}

/** The pieces of a set of installed mods, in mod order (unknown and invisible mods add none). */
export function piecesOf(mods: readonly string[], visuals: VisualsJson | null = loaded): Piece[] {
  const out: Piece[] = [];
  for (const id of mods) {
    const v = visuals?.mods[id];
    if (Array.isArray(v)) out.push(...v);
  }
  return out;
}
