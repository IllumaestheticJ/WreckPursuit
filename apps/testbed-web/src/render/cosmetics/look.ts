// A car's cosmetic look: every installed mod drawn on its panel mesh (data/cosmetics/visuals.json
// → the builders in body.ts, shell.ts, wheels.ts and cabin.ts). Render only: the sim never sees a look.
//
// Bolt-on pieces hang off the vehicle root in chassis space and ride with their body region's
// lattice (regionShift), so a bull bar follows a crushed bumper and a wing goes when the trunk
// lid comes off. Wheel pieces sit in the view's wheel groups. Material looks (paint finish,
// vinyl, tint, glazing, interior, calipers, tyres) are applied after every art-style
// change through `PanelBody.onStyled`, since the style resets those materials.
import * as THREE from "three";
import type { VehicleInfo } from "../../wasm/sim";
import { loadCarMesh, DEFAULT_ATLAS, type PanelBody } from "../panels";
import { BODY_BUILDERS } from "./body";
import { CABIN_BUILDERS } from "./cabin";
import type { Builder, LookCtx, Tweaks } from "./context";
import { CarFrame } from "./frame";
import { kitMats } from "./kit";
import { SHELL_BUILDERS } from "./shell";
import { liveryCanvas } from "./vinyls";
import { WHEEL_BUILDERS } from "./wheels";
import { loadVisuals, piecesOf } from "./visuals";

/** What a look needs from the vehicle view (VehicleView implements it). */
export interface LookHost {
  readonly root: THREE.Object3D;
  readonly panels: PanelBody;
  readonly info: VehicleInfo;
  /** Wheel `i`'s spinning group (`spin`) or its steering-only group. */
  wheelGroup(i: number, spin: boolean): THREE.Object3D;
  /** Head / tail lamp lens style (smoked, xenon, amber, halo; undefined = stock). */
  setLampStyle(style: string | undefined): void;
}

/** Every builder by piece name. */
const BUILDERS: Record<string, Builder> = { ...BODY_BUILDERS, ...SHELL_BUILDERS, ...WHEEL_BUILDERS, ...CABIN_BUILDERS };

/** Piece names the builders know (tests / data checks). */
export const PIECES = Object.keys(BUILDERS);

interface BodyPiece {
  obj: THREE.Object3D;
  base: THREE.Vector3;
  region: string | null;
}

/** Paint finishes: roughness, metalness (and a colour for chrome). */
const FINISH: Record<string, { roughness: number; metalness: number; colour?: number }> = {
  gloss: { roughness: 0.18, metalness: 0.15 },
  metallic: { roughness: 0.3, metalness: 0.65 },
  matte: { roughness: 0.95, metalness: 0.0 },
  pearl: { roughness: 0.22, metalness: 0.45 },
  satin: { roughness: 0.55, metalness: 0.25 },
  chrome: { roughness: 0.06, metalness: 1.0, colour: 0xdfe4ea },
};

const tmp = new THREE.Vector3();

/** One car's look. Build with `CarLook.build`; `update` every frame, `dispose` when done. */
export class CarLook {
  /** Body pieces (child of the view root, chassis space). */
  readonly root = new THREE.Group();
  /** Show lamps lit regardless of the headlights (the workshop showroom). */
  showcase = false;
  private readonly pieces: BodyPiece[] = [];
  private readonly wheelPieces: THREE.Object3D[] = [];
  private readonly lamps: { m: THREE.MeshStandardMaterial; glow: number }[] = [];
  private readonly tweaks: Tweaks = {};
  private canvas: HTMLCanvasElement | null = null;
  private texture: THREE.CanvasTexture | null = null;
  private lit: boolean | null = null;
  private disposed = false;
  /** Regions whose panel is off the car in the workshop: what is bolted to them is not drawn. */
  private removed: ReadonlySet<string> = new Set();

  private constructor(
    private readonly host: LookHost,
    readonly frame: CarFrame,
    readonly mods: readonly string[],
  ) {
    this.root.name = "cosmetics";
  }

  /** Build the look of `mods` on a view whose panel mesh is ready (null: nothing visible to add). */
  static async build(host: LookHost, mods: readonly string[]): Promise<CarLook | null> {
    const [visuals, mesh] = await Promise.all([loadVisuals(), loadCarMesh(host.info.car_id)]);
    if (!visuals || !mesh) return null;
    const pieces = piecesOf(mods, visuals);
    if (!pieces.length) return null;
    const look = new CarLook(host, new CarFrame(mesh, host.info.wheels, mesh.atlas ?? DEFAULT_ATLAS), mods);
    const ctx = look.context();
    for (const p of pieces) {
      const b = BUILDERS[p.piece];
      if (b) b(ctx, p);
      else console.warn(`cosmetics: no builder for piece "${p.piece}"`);
    }
    look.finish();
    return look;
  }

  private context(): LookCtx {
    const host = this.host;
    return {
      frame: this.frame,
      kit: kitMats(),
      paint: host.panels.paint,
      tweaks: this.tweaks,
      body: (obj, region, slot) => {
        obj.userData.slot = slot;
        this.root.add(obj);
        this.pieces.push({ obj, base: obj.position.clone(), region });
      },
      wheel: (i, obj, spin, slot) => {
        if (i >= host.info.wheels.length) return;
        obj.userData.slot = slot;
        obj.userData.cosmetic = true;
        host.wheelGroup(i, spin).add(obj);
        this.wheelPieces.push(obj);
      },
      hide: (pick) => host.panels.hideParts((def) => pick(def)),
      lamp: (m, glow) => this.lamps.push({ m, glow }),
    };
  }

  /** After the builders ran: wheel sizes, lamp styles, materials, then follow restyles. */
  private finish(): void {
    const host = this.host;
    host.root.add(this.root);
    const t = this.tweaks;
    if (t.wheelWidth || t.dragRadials) {
      host.info.wheels.forEach((w, i) => {
        const rear = w.position_m[1] < 0;
        const sx = t.wheelWidth ?? (t.dragRadials ? (rear ? 1.12 : 0.8) : 1);
        const sr = t.dragRadials && rear ? 1.05 : 1;
        for (const m of host.panels.wheelParts[i]) {
          const def = host.panels.wheelDef(m);
          if (def?.kind === "tire") m.scale.set(sx, sr, sr);
          else if (def?.kind === "rim") m.scale.x = sx;
        }
      });
      for (const o of this.wheelPieces) o.scale.x = t.wheelWidth ?? 1;
    }
    host.setLampStyle(t.lamps);
    host.panels.onStyled = () => this.applyMaterials();
    this.applyMaterials();
    this.update();
  }

  /** Re-apply the material looks (after the art style reset them). */
  private applyMaterials(): void {
    if (this.disposed) return;
    const panels = this.host.panels;
    const t = this.tweaks;
    const paint = panels.paint;
    const fin = t.finish ? FINISH[t.finish] : undefined;
    if (fin?.colour !== undefined) paint.color.setHex(fin.colour);
    const body = panels.liveryMaterial === paint ? [paint] : [paint, panels.liveryMaterial];
    if (fin) for (const m of body) Object.assign(m, { roughness: fin.roughness, metalness: fin.metalness });
    // Livery: the vinyl on the body colour.
    this.canvas = liveryCanvas(this.frame, paint.color, t.vinyl, this.canvas);
    if (this.canvas) {
      if (!this.texture) {
        this.texture = new THREE.CanvasTexture(this.canvas);
        this.texture.colorSpace = THREE.SRGBColorSpace;
        this.texture.anisotropy = 8;
      }
      this.texture.needsUpdate = true;
      panels.setLivery(this.texture);
    }
    if (t.tint || t.glass) {
      const g = panels.ownSlot("glass");
      if (t.glass === "shatterproof") g.color.lerp(new THREE.Color(0x6fb0a4), 0.45);
      if (t.glass === "poly") g.color.lerp(new THREE.Color(0xd8dcc8), 0.4);
      if (t.tint === "dark") {
        g.color.multiplyScalar(0.3);
        g.opacity = Math.max(g.opacity, 0.78);
      } else if (t.tint === "limo") {
        g.color.setHex(0x060708);
        g.opacity = 0.94;
      }
    }
    if (t.interior !== undefined || t.strip) {
      const m = panels.ownSlot("interior");
      if (t.interior !== undefined) m.color.setHex(t.interior);
      if (t.strip) {
        m.color.setHex(0x70757b);
        m.metalness = 0.5;
        m.roughness = 0.5;
      }
    }
    if (t.caliper !== undefined) panels.ownSlot("caliper").color.setHex(t.caliper);
    if (t.tyreGloss !== undefined) panels.ownSlot("rubber").roughness = 0.95 - 0.6 * t.tyreGloss;
  }

  /** Headlights on or off: lamp pieces (fog lamps, halos, LED bar, underglow) follow. */
  setLights(on: boolean): void {
    const lit = on || this.showcase;
    if (lit === this.lit) return;
    this.lit = lit;
    for (const { m, glow } of this.lamps) {
      m.emissiveIntensity = lit ? glow : 0;
      const pool = m.userData.pool as THREE.MeshBasicMaterial | undefined;
      if (pool) pool.opacity = lit ? 1 : 0;
    }
  }

  /**
   * Workshop: the panels of these body regions are off the car, so the pieces bolted to them
   * (a lip on the front bumper, flares on a fender) are not drawn either.
   */
  setRemovedRegions(regions: ReadonlySet<string>): void {
    this.removed = regions;
    this.update();
  }

  /** Per frame, after the panel mesh re-skinned: pieces ride with their regions. */
  update(): void {
    const panels = this.host.panels;
    for (const p of this.pieces) {
      if (!p.region) continue;
      p.obj.visible = !panels.regionGone(p.region) && !this.removed.has(p.region);
      p.obj.position.copy(p.base).add(panels.regionShift(p.region, tmp));
    }
  }

  /** The workshop slot of the piece `ray` hits first (null: no piece, or one with no slot). */
  slotAt(ray: THREE.Raycaster): { slot: string | null; distance: number } | null {
    const targets: THREE.Object3D[] = [this.root, ...this.wheelPieces];
    const hit = ray.intersectObjects(targets, true).find((h) => h.object.visible);
    if (!hit) return null;
    let o: THREE.Object3D | null = hit.object;
    while (o && !("slot" in o.userData)) o = o.parent;
    return { slot: (o?.userData.slot as string | null) ?? null, distance: hit.distance };
  }

  private glow: THREE.Mesh[] = [];
  private glowMat: THREE.MeshBasicMaterial | null = null;

  /** Glow over the pieces of `slots` (workshop selection); an empty set clears it. */
  highlight(slots: ReadonlySet<string>, colour = 0x39ff88): void {
    for (const m of this.glow) m.removeFromParent();
    this.glow = [];
    if (!slots.size) return;
    const material = (this.glowMat ??= new THREE.MeshBasicMaterial({ color: colour, transparent: true, opacity: 0.45, depthTest: false, depthWrite: false }));
    for (const top of [...this.root.children, ...this.wheelPieces]) {
      if (!slots.has(top.userData.slot as string)) continue;
      top.traverse((o) => {
        if (!(o instanceof THREE.Mesh) || this.glow.includes(o)) return;
        const g = new THREE.Mesh(o.geometry, material);
        g.renderOrder = 10;
        o.add(g);
        this.glow.push(g);
      });
    }
  }

  /** Remove the pieces (the panel mesh keeps hidden stock parts until the view is rebuilt). */
  dispose(): void {
    this.disposed = true;
    if (this.host.panels.onStyled) this.host.panels.onStyled = null;
    this.root.removeFromParent();
    for (const o of this.wheelPieces) o.removeFromParent();
    this.texture?.dispose();
  }
}
