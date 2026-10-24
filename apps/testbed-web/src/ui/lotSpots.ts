// Car lots on the city map (feature F37): PR #57's dealerships, used lots and junkyards each
// get a spot on a street (wp_wasm `lot_sites_json()`): a ring, a beam and a sign in the world,
// a "$" icon on the map, and, with a career, "Enter: browse <lot>" while the car is stopped
// inside. Enter / pad A opens the lot visit (it ends a career run first, like the menu's "Visit").
import * as THREE from "three";
import { sim } from "../wasm/sim";

/** `wp_wasm::lot_sites::LotSite`. */
export interface LotSite {
  id: string;
  name: string;
  kind: "Dealership" | "UsedLot" | "Junkyard";
  tier: number;
  pos: [number, number, number];
  heading_rad: number;
  radius_m: number;
  district: string;
}

const COLOUR = 0x2fbf71;
const BEAM_M = 30;
const KIND: Record<LotSite["kind"], string> = { Dealership: "Dealership", UsedLot: "Used cars", Junkyard: "Junkyard" };

export class LotSpotsView {
  readonly group = new THREE.Group();
  /** This session's lots (e2e reads them). */
  sites: LotSite[] = [];
  /** The lot the car is stopped in (prompt shown), or null. */
  here: LotSite | null = null;
  private readonly hud: HTMLDivElement;
  private padWas = true;

  constructor(
    parent: THREE.Object3D,
    overlay: HTMLElement,
    private readonly visit: (lotId: string) => void,
    private readonly allowed: () => boolean,
  ) {
    this.group.name = "car-lots";
    parent.add(this.group);
    this.hud = document.createElement("div");
    this.hud.className = "wp-ui wp-root";
    this.hud.dataset.testid = "lot-prompt";
    this.hud.style.cssText =
      "position:absolute;left:50%;top:15%;transform:translateX(-50%);pointer-events:none;z-index:60;padding:0.35em 0.9em;" +
      "background:rgba(8,10,16,0.78);border:1px solid #2fbf71;color:#e6efe9;font:600 14px ui-monospace,Menlo,Consolas,monospace;display:none";
    overlay.appendChild(this.hud);
    window.addEventListener(
      "keydown",
      (e) => {
        if (!this.here || e.repeat || (e.code !== "Enter" && e.code !== "NumpadEnter")) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        this.go();
      },
      true,
    );
  }

  /** New session: fetch the lots and rebuild the markers. */
  reset(): void {
    for (const c of [...this.group.children]) {
      c.traverse((o) => {
        if (o instanceof THREE.Mesh) {
          o.geometry.dispose();
          (o.material as THREE.Material).dispose();
        } else if (o instanceof THREE.Sprite) {
          o.material.map?.dispose();
          o.material.dispose();
        }
      });
      this.group.remove(c);
    }
    this.here = null;
    this.hud.style.display = "none";
    try {
      this.sites = JSON.parse(sim.lotSitesJson()) as LotSite[];
    } catch {
      this.sites = [];
    }
    for (const s of this.sites) {
      const holder = new THREE.Group();
      holder.position.set(...s.pos);
      holder.name = `lot:${s.id}`;
      const ring = new THREE.Mesh(
        new THREE.RingGeometry(s.radius_m - 0.6, s.radius_m, 48),
        new THREE.MeshBasicMaterial({ color: COLOUR, transparent: true, opacity: 0.55, side: THREE.DoubleSide, depthWrite: false, forceSinglePass: true }),
      );
      ring.position.z = 0.08;
      const beam = new THREE.Mesh(
        new THREE.CylinderGeometry(0.45, 0.45, BEAM_M, 10, 1, true),
        new THREE.MeshBasicMaterial({ color: COLOUR, transparent: true, opacity: 0.25, depthWrite: false }),
      );
      beam.rotation.x = Math.PI / 2;
      beam.position.z = BEAM_M / 2;
      const label = sign(s.name, `${KIND[s.kind]}${s.kind === "Dealership" ? " " + "★".repeat(s.tier) : ""} · stop to browse`);
      label.position.z = 6.5;
      holder.add(ring, beam, label);
      this.group.add(holder);
    }
  }

  private last: { pos: [number, number, number]; t: number } | null = null;

  /** One frame: the prompt for the player at `pos` (sim m) at time `nowMs`. The speed is the
   * position change since the last frame (wall clock; slow motion only makes it read low). */
  frame(pos: [number, number, number] | null, nowMs: number): void {
    let speed = 0;
    if (pos && this.last && nowMs > this.last.t) speed = Math.hypot(pos[0] - this.last.pos[0], pos[1] - this.last.pos[1]) / ((nowMs - this.last.t) / 1000);
    this.last = pos ? { pos, t: nowMs } : null;
    const inside = pos && this.allowed() ? (this.sites.find((s) => Math.hypot(pos[0] - s.pos[0], pos[1] - s.pos[1]) <= s.radius_m && Math.abs(pos[2] - s.pos[2]) <= 4) ?? null) : null;
    this.here = inside && speed < 3 ? inside : null;
    const text = inside ? (this.here ? `${inside.name} · Enter / A: browse today's cars` : `${inside.name} · stop to browse`) : "";
    if (this.hud.textContent !== text) this.hud.textContent = text;
    this.hud.style.display = text ? "block" : "none";
    if (this.here) this.pollPad();
    else this.padWas = true;
  }

  private go(): void {
    const s = this.here;
    if (!s) return;
    this.here = null;
    this.hud.style.display = "none";
    this.visit(s.id);
  }

  private pollPad(): void {
    const pads = typeof navigator !== "undefined" && navigator.getGamepads ? navigator.getGamepads() : [];
    let a = false;
    for (const p of pads) a ||= !!p?.buttons[0]?.pressed;
    const was = this.padWas;
    this.padWas = a;
    if (a && !was) this.go();
  }
}

/** The floating sign over a lot. */
function sign(name: string, sub: string): THREE.Sprite {
  const w = 512;
  const h = 150;
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const g = c.getContext("2d");
  if (g) {
    g.fillStyle = "rgba(8,10,16,0.72)";
    g.beginPath();
    g.roundRect(4, 4, w - 8, h - 8, 22);
    g.fill();
    g.lineWidth = 5;
    g.strokeStyle = "#2fbf71";
    g.stroke();
    g.textAlign = "center";
    g.fillStyle = "#ffffff";
    g.font = "700 42px system-ui, sans-serif";
    g.fillText(name, w / 2, 64, w - 40);
    g.fillStyle = "#2fbf71";
    g.font = "600 30px system-ui, sans-serif";
    g.fillText(sub, w / 2, 116, w - 40);
  }
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, transparent: true, depthWrite: false, fog: false }));
  sprite.scale.set(13, 3.8, 1);
  sprite.renderOrder = 10;
  return sprite;
}
