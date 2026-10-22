// Shared palette for the "stylised low-poly dark" look (machine S, docs/art/STYLE_DECISION.md).
// Every workstream that draws something takes its colours from here. Base values come from the
// "Stylised low-poly dark" mode of docs/art/style_picker.html (the page the owner chose from);
// city-scale values (fog density, lamp and window strengths) are tuned for the testbed's
// 300 m+ chase views instead of the picker's 60 m street.
//
// Colours are sRGB hex (as an artist reads them). Glow strengths are linear multipliers on top
// of the colour: anything above ~1 reaches the bloom pass at night (render/post.ts).

/** sRGB hex colour. */
export type Hex = number;

/** District order of `wp_contracts::DistrictId` (map_static_json zone `kind` names). */
export const DISTRICTS = ["Core", "Commercial", "Residential", "Industrial", "Port", "Suburb"] as const;
/** One district name. */
export type District = (typeof DISTRICTS)[number];

/** Per-district look: building tiers, ground, window and street-light character. */
export interface DistrictLook {
  /** Three building façade tiers (picked per façade so blocks are not one flat colour). */
  buildings: [Hex, Hex, Hex];
  /** District ground (city v2 `ground_<district>` terrain). */
  ground: Hex;
  /** Lit window colour (warm = homes, cool = offices). */
  window: Hex;
  /** Share of windows lit at full night, 0..1. */
  windowsLit: number;
  /** Street-light kind: sodium (orange) or LED (white-blue). */
  lamp: "sodium" | "led";
}

/** The low-poly dark palette. */
export const PALETTE = {
  // ---- streets ----
  asphalt: 0x2b2e38,
  highway: 0x3a3d47,
  parking: 0x30333d,
  busLane: 0x3a2f33,
  marking: 0x8d8b84,
  markingYellow: 0x8f7a45,
  sidewalk: 0x3b3e4a,
  kerb: 0x555866,
  bridge: 0x474a56,
  retainingWall: 0x3c3f4b,
  /** Level lot behind the sidewalk (front strip of every frontage). */
  lot: 0x343a37,
  // ---- residential lots (playtest row 95) ----
  /** Grass verge between kerb and walkway, residential streets. */
  verge: 0x2f3c31,
  /** Pitched roofs (clay). */
  roof: 0x3d3234,
  garage: 0x46434a,
  driveway: 0x44464f,
  hedge: 0x27362a,
  fence: 0x4a4540,
  treeTrunk: 0x352c28,
  treeCrown: 0x2b3d2e,
  /** Flat test world ground (no city). */
  flatGround: 0x2a2d36,
  rock: 0x3d3a40,
  seabed: 0x2c3036,
  water: 0x1a2430,

  // ---- buildings / windows ----
  /** Unlit window glass (by day, and dark windows at night). */
  windowDark: 0x1f2533,
  /** Window glass by day (reflects the grey sky; blends to windowDark at night). */
  windowDay: 0x4a5568,
  windowWarm: 0xf3c37a,
  windowCool: 0xb8d0e8,
  /** Lit-window glow at full night. */
  windowGlow: 0.8,

  // ---- street lights / signals ----
  lampPole: 0x3a3d48,
  lampSodium: 0xffc27a,
  lampLed: 0xe4ecff,
  /** Lamp-head glow at night. */
  lampGlow: 4,
  /** Light pool on the road under each lamp (additive), peak strength at night. */
  lampPool: 0.26,
  signalRed: 0xff4a3a,
  signalYellow: 0xffc040,
  signalGreen: 0x5fe0a0,
  /** Signal-lamp glow (they read at night without being brighter than police bars). */
  signalGlow: 2.5,

  // ---- cars ----
  /** Player and scenario cars, in order (muted, slightly desaturated). */
  carPaints: [0xc9913a, 0x4f7396, 0x5d6f63, 0xa04a3a, 0x6b5a7a, 0x4f8a86] as Hex[],
  /** Civilian traffic paints (picked by agent / entity id). */
  trafficPaints: [0x6b5a7a, 0x5d6f63, 0x6d7078, 0x4a4e5a, 0x7a6a55, 0x56606e, 0x8a8f96, 0x3d4250] as Hex[],
  policePaint: 0xd8dbe0,
  glass: 0x1d2633,
  tire: 0x18181c,
  rim: 0x6d7078,
  trim: 0x1a1b20,
  interior: 0x22201f,
  headlight: 0xf4f1e6,
  /** Headlight lens glow when the headlights are on. */
  headlightGlow: 2.4,
  taillight: 0xe04848,
  /** Hazard / indicator amber. */
  hazard: 0xffa030,
  /** Brake-light glow (running tail lights are a quarter of this). */
  brakeGlow: 3,
  brokenLight: 0x202227,

  // ---- police ----
  lightbarBase: 0x16171b,
  policeRed: 0xff3030,
  policeBlue: 0x3060ff,
  /** Light-bar glow: the brightest thing in the frame at night (≥ 3× any other light). */
  policeRedGlow: 13,
  policeBlueGlow: 15,
  /** The scene's one siren point light (scene.ts setSiren): intensity per unit of bar glow. */
  sirenLightPerGlow: 2,

  // ---- props (parked cars use trafficPaints through render/fleet.ts styledPaint) ----
  /** Traffic cone body (muted orange; a faint self-glow keeps it readable at night). */
  propCone: 0xd0682c,
  propConeGlow: 0x2a0e00,
  /** Extra cone glow at full night (multiplies propConeGlow: 1 + this × night). */
  propConeNightGlow: 3,
} as const;

/** Siren point-light intensity for a lit light-bar side of glow `glow` (palette units). */
export function sirenLightIntensity(glow: number): number {
  return glow * PALETTE.sirenLightPerGlow;
}

/** Per-district looks (`DISTRICTS` order). */
export const DISTRICT_LOOKS: Record<District, DistrictLook> = {
  Core: { buildings: [0x2f3a4a, 0x363a4c, 0x283240], ground: 0x34363f, window: 0x9fb8d0, windowsLit: 0.4, lamp: "led" },
  Commercial: { buildings: [0x3a3446, 0x423c4a, 0x33303e], ground: 0x383640, window: 0xc8ccd0, windowsLit: 0.3, lamp: "led" },
  Residential: { buildings: [0x433a3c, 0x4a4040, 0x3a3436], ground: 0x2f3833, window: 0xf3c37a, windowsLit: 0.35, lamp: "sodium" },
  Industrial: { buildings: [0x3c3d40, 0x44423e, 0x35373b], ground: 0x37352f, window: 0xe8c890, windowsLit: 0.12, lamp: "sodium" },
  Port: { buildings: [0x34393f, 0x3d3a36, 0x2e3438], ground: 0x30333a, window: 0xe8c890, windowsLit: 0.1, lamp: "sodium" },
  Suburb: { buildings: [0x473e3a, 0x40403c, 0x3c3836], ground: 0x2d3630, window: 0xf3c37a, windowsLit: 0.25, lamp: "sodium" },
};

/** Look for a zone kind name; unknown names fall back to Commercial. */
export function districtLook(name: string): DistrictLook {
  return DISTRICT_LOOKS[(DISTRICTS as readonly string[]).includes(name) ? (name as District) : "Commercial"];
}

/** Greybox glTF material name → palette colour (data/city/*.greybox.glb). */
export const CITY_MATERIAL_COLOURS: Record<string, Hex> = {
  asphalt: PALETTE.asphalt,
  highway_concrete: PALETTE.highway,
  parking: PALETTE.parking,
  bus_lane: PALETTE.busLane,
  sidewalk: PALETTE.sidewalk,
  sidewalk_corner: PALETTE.sidewalk,
  lot: PALETTE.lot,
  lot_skirt: PALETTE.lot,
  kerb: PALETTE.kerb,
  verge: PALETTE.verge,
  roof: PALETTE.roof,
  garage: PALETTE.garage,
  driveway: PALETTE.driveway,
  hedge: PALETTE.hedge,
  fence: PALETTE.fence,
  tree_trunk: PALETTE.treeTrunk,
  tree_crown: PALETTE.treeCrown,
  bridge: PALETTE.bridge,
  retaining_wall: PALETTE.retainingWall,
  rock: PALETTE.rock,
  seabed: PALETTE.seabed,
  water: PALETTE.water,
  ground_core: DISTRICT_LOOKS.Core.ground,
  ground_commercial: DISTRICT_LOOKS.Commercial.ground,
  ground_residential: DISTRICT_LOOKS.Residential.ground,
  ground_industrial: DISTRICT_LOOKS.Industrial.ground,
  ground_port: DISTRICT_LOOKS.Port.ground,
  ground_suburb: DISTRICT_LOOKS.Suburb.ground,
  // Buildings get per-district tiers in the building shader (render/style.ts); this is the base.
  building: DISTRICT_LOOKS.Commercial.buildings[0],
};

/** Sky, fog, light and post settings at one hour of the day. */
export interface TimeOfDayLook {
  /** Hour, 0..24. */
  hour: number;
  skyTop: Hex;
  /** Horizon colour; the fog uses the same colour so the city fades into the sky. */
  horizon: Hex;
  /** FogExp2 density, 1/m (soft distance fog; weather adds its own on top). */
  fogDensity: number;
  hemiSky: Hex;
  hemiGround: Hex;
  hemiIntensity: number;
  sun: Hex;
  sunIntensity: number;
  /** Sun elevation, degrees (moon at night). */
  sunElevationDeg: number;
  /** Bloom threshold (linear scene colour); high by day so only lights bloom. */
  bloomThreshold: number;
  bloomStrength: number;
  exposure: number;
}

/**
 * Time-of-day keyframes (dawn / day / dusk / night), interpolated by hour. Night stays
 * readable: a blue-biased moon fill and a hemisphere light keep silhouettes visible, the fog
 * is a dark blue-grey rather than black.
 */
export const TIME_OF_DAY: TimeOfDayLook[] = [
  { hour: 0, skyTop: 0x0e1120, horizon: 0x1d2030, fogDensity: 0.0042, hemiSky: 0x5a6080, hemiGround: 0x1a1a22, hemiIntensity: 3.2, sun: 0x8090c0, sunIntensity: 0.6, sunElevationDeg: 40, bloomThreshold: 1.1, bloomStrength: 0.7, exposure: 1.3 },
  { hour: 4.5, skyTop: 0x0e1120, horizon: 0x1d2030, fogDensity: 0.0042, hemiSky: 0x5a6080, hemiGround: 0x1a1a22, hemiIntensity: 3.2, sun: 0x8090c0, sunIntensity: 0.6, sunElevationDeg: 40, bloomThreshold: 1.1, bloomStrength: 0.7, exposure: 1.3 },
  // Dawn: cold blue top, dusty pink-grey horizon, low sun.
  { hour: 6.5, skyTop: 0x34405e, horizon: 0x8a7a80, fogDensity: 0.0034, hemiSky: 0x8a90a8, hemiGround: 0x2a2a30, hemiIntensity: 2.2, sun: 0xffc8a0, sunIntensity: 1.2, sunElevationDeg: 10, bloomThreshold: 1.6, bloomStrength: 0.55, exposure: 1.1 },
  // Day: overcast-leaning, muted, slight blue bias.
  { hour: 9, skyTop: 0x5d7598, horizon: 0x9aa6b4, fogDensity: 0.0016, hemiSky: 0xb4bed0, hemiGround: 0x3a3a40, hemiIntensity: 2.6, sun: 0xfff2e0, sunIntensity: 2.4, sunElevationDeg: 45, bloomThreshold: 2.6, bloomStrength: 0.4, exposure: 1.05 },
  { hour: 16, skyTop: 0x5d7598, horizon: 0x9aa6b4, fogDensity: 0.0016, hemiSky: 0xb4bed0, hemiGround: 0x3a3a40, hemiIntensity: 2.6, sun: 0xfff2e0, sunIntensity: 2.4, sunElevationDeg: 40, bloomThreshold: 2.6, bloomStrength: 0.4, exposure: 1.05 },
  // Dusk: violet top, amber horizon.
  { hour: 19, skyTop: 0x2c2f4a, horizon: 0x8a5e52, fogDensity: 0.0032, hemiSky: 0x7a7090, hemiGround: 0x241e24, hemiIntensity: 2.1, sun: 0xff9a60, sunIntensity: 1.0, sunElevationDeg: 8, bloomThreshold: 1.4, bloomStrength: 0.6, exposure: 1.1 },
  { hour: 21, skyTop: 0x141724, horizon: 0x2c2f44, fogDensity: 0.004, hemiSky: 0x5a6080, hemiGround: 0x1a1a22, hemiIntensity: 3.2, sun: 0x8090c0, sunIntensity: 0.6, sunElevationDeg: 40, bloomThreshold: 1.1, bloomStrength: 0.7, exposure: 1.3 },
  { hour: 24, skyTop: 0x0e1120, horizon: 0x1d2030, fogDensity: 0.0042, hemiSky: 0x5a6080, hemiGround: 0x1a1a22, hemiIntensity: 3.2, sun: 0x8090c0, sunIntensity: 0.6, sunElevationDeg: 40, bloomThreshold: 1.1, bloomStrength: 0.7, exposure: 1.3 },
];

/** Post-process constants (render/post.ts). */
export const POST = {
  /** Saturation after tonemapping (the picker's 0.85: muted). */
  saturation: 0.88,
  /** Shadow lift, linear RGB (slight blue bias in the blacks). */
  lift: [0.008, 0.01, 0.018] as [number, number, number],
  vignette: 0.22,
  grain: 0.012,
} as const;
