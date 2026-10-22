// Garage workshop: which part slot (data/economy/parts.toml) a panel-mesh part belongs to.
//
// The car meshes (data/panels/meshes/<car>.json) tag every part with its body region and kind;
// the slot comes from those, with a few names for the engine bay and the exhaust. Parts with no
// slot (the cabin shell, sills, rear quarters: welded body) cannot be taken off.
import type { MeshPartJson } from "../render/panels";

const CORNERS = ["fl", "fr", "rl", "rr"] as const;

/** Body region → slot for panel / glass / detail parts. */
const REGION_SLOT: Record<string, string> = {
  FrontBumper: "front_bumper",
  RearBumper: "rear_bumper",
  Hood: "hood",
  Trunk: "trunk_lid",
  FrontLeftFender: "fender_fl",
  FrontRightFender: "fender_fr",
  LeftDoorF: "door_fl",
  RightDoorF: "door_fr",
  LeftDoorR: "door_rl",
  RightDoorR: "door_rr",
  Windshield: "windshield",
  RearGlass: "rear_glass",
  Accessory: "push_bar",
};

/** Side of a part from its name suffix (`_l` / `_r`), else from its centre x. */
function side(name: string, centreX: number): "l" | "r" {
  if (/_l(_|$)/.test(name)) return "l";
  if (/_r(_|$)/.test(name)) return "r";
  return centreX < 0 ? "l" : "r";
}

/**
 * The slot of mesh part `def` (centre x in chassis metres for the side), or null when the part
 * is welded body. Wheel-space parts use their wheel index (0–3: FL, FR, RL, RR).
 */
export function slotOfPart(def: MeshPartJson, centreX: number): string | null {
  const n = def.name;
  if (def.wheel != null) {
    const c = CORNERS[def.wheel] ?? "fl";
    if (def.kind === "tire") return `tyre_${c}`;
    if (def.kind === "rim") return `wheel_${c}`;
    if (def.kind === "brake_disc" || def.kind === "caliper") return `brake_${c}`;
    return `suspension_${c}`; // upright
  }
  if (def.kind === "light") return "lights";
  if (def.kind === "interior") return n === "interior" ? "seats" : null;
  if (def.kind === "engine") {
    if (n.includes("radiator")) return "radiator";
    if (n.includes("intercooler")) return "intercooler";
    if (n.includes("charge_pipes")) return "turbo";
    if (n.includes("headers")) return "exhaust";
    return "engine";
  }
  if (def.kind === "suspension") {
    if (n.startsWith("exhaust")) return "exhaust";
    if (n.startsWith("driveline")) return "driveshaft";
    if (n.startsWith("susp")) {
      const front = def.region === "FrameFront" || n.includes("front");
      return `suspension_${front ? "f" : "r"}${side(n, centreX)}`;
    }
    return null; // frame rails and cross members
  }
  // Bolt-on aero and scoops modelled on some cars: their own slots, not the panel under them.
  if (n.startsWith("wing") || n === "spoiler") return "rear_wing";
  if (n === "splitter") return "front_splitter";
  if (n.startsWith("hood_scoop")) return "hood_vent";
  if (n.startsWith("roof_scoop")) return "roof_rack";
  if (def.region && REGION_SLOT[def.region]) {
    if (def.region === "Accessory" && !n.startsWith("pushbar")) return null;
    return REGION_SLOT[def.region];
  }
  if (n.startsWith("skirt")) return "side_skirts";
  return null;
}
