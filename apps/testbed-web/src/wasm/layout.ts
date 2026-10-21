// Packed array layouts returned by wp_wasm. Mirrors crates/wp_wasm/src/pack.rs — keep in sync
// (documented in docs/integration/testbed.md).

/** Floats per vehicle in vehicle_states(). */
export const STATE_STRIDE = 50;
/** Field offsets inside one state record. */
export const S = {
  entity: 0,
  pos: 1, // 3
  rot: 4, // 4, quaternion [x, y, z, w]
  linVel: 8, // 3
  angVel: 11, // 3
  speed: 14,
  rpm: 15,
  gear: 16,
  health: 17,
  wheelBase: 18,
  wheelStride: 8,
} as const;
/** Offsets inside a wheel block of a state record. */
export const SW = {
  omega: 0,
  steer: 1,
  compression: 2,
  slipAngle: 3,
  slipRatio: 4,
  fz: 5,
  contact: 6,
  detached: 7,
} as const;

/** Offsets inside telemetry(). */
export const T = {
  speed: 0,
  rpm: 1,
  gear: 2,
  steerAngle: 3,
  health: 4,
  gLong: 5,
  gLat: 6,
  gVert: 7,
  yawRate: 8,
  inThrottle: 9,
  inBrake: 10,
  inSteer: 11,
  inHandbrake: 12,
  wheelBase: 13,
  wheelStride: 10,
  componentBase: 53,
} as const;
/** Offsets inside a telemetry wheel block. */
export const TW = {
  slipAngle: 0,
  slipRatio: 1,
  load: 2,
  surface: 3,
  compression: 4,
  contact: 5,
  detached: 6,
  omega: 7,
  /** Applied longitudinal tire force, N (along the wheel heading). */
  fx: 8,
  /** Applied lateral tire force, N (positive = toward the wheel's left). */
  fyLeft: 9,
} as const;

/** set_input() flag bits. */
export const INPUT_FLAG = {
  gearUp: 1,
  gearDown: 2,
  horn: 4,
  headlights: 8,
  siren: 16,
} as const;

/** world_info() offsets. */
export const W = { tick: 0, hours: 1, night: 2, mode: 3, replay: 4, recording: 5, city: 6, scripted: 7 } as const;

/** Floats per head in signal_heads() (crates/wp_wasm/src/signals.rs). */
export const SIGNAL_HEAD_STRIDE = 10;
/** Offsets inside one signal_heads() record. */
export const SH = { x: 0, y: 1, z: 2, yaw: 3, left: 4, right: 5, radius: 6, centreZ: 7, intersection: 8, group: 9 } as const;
/** signal_states() codes, one byte per head. */
export const SIGNAL = { off: 0, red: 1, yellow: 2, green: 3 } as const;

/** Floats per agent in traffic_agents(): [x, y, z, yaw, flags]. */
export const AGENT_STRIDE = 5;
/** traffic_agents() flag bits (variant in bits 8+). */
export const AGENT_FLAG = {
  physical: 1,
  brake: 2,
  hazard: 4,
  yielding: 8,
  turnLeft: 16,
  turnRight: 32,
  aggressive: 64,
} as const;

/** Floats per intent arrow in intents(). */
export const INTENT_STRIDE = 8;

/** Floats per vehicle in vehicle_meta(): [entity, kind, flags]. */
export const META_STRIDE = 3;
/** vehicle_meta() kinds (`wp_sim::VehicleKind`). */
export const VEHICLE_KIND = { player: 0, aiRacer: 1, police: 2, traffic: 3 } as const;
/** vehicle_meta() flag bits (from the input the vehicle drives with). */
export const META_FLAG = { siren: 1, headlights: 2, brake: 4, horn: 8 } as const;

/** Floats per feature in city_features() (crates/wp_wasm/src/features.rs). Box and cylinder
 * features only (prisms are in the greybox), network order:
 * `[shape, material, kind, cx, cy, cz, hx, hy, hz, yaw, pitch]` in sim space (+Z up).
 * Box: centre, half extents (local x right, y forward, z up), yaw about +Z (0 = local +Y north,
 * CCW) applied after pitch about local +X (positive = the +Y end up).
 * Cylinder: centre = base + height / 2, hx = hy = radius, hz = height / 2, yaw = pitch = 0.
 * `material` / `kind` are wp_world `Material` / `FeatureKind` indices (declaration order). */
export const FEATURE_STRIDE = 11;
/** Field offsets inside one city_features() record. */
export const FT = { shape: 0, material: 1, kind: 2, cx: 3, cy: 4, cz: 5, hx: 6, hy: 7, hz: 8, yaw: 9, pitch: 10 } as const;
/** city_features() shape codes. */
export const FEATURE_SHAPE = { box: 0, cylinder: 1 } as const;
/** Material indices the testbed treats specially (feature_palette() has 4 floats per index). */
export const FEATURE_MATERIAL = { carPaint: 22, light: 23 } as const;
/** Spatial chunk size for instanced features, m (frustum culling + draw distance). */
export const FEATURE_CHUNK_M = 512;
