// What a cosmetic piece builder gets: the car's geometry, materials, and ways to add pieces,
// hide stock parts and ask for material changes. Implemented by look.ts.
import type * as THREE from "three";
import type { MeshPartJson } from "../panels";
import type { CarFrame } from "./frame";
import type { KitMats } from "./kit";
import type { Piece } from "./visuals";

/** Material changes of a look, re-applied after every art-style change (see look.ts). */
export interface Tweaks {
  /** Paint finish: gloss, metallic, matte, pearl, satin, chrome. */
  finish?: string;
  /** Vinyl design id (vinyls.ts). */
  vinyl?: string;
  /** Window tint. */
  tint?: "dark" | "limo";
  /** Glazing: polycarbonate rear windows or laminated shatter-proof glass. */
  glass?: "poly" | "shatterproof";
  /** Interior colour (sRGB hex). */
  interior?: number;
  /** Head / tail lamp style: smoked, xenon, amber, halo. */
  lamps?: string;
  /** Brake caliper colour. */
  caliper?: number;
  /** Tyre rubber: 0 = matte street, 1 = glossy slick. */
  tyreGloss?: number;
  /** Drag radials: tall rear tyres, skinny fronts. */
  dragRadials?: boolean;
  /** Interior stripped: 1 = bare trim, 2 = interior removed (one race seat). */
  strip?: number;
}

/** Builder context (one per look build). */
export interface LookCtx {
  frame: CarFrame;
  kit: KitMats;
  /** This car's body paint (pieces in body colour use it, so they follow paint and style). */
  paint: THREE.MeshStandardMaterial;
  /**
   * Add a body piece in chassis space. It rides with `region`'s lattice (a crushed bumper takes
   * the bull bar with it) and disappears when that panel comes off; `slot` is the workshop part
   * slot it belongs to (clicking it in the workshop selects that slot).
   */
  body(obj: THREE.Object3D, region: string | null, slot: string | null): void;
  /** Add a piece to wheel `i` in wheel space: `spin` turns with the wheel, else it steers only. */
  wheel(i: number, obj: THREE.Object3D, spin: boolean, slot: string | null): void;
  /** Stop drawing the stock parts `pick` selects (replaced by a piece). */
  hide(pick: (def: MeshPartJson) => boolean): void;
  /** A lamp lit with the headlights (emissive at `glow` intensity when on). */
  lamp(m: THREE.MeshStandardMaterial, glow: number): void;
  tweaks: Tweaks;
}

/** A piece builder. */
export type Builder = (ctx: LookCtx, p: Piece) => void;
