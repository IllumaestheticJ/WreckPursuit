// Honk-to-start race spots (wp_wasm `race_spots()`, `race_spot_honked()`): every picker race
// that runs on the session's map has a meet spot at its start. Drawn in the scene's Z-up
// `world` group (sim coordinates) as a glowing ground ring with a light pillar and a floating
// label (unlit materials: they glow at night and read from a distance), plus a HUD prompt while
// the player's car is inside one. Stop in a spot and honk: the sim reports the race and
// `main.ts` starts it like the picker's "Race" button. Hidden during races (the session lists
// no spots then).
//
// One challenge system (round 12): the honk is the same horn action as a street-racer
// challenge, and the sim reads one horn edge for both: a street racer within honking range
// takes it first. The money is the picker race's existing stake (buy-in / purse from
// `race_list`), shown in the prompt; there is no separate bet.
import * as THREE from "three";

/** One spot from `race_spots()` (`wp_wasm::race_spots::SpotCard`). */
export interface RaceSpot {
  id: string;
  name: string;
  kind: string;
  laps: number;
  legality: string;
  x: number;
  y: number;
  z: number;
  heading: number;
  radius: number;
  length_m: number;
}

/** Below this speed a honk starts the race (`wp_race::spots::HONK_MAX_SPEED_MPS`), m/s. */
export const HONK_MAX_SPEED_MPS = 3.0;
/** A car this far above or below a spot is not in it (`SPOT_HEIGHT_WINDOW_M`), m. */
const HEIGHT_WINDOW_M = 4.0;

const STREET = 0xff9a2e;
const ILLEGAL = 0xff3b5c;
const PILLAR_H = 28;

const KIND_LABEL: Record<string, string> = {
  sprint: "Sprint",
  circuit: "Circuit",
  checkpoint: "Checkpoint",
  drag: "Drag",
  outrun: "Outrun",
  cannonball: "Cannonball",
  delivery: "Delivery",
  time_attack: "Time attack",
};

/** "Sprint", "Circuit ×2", … */
export function kindLabel(s: Pick<RaceSpot, "kind" | "laps">): string {
  const k = KIND_LABEL[s.kind] ?? s.kind;
  return s.kind === "circuit" && s.laps > 1 ? `${k} ×${s.laps}` : k;
}

/** "1.0 km" / "402 m" (per lap for circuits). */
export function lengthLabel(s: Pick<RaceSpot, "kind" | "length_m">): string {
  const m = s.length_m;
  const len = m >= 1000 ? `${(m / 1000).toFixed(1)} km` : `${Math.round(m)} m`;
  return s.kind === "circuit" ? `${len} lap` : len;
}

/** The picker's advertised stake of a race for the player's car (`race_list`: buy-in, purse), $. */
export interface SpotStake {
  entry_fee: number;
  purse: number;
}

/** The HUD prompt for a spot ("Honk to start: …", with the race's buy-in and purse when known). */
export function promptText(s: RaceSpot, honkKey: string, stake?: SpotStake): string {
  const money = stake ? ` — buy-in $${stake.entry_fee.toLocaleString("en-US")}, purse $${stake.purse.toLocaleString("en-US")}` : "";
  return `Honk (${honkKey}) to start: ${s.name} — ${kindLabel(s)}, ${lengthLabel(s)}${money}`;
}

interface SpotView {
  spot: RaceSpot;
  root: THREE.Group;
  ring: THREE.Mesh;
  label: THREE.Sprite;
}

export class RaceSpotsView {
  readonly group = new THREE.Group();
  private views: SpotView[] = [];
  private readonly prompt: HTMLDivElement;
  private readonly ringGeo = new THREE.RingGeometry(0.86, 1.0, 64);
  private readonly discGeo = new THREE.CircleGeometry(1.0, 48);
  private readonly pillarGeo = new THREE.CylinderGeometry(0.9, 0.9, PILLAR_H, 16, 1, true);
  private readonly arrowGeo = new THREE.ConeGeometry(1.4, 3.2, 3);
  private readonly mats = new Map<number, { ring: THREE.MeshBasicMaterial; disc: THREE.MeshBasicMaterial; pillar: THREE.MeshBasicMaterial }>();
  /** Spot the player is in (null outside); e2e reads it. */
  inside: RaceSpot | null = null;
  /** Stake per race id for the player's car (`set`). */
  private stakes = new Map<string, SpotStake>();

  constructor(parent: THREE.Object3D, overlay: HTMLElement) {
    this.group.name = "race-spots";
    parent.add(this.group);
    this.prompt = document.createElement("div");
    this.prompt.id = "race-spot-prompt";
    this.prompt.style.cssText =
      "position:absolute;left:50%;bottom:22%;transform:translateX(-50%);padding:8px 16px;border-radius:8px;" +
      "background:rgba(10,12,18,0.78);color:#fff;font:600 15px/1.35 system-ui,sans-serif;text-align:center;" +
      "border:1px solid rgba(255,154,46,0.8);box-shadow:0 0 18px rgba(255,154,46,0.35);pointer-events:none;display:none;max-width:90vw;";
    overlay.appendChild(this.prompt);
  }

  private matsFor(colour: number) {
    let m = this.mats.get(colour);
    if (!m) {
      const common = { color: colour, transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true } as const;
      m = {
        ring: new THREE.MeshBasicMaterial({ ...common, opacity: 0.9 }),
        disc: new THREE.MeshBasicMaterial({ ...common, opacity: 0.16 }),
        pillar: new THREE.MeshBasicMaterial({ ...common, opacity: 0.14, blending: THREE.AdditiveBlending }),
      };
      this.mats.set(colour, m);
    }
    return m;
  }

  /** Replace the spots (a new session; `[]` hides them all) and their stakes per race id. */
  set(spots: RaceSpot[], stakes: Map<string, SpotStake> = new Map()): void {
    this.stakes = stakes;
    for (const v of this.views) {
      this.group.remove(v.root);
      (v.label.material as THREE.SpriteMaterial).map?.dispose();
      v.label.material.dispose();
    }
    this.views = [];
    this.inside = null;
    this.prompt.style.display = "none";
    for (const s of spots) {
      const colour = s.legality === "illegal_daytime" ? ILLEGAL : STREET;
      const m = this.matsFor(colour);
      const root = new THREE.Group();
      root.position.set(s.x, s.y, s.z);
      root.rotation.z = s.heading; // local +Y = race direction
      const ring = new THREE.Mesh(this.ringGeo, m.ring);
      ring.scale.setScalar(s.radius);
      ring.position.z = 0.12;
      const disc = new THREE.Mesh(this.discGeo, m.disc);
      disc.scale.setScalar(s.radius);
      disc.position.z = 0.1;
      const pillar = new THREE.Mesh(this.pillarGeo, m.pillar);
      pillar.rotation.x = Math.PI / 2; // cylinder axis (+Y) → +Z
      pillar.position.z = PILLAR_H / 2;
      // A flat arrow on the ground pointing the race direction.
      const arrow = new THREE.Mesh(this.arrowGeo, m.ring);
      arrow.position.set(0, s.radius * 0.45, 0.14);
      arrow.scale.set(1, 1, 0.05);
      const label = makeLabel(s, colour);
      label.position.z = 9;
      root.add(disc, ring, pillar, arrow, label);
      this.group.add(root);
      this.views.push({ spot: s, root, ring, label });
    }
    this.group.visible = this.views.length > 0;
  }

  /** Per frame: pulse the rings and update the HUD prompt from the player's position / speed. */
  update(player: [number, number, number] | null, speedMps: number, tS: number, honkKey: string, hidden: boolean): void {
    this.group.visible = !hidden && this.views.length > 0;
    let inside: SpotView | null = null;
    let best = Infinity;
    for (const v of this.views) {
      const pulse = 1 + 0.04 * Math.sin(tS * 3 + v.spot.x * 0.01);
      v.ring.scale.setScalar(v.spot.radius * pulse);
      if (!player) continue;
      const d = Math.hypot(player[0] - v.spot.x, player[1] - v.spot.y);
      if (d <= v.spot.radius && Math.abs(player[2] - v.spot.z) <= HEIGHT_WINDOW_M && d < best) {
        best = d;
        inside = v;
      }
    }
    this.inside = hidden ? null : (inside?.spot ?? null);
    if (!this.inside) {
      this.prompt.style.display = "none";
      return;
    }
    this.prompt.style.display = "";
    const text = promptText(this.inside, honkKey, this.stakes.get(this.inside.id));
    const slow = speedMps < HONK_MAX_SPEED_MPS;
    const line = slow ? text : `Stop here to start: ${this.inside.name}`;
    if (this.prompt.textContent !== line) this.prompt.textContent = line;
    this.prompt.style.opacity = slow ? "1" : "0.75";
  }
}

/** A canvas sprite with the race's name and "kind · length · legality". */
function makeLabel(s: RaceSpot, colour: number): THREE.Sprite {
  const w = 512;
  const h = 160;
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const g = c.getContext("2d");
  if (g) {
    const hex = `#${colour.toString(16).padStart(6, "0")}`;
    g.fillStyle = "rgba(8,10,16,0.72)";
    g.beginPath();
    g.roundRect(4, 4, w - 8, h - 8, 22);
    g.fill();
    g.lineWidth = 5;
    g.strokeStyle = hex;
    g.stroke();
    g.textAlign = "center";
    g.fillStyle = "#ffffff";
    g.font = "700 46px system-ui, sans-serif";
    g.fillText(fit(g, s.name, w - 40), w / 2, 68, w - 40);
    g.fillStyle = hex;
    g.font = "600 34px system-ui, sans-serif";
    const legal = s.legality === "illegal_daytime" ? " · illegal" : "";
    g.fillText(`${kindLabel(s)} · ${lengthLabel(s)}${legal}`, w / 2, 122, w - 40);
  }
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthWrite: false, fog: false });
  const sprite = new THREE.Sprite(mat);
  sprite.scale.set(16, 5, 1);
  sprite.renderOrder = 10;
  return sprite;
}

function fit(g: CanvasRenderingContext2D, text: string, max: number): string {
  if (g.measureText(text).width <= max) return text;
  let t = text;
  while (t.length > 4 && g.measureText(`${t}…`).width > max) t = t.slice(0, -1);
  return `${t}…`;
}
