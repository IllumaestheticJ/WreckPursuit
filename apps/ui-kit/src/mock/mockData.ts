/**
 * Static mock content for the non-race screens (garage, market, lobby, results, impound, crews,
 * police career). Shapes follow `model/types.ts`; car and mod data are the real `data/` files.
 * Everything is deterministic (no Math.random).
 */
import type { RepairEstimate, RepairItem, VehicleClass } from "../generated/contracts";
import type {
  AccountView,
  CityMap,
  Crew,
  CrewState,
  GarageCar,
  ImpoundState,
  LobbyState,
  MarketListing,
  PoliceCareer,
  ResultsState,
  ShopView,
} from "../model/types";
import { CARS, isDebugCar } from "../data/catalog";
import {
  carValue,
  COP_RANK_THRESHOLDS,
  COP_RANKS,
  copLevel,
  impoundFee,
  IMPOUND,
  insuranceQuote,
  marketPrice,
  mockRepairLine,
} from "../data/economy";
import { previewBuild } from "../data/estimate";
import mapJson from "../../fixtures/city/procedural_12x12.map.json";

export { ASSIST_PRESETS, defaultSettings, PRESET_MODE, presetOf } from "../model/defaults";

/** The city map fixture as a `CityMap`. */
export function cityMap(): CityMap {
  const m = mapJson as unknown as {
    id: string;
    bounds_min: [number, number];
    bounds_max: [number, number];
    roads: { id: number; points: [number, number][]; speed_limit_mps: number }[];
    zones: { id: number; kind: string; polygon: [number, number][] }[];
    pois: { kind: string; name: string; pos: [number, number] }[];
  };
  return {
    id: m.id,
    boundsMin: m.bounds_min,
    boundsMax: m.bounds_max,
    roads: m.roads.map((r) => ({ id: r.id, points: r.points, speedLimitMps: r.speed_limit_mps })),
    zones: m.zones,
    pois: m.pois,
  };
}

function estimateOf(cls: VehicleClass, value: number, items: [RepairItem, number][]): RepairEstimate {
  const lines = items.map(([item, sev]) => mockRepairLine(item, sev, cls, value));
  return { lines, total: lines.reduce((s, l) => s + l.total, 0) };
}

function car(
  uid: number,
  defId: string,
  mods: string[],
  damage: { health: number; totalLoss?: boolean; items?: [RepairItem, number][]; captured?: boolean },
  extra: Partial<GarageCar> = {},
): GarageCar {
  const def = CARS[defId];
  const outstanding = estimateOf(def.class, def.base_price, damage.items ?? []);
  const base: GarageCar = {
    uid,
    defId,
    name: def.display_name,
    class: def.class,
    basePrice: def.base_price,
    mods,
    health: damage.health,
    totalLoss: !!damage.totalLoss,
    drivable: !damage.totalLoss,
    location: { kind: "Garage" },
    insurance: null,
    outstanding,
    sustainedBeforeCapture: !!damage.captured,
    value: 0,
    ...extra,
  };
  base.value = carValue(base);
  return base;
}

/** The player's garage. */
export function mockGarage(): GarageCar[] {
  const shiro = car(
    1,
    "shiro_turbo",
    ["engine_intake_1", "engine_exhaust_1", "engine_ecu_1", "prot_bumper_front"],
    {
      health: 0.71,
      captured: true,
      items: [
        [{ Region: "FrontBumper" }, 0.72],
        [{ Region: "Hood" }, 0.35],
        [{ Region: "LeftDoorF" }, 0.4],
        [{ Region: "FrameFront" }, 0.18],
        [{ Component: "Radiator" }, 0.6],
        [{ Component: "Lights" }, 1.0],
        [{ Component: "SuspensionFL" }, 0.38],
        [{ DetachedPart: "FrontBumper" }, 1.0],
      ],
    },
  );
  shiro.insurance = insuranceQuote(shiro, "basic");
  const kestrel = car(2, "kestrel_ts", ["tires_sport"], { health: 1 });
  const ironside = car(3, "ironside_440", [], {
    health: 0.08,
    totalLoss: true,
    items: [
      [{ Region: "FrameFront" }, 1.0],
      [{ Region: "Cabin" }, 0.65],
      [{ Region: "Hood" }, 1.0],
      [{ Component: "Engine" }, 0.95],
      [{ Component: "Radiator" }, 1.0],
      [{ Component: "SteeringRack" }, 0.8],
    ],
  });
  const vireo = car(4, "vireo_xs", [], { health: 0.88, items: [[{ Region: "RearBumper" }, 0.3]] }, {
    location: { kind: "Impound", sinceDay: 4, level: "L2" },
  });
  return [shiro, kestrel, ironside, vireo];
}

/** Repair shops (from the map's garage POIs). */
export function mockShops(): ShopView[] {
  return [
    { id: "garage_1", name: "Garage 1 — Midtown Body", kind: "legit", district: "Core", labourMultiplier: 1.25 },
    { id: "garage_2", name: "Garage 2 — Dockside (no questions)", kind: "crooked", district: "Docks", labourMultiplier: 0.8 },
    { id: "garage_3", name: "Garage 3 — Suburb Auto", kind: "legit", district: "Suburb", labourMultiplier: 1.0 },
  ];
}

/** Market: every civilian car in three conditions. */
export function mockMarket(): MarketListing[] {
  const out: MarketListing[] = [];
  for (const def of Object.values(CARS).sort((a, b) => a.base_price - b.base_price)) {
    if (isDebugCar(def.id) || def.class.startsWith("Police")) continue;
    const p = previewBuild(def, []);
    for (const condition of ["New", "Used", "Worn"] as const) {
      out.push({
        defId: def.id,
        name: def.display_name,
        analogOf: def.analog_of,
        class: def.class,
        condition,
        price: marketPrice(def.base_price, def.class, condition),
        classIndex: p.classIndex,
        bracket: p.bracket,
      });
    }
  }
  return out;
}

/** Account. */
export function mockAccount(): AccountView {
  const copXp = 2350;
  return { cash: 12850, rep: 3420, bounty: 6200, day: 7, copXp, copLevel: copLevel(copXp), inDebt: false };
}

/** Lobby for a street meet with asymmetric roles. */
export function mockLobby(): LobbyState {
  const ci = (id: string) => previewBuild(CARS[id], []).classIndex;
  return {
    eventName: "Harbor Midnight Sprint",
    kind: "Sprint",
    location: "Port → Core, 6.2 km",
    laps: 1,
    bracket: "B",
    maxClassIndex: 599,
    buyIn: 1500,
    purse: 9000,
    damageMode: "Arcade",
    policeEnabled: true,
    entrants: [
      { player: 1, name: "You", role: "Racer", car: "shiro_turbo", classIndex: ci("shiro_turbo"), ready: false, isLocal: true, crew: "NGT" },
      { player: 2, name: "kestrel_kid", role: "Racer", car: "kestrel_ts", classIndex: ci("kestrel_ts"), ready: true, isLocal: false, crew: "NGT" },
      { player: 3, name: "R0TOR", role: "Racer", car: "sperber_32", classIndex: ci("sperber_32"), ready: true, isLocal: false, crew: "DRY" },
      { player: 4, name: "lowrider_88", role: "Racer", car: "ironside_440", classIndex: ci("ironside_440"), ready: false, isLocal: false, crew: null },
      { player: 5, name: "Sgt. Okafor", role: "PoliceCaptain", car: null, classIndex: null, ready: true, isLocal: false, crew: null },
      { player: 6, name: "unit_nine", role: "PoliceChase", car: "sentinel_wolfhound", classIndex: ci("sentinel_wolfhound"), ready: true, isLocal: false, crew: null },
      { player: 7, name: "watcher", role: "Spectator", car: null, classIndex: null, ready: true, isLocal: false, crew: null },
    ],
    countdownS: null,
    maxRacers: 8,
    roleSlots: { Racer: 8, PoliceCaptain: 1, PoliceChase: 4, Spectator: 8 },
  };
}

/** Results of the last race. */
export function mockResults(): ResultsState {
  return {
    eventName: "Harbor Midnight Sprint",
    rows: [
      { position: 1, name: "You", car: "Shiro Turbo", timeS: 251.42, bestLapS: null, status: "Finished", isLocal: true },
      { position: 2, name: "R0TOR", car: CARS.sperber_32.display_name, timeS: 253.9, bestLapS: null, status: "Finished", isLocal: false },
      { position: 3, name: "kestrel_kid", car: CARS.kestrel_ts.display_name, timeS: 260.15, bestLapS: null, status: "Finished", isLocal: false },
      { position: 4, name: "lowrider_88", car: CARS.ironside_440.display_name, timeS: null, bestLapS: null, status: "Busted", isLocal: false },
    ],
    payout: [
      { label: "Purse — 1st of 4", cash: 4500, rep: 0, bounty: 0 },
      { label: "Clean race bonus (forfeited: contact)", cash: 0, rep: 0, bounty: 0 },
      { label: "Race reputation", cash: 0, rep: 180, bounty: 0 },
      { label: "Won while wanted", cash: 0, rep: 0, bounty: 2500 },
      { label: "Entry fee (paid at start)", cash: -1500, rep: 0, bounty: 0 },
    ],
    damageEstimate: 4180,
    cashBefore: 9850,
    cashAfter: 12850,
  };
}

/** Impound state from the garage. */
export function mockImpound(cars: GarageCar[], day: number): ImpoundState {
  return {
    entries: cars.flatMap((c) =>
      c.location.kind === "Impound"
        ? [
            {
              carUid: c.uid,
              name: c.name,
              daysIn: day - c.location.sinceDay,
              fee: impoundFee(c.class, day - c.location.sinceDay),
              feePerDay: IMPOUND.perDay[c.class],
              auctionInDays: IMPOUND.auctionAfterDays - (day - c.location.sinceDay),
              level: c.location.level,
              reason: "Captured at wanted level 2",
            },
          ]
        : [],
    ),
    finesOwed: 0,
    lastCapture: { level: "L2", fine: 1850, bail: 600, repLost: 240, fate: "Impounded" },
  };
}

/** Crews and territories (zones of the map, owners assigned deterministically). */
export function mockCrews(map: CityMap): CrewState {
  const crews: Crew[] = [
    { id: "NGT", name: "Night Shift", tag: "NGT", hue: 145, rep: 3420, members: 6, isPlayers: true },
    { id: "DRY", name: "Dry Docks", tag: "DRY", hue: 28, rep: 4100, members: 9, isPlayers: false },
    { id: "VLT", name: "Voltage", tag: "VLT", hue: 270, rep: 2600, members: 5, isPlayers: false },
    { id: "RST", name: "Rust Belt", tag: "RST", hue: 0, rep: 1900, members: 7, isPlayers: false },
  ];
  // Owner by zone kind with a deterministic scramble so the map looks lived in.
  const byKind: Record<string, (string | null)[]> = {
    Core: ["VLT", "VLT", "NGT", null],
    Commercial: ["NGT", "VLT", "NGT", "DRY"],
    Residential: [null, "NGT", null, "RST", null],
    Suburb: [null, null, "RST", null],
    Industrial: ["DRY", "RST", "DRY"],
    Port: ["DRY", "DRY", "NGT"],
  };
  const territories = map.zones.map((z) => {
    const opts = byKind[z.kind] ?? [null];
    const h = (z.id * 2654435761) >>> 0;
    const owner = opts[h % opts.length];
    return {
      zoneId: z.id,
      kind: z.kind,
      polygon: z.polygon,
      owner,
      control: owner ? 0.35 + ((h >>> 8) % 65) / 100 : 0,
      contested: owner !== null && (h >>> 4) % 7 === 0,
    };
  });
  return { crews, territories, pois: map.pois };
}

/** Police career for the police-side screen. */
export function mockPoliceCareer(xp: number): PoliceCareer {
  const level = copLevel(xp);
  const next = COP_RANK_THRESHOLDS[level] ?? xp;
  return {
    rank: COP_RANKS[level],
    level,
    xp,
    xpToNext: Math.max(0, next - xp),
    unlocks: [
      { id: "cruiser", name: "Patrol cruiser", level: 0, unlocked: true, description: "Sentinel Cruiser, siren, radio." },
      { id: "spikes", name: "Spike strips", level: 1, unlocked: level >= 1, description: "Deploy one strip ahead of a suspect." },
      { id: "pit", name: "PIT authorisation", level: 2, unlocked: level >= 2, description: "Contact manoeuvres without XP penalty at level 3+." },
      { id: "interceptor", name: "Interceptor", level: 3, unlocked: level >= 3, description: "Sentinel Wolfhound pursuit car." },
      { id: "roadblock", name: "Roadblocks", level: 4, unlocked: level >= 4, description: "Call a two-car roadblock." },
      { id: "air", name: "Air support", level: 5, unlocked: level >= 5, description: "Request the helicopter (spotlight, tracking)." },
      { id: "captain", name: "Captain's chair", level: 6, unlocked: level >= 6, description: "Command a district from the captain map." },
    ],
    stats: { busts: 14, escapes: 9, pursuits: 31, damageCaused: 48200 },
    orders: [],
    assignment: null,
  };
}
