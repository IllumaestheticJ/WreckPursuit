// Safehouses (feature F39): a green ring + "SAFEHOUSE" sign at each one in the world, and a HUD
// line while the player is in one: how long they have hidden ("Hiding 4 / 10 s") or why it is
// not counting (not wanted; police too close). The rule is wp_police's (wanted, inside the
// radius, no unit within `clear_radius_m`, for `dwell_s`); wp_wasm `safehouses_json()` /
// `safehouse_progress(entity)` read it.
import * as THREE from "three";
import { sim } from "../wasm/sim";

/** `wp_wasm::safehouse::Safehouses`. */
export interface Safehouses {
  sites: { name: string; pos: [number, number, number]; radius_m: number }[];
  dwell_s: number;
  clear_radius_m: number;
}

const COLOUR = 0x3dff8f;
const BEAM_M = 30;

export class SafehouseView {
  /** World markers (scene `world` group, Z up). */
  readonly group = new THREE.Group();
  private data: Safehouses | null = null;
  private readonly hud: HTMLDivElement;
  /** The line shown now ("" = hidden); e2e reads it. */
  text = "";

  constructor(parent: THREE.Object3D, overlay: HTMLElement) {
    this.group.name = "safehouses";
    parent.add(this.group);
    this.hud = document.createElement("div");
    this.hud.className = "wp-ui wp-root";
    this.hud.dataset.testid = "safehouse-hud";
    this.hud.style.cssText =
      "position:absolute;left:50%;top:15%;transform:translateX(-50%);pointer-events:none;z-index:60;padding:0.35em 0.9em;" +
      "background:rgba(8,10,16,0.78);border:1px solid #3dff8f;color:#e6efe9;font:600 14px ui-monospace,Menlo,Consolas,monospace;display:none";
    overlay.appendChild(this.hud);
  }

  /** New session: fetch the safehouses and rebuild the markers. */
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
    this.show("");
    try {
      this.data = JSON.parse(sim.safehousesJson()) as Safehouses | null;
    } catch {
      this.data = null;
    }
    for (const s of this.data?.sites ?? []) {
      const holder = new THREE.Group();
      holder.position.set(...s.pos);
      holder.name = `safehouse:${s.name}`;
      const ring = new THREE.Mesh(
        new THREE.RingGeometry(s.radius_m - 0.5, s.radius_m, 48),
        new THREE.MeshBasicMaterial({ color: COLOUR, transparent: true, opacity: 0.45, side: THREE.DoubleSide, depthWrite: false, forceSinglePass: true }),
      );
      ring.position.z = 0.08;
      const beam = new THREE.Mesh(
        new THREE.CylinderGeometry(0.4, 0.4, BEAM_M, 10, 1, true),
        new THREE.MeshBasicMaterial({ color: COLOUR, transparent: true, opacity: 0.22, depthWrite: false }),
      );
      beam.rotation.x = Math.PI / 2;
      beam.position.z = BEAM_M / 2;
      const label = sign(s.name);
      label.position.z = 6;
      holder.add(ring, beam, label);
      this.group.add(holder);
    }
  }

  /** One frame: the HUD line for the player at `pos` (sim m). */
  frame(player: number, pos: [number, number, number] | null): void {
    const d = this.data;
    if (!d || !pos) return this.show("");
    const site = d.sites.find((s) => Math.hypot(pos[0] - s.pos[0], pos[1] - s.pos[1]) <= s.radius_m && Math.abs(pos[2] - s.pos[2]) <= 4);
    if (!site) return this.show("");
    const st = sim.policeStatus(player);
    if (!st || st.level === "None") return this.show(`${site.name} · safehouse (use it to lose the police while wanted)`);
    const t = sim.safehouseProgress(player);
    if (t <= 0) return this.show(`${site.name} · hiding needs no police within ${Math.round(d.clear_radius_m)} m`);
    this.show(`${site.name} · hiding ${Math.min(t, d.dwell_s).toFixed(0)} / ${d.dwell_s.toFixed(0)} s`);
  }

  private show(text: string): void {
    if (text === this.text) return;
    this.text = text;
    this.hud.textContent = text;
    this.hud.style.display = text ? "block" : "none";
  }
}

/** The floating "SAFEHOUSE" sign over a safehouse. */
function sign(name: string): THREE.Sprite {
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
    g.strokeStyle = "#3dff8f";
    g.stroke();
    g.textAlign = "center";
    g.fillStyle = "#ffffff";
    g.font = "700 40px system-ui, sans-serif";
    g.fillText(name, w / 2, 64, w - 40);
    g.fillStyle = "#3dff8f";
    g.font = "600 32px system-ui, sans-serif";
    g.fillText("Safehouse · lose the police here", w / 2, 116, w - 40);
  }
  const tex = new THREE.CanvasTexture(c);
  tex.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, transparent: true, depthWrite: false, fog: false }));
  sprite.scale.set(13, 3.8, 1);
  sprite.renderOrder = 10;
  return sprite;
}
