// Chase radar blips for the UI kit's minimap (ChaseHud): police cars, AI racers and the
// helicopter, which is drawn as Air-1 with a tether to whoever its spotlight is on.
import type { MapBlip } from "../../../ui-kit/src/model/types";
import { HELI_STRIDE, HELI_TRACKS_PLAYER } from "../heli_view";
import { META_STRIDE, S, STATE_STRIDE, VEHICLE_KIND } from "../wasm/layout";

/** Floats per unit in police_units(). */
const POLICE_UNIT_STRIDE = 6;

/** The player's position and heading (0 = north, clockwise) plus every blip around them. */
export interface Radar {
  pos: [number, number];
  headingRad: number;
  blips: MapBlip[];
}

/** Heading (0 = +Y, clockwise) of a state record's body: its +Y axis rotated by the quaternion. */
function headingOf(st: Float32Array, o: number): number {
  const [x, y, z, w] = [st[o + S.rot], st[o + S.rot + 1], st[o + S.rot + 2], st[o + S.rot + 3]];
  const fx = 2 * (x * y - w * z);
  const fy = 1 - 2 * (x * x + z * z);
  return Math.atan2(fx, fy);
}

/**
 * Build the radar. `states` / `meta` = vehicle_states() / vehicle_meta(), `units` =
 * police_units(), `helis` = police_helicopters(player). Null when the player has no car.
 */
export function buildRadar(player: number, states: Float32Array, meta: Float32Array, units: Float32Array, helis: Float32Array): Radar | null {
  const kinds = new Map<number, number>();
  for (let o = 0; o < meta.length; o += META_STRIDE) kinds.set(meta[o], meta[o + 1]);
  let me: Radar | null = null;
  const blips: MapBlip[] = [];
  for (let o = 0; o < states.length; o += STATE_STRIDE) {
    const id = states[o + S.entity];
    const pos: [number, number] = [states[o + S.pos], states[o + S.pos + 1]];
    const headingRad = headingOf(states, o);
    if (id === player) {
      me = { pos, headingRad, blips };
      continue;
    }
    const kind = kinds.get(id);
    if (kind === VEHICLE_KIND.police) blips.push({ id, kind: "police", pos, headingRad });
    else if (kind === VEHICLE_KIND.aiRacer) blips.push({ id, kind: "racer", pos, headingRad });
  }
  if (!me) return null;

  // Spotlight aims, in police_helicopters() order (helicopters on station only).
  const aims: { at: [number, number]; pos: [number, number]; onPlayer: boolean }[] = [];
  for (let o = 0; o < helis.length; o += HELI_STRIDE) {
    aims.push({ at: [helis[o], helis[o + 1]], pos: [helis[o + 3], helis[o + 4]], onPlayer: (helis[o + 6] & HELI_TRACKS_PLAYER) !== 0 });
  }
  let n = 0;
  for (let o = 0, i = 0; o < units.length; o += POLICE_UNIT_STRIDE, i++) {
    const pos: [number, number] = [units[o], units[o + 1]];
    // police_units() yaw is counter-clockwise from +Y; the minimap's heading is clockwise.
    const headingRad = -units[o + 3];
    if (units[o + 5] & 1) {
      n++;
      // Match a helicopter to its spotlight by position (police_units() also lists
      // helicopters returning to base, which have no spotlight).
      const aim = aims.find((a) => a.at[0] === pos[0] && a.at[1] === pos[1]);
      // The tether goes to the player's own position when they are the target: it reads as
      // "following you" even while the spotlight lags behind.
      const tracking = aim ? (aim.onPlayer ? me.pos : aim.pos) : undefined;
      blips.push({ id: 100000 + i, kind: "helicopter", pos, headingRad, label: `AIR-${n}`, tracking });
    } else {
      blips.push({ id: 100000 + i, kind: "police", pos, headingRad });
    }
  }
  return me;
}
