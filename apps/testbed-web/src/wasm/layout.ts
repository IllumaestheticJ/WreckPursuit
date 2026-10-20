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

/** Floats per record in traffic_draw() / police_draw() (crates/wp_wasm/src/lod.rs). */
export const DRAW_STRIDE = 10;
/** Offsets inside a draw record: id (agent / unit), entity (0 = none), pos xyz, rot quaternion xyzw, flags. */
export const D = { id: 0, entity: 1, pos: 2, rot: 5, flags: 9 } as const;
/** police_draw() flag bits. */
export const POLICE_DRAW_FLAG = { helicopter: 1, chasing: 2 } as const;

/** Floats per intent arrow in intents(). */
export const INTENT_STRIDE = 8;

/** Floats per vehicle in vehicle_meta(): [entity, kind, flags]. */
export const META_STRIDE = 3;
/** vehicle_meta() kinds (`wp_sim::VehicleKind`). */
export const VEHICLE_KIND = { player: 0, aiRacer: 1, police: 2, traffic: 3 } as const;
/** vehicle_meta() flag bits (from the input the vehicle drives with). */
export const META_FLAG = { siren: 1, headlights: 2, brake: 4, horn: 8 } as const;

/** Floats per record in police_tools(): [kind, x, y, z, qx, qy, qz, qw, hx, hy, hz]. */
export const TOOL_STRIDE = 11;
/** Offsets inside one police_tools() record (box centre, orientation, half extents). */
export const TL = { kind: 0, x: 1, y: 2, z: 3, qx: 4, qy: 5, qz: 6, qw: 7, hx: 8, hy: 9, hz: 10 } as const;
/** police_tools() kinds. */
export const TOOL_KIND = { barrier: 0, spikes: 1 } as const;

/** `race_standings()` record (wp_wasm::race::STANDING_STRIDE floats per racer). */
export const RACE_STRIDE = 10;
export const RS = {
  entity: 0,
  position: 1,
  /** 0 running, 1 finished, 2 DNF, 3 DQ. */
  status: 2,
  lap: 3,
  nextGate: 4,
  gatesPassed: 5,
  progressM: 6,
  /** −1 = none. */
  gapAheadS: 7,
  gapBehindS: 8,
  elapsedS: 9,
} as const;

/** `race_info()` fields. */
export const RI = {
  /** 0 countdown, 1 running, 2 finished. */
  phase: 0,
  countdownS: 1,
  elapsedS: 2,
  laps: 3,
  gates: 4,
  player: 5,
  racers: 6,
  closed: 7,
  lengthM: 8,
  autopilot: 9,
  /** 0 sprint, 1 circuit, 2 checkpoint, 3 other. */
  kind: 10,
  playerDone: 11,
} as const;

/** Floats per prop in props_static() (crates/wp_wasm/src/props.rs). */
export const PROP_STRIDE = 11;
/** Offsets inside one props_static() record: kind, box-centre pos xyz, rot quaternion xyzw, half extents xyz. */
export const P = { kind: 0, pos: 1, rot: 4, half: 8 } as const;
/** props_static() kind codes. */
export const PROP_KIND = { parkedCar: 0, cone: 1 } as const;
/** Floats per record in props_moved(): [prop index, x, y, z, qx, qy, qz, qw]. */
export const PROP_MOVED_STRIDE = 8;

/** Wheel-centre height in chassis space for a reported compression (matches wp_vehicle: the
 * hub sits at `position_m` at the static compression; playtest row 92 — drawing it at half
 * the travel left wheels 1–3 cm off the road). */
export function wheelCentreZ(
  wi: { position_m: [number, number, number]; travel_m: number; static_compression_m?: number },
  compression: number,
): number {
  return wi.position_m[2] + compression - (wi.static_compression_m ?? wi.travel_m * 0.5);
}
