/**
 * District names and label spots for the full map. The city data has no zone names yet
 * (CITY.md C2 asks for `Zone.name`), so the names live here by zone index: Port Vesper v2's
 * from `crates/wp_world/src/vesper.rs` and `docs/design/CITY.md` §2, and the combined world's
 * (`ashpeak`: Ashpeak County's zones first, then Port Vesper's, `vesper.rs` `region.zones()`
 * then `zones()`) from `crates/wp_world/src/vesper/ashpeak/{metro,farms,mountains}.rs`. Other
 * cities fall back to the district kind.
 */
import type { MapStatic, MapZone } from "./data";
import { pointInPolygon } from "../ui/mapGeometry";

/** Port Vesper v2 zone names (`vesper::zones()` order). `null` = the catch-all outskirts. */
const VESPER_V2: (string | null)[] = [
  "Lantern Row",
  "Midtown",
  "Harbor Blvd",
  "Gantry Row",
  "Canal Flats",
  "Mercer",
  "Eastgate",
  "Hollins Heights",
  "Bayview Bluffs",
  "Larkspur",
  null,
];

/** Ashpeak County zone names in the combined world (`ashpeak::zones()` order: metro, farms,
 * mountains). Port Vesper's zones follow them. */
const ASHPEAK_COUNTY: string[] = [
  "Ashpeak Downtown",
  "Coliseum",
  "Granite District",
  "Ashpeak Waterfront",
  "Ashpeak Docks",
  "Waterfront Lofts",
  "Westbrook",
  "Northgate Heights",
  "Harnell Crossing",
  "Harnell Crossing",
  "Harnell Flats",
  "Harnell Flats",
  "Ashpeak Range",
];

/** Zone names of each city with named zones, by zone index. */
const ZONE_NAMES: Record<string, (string | null)[]> = {
  vesper_v2: VESPER_V2,
  ashpeak: [...ASHPEAK_COUNTY, ...VESPER_V2],
};

/** City display names. */
export const CITY_NAMES: Record<string, string> = { vesper_v2: "Port Vesper", ashpeak: "Ashpeak County", procedural_12x12: "Grid City" };

/** Display name of a zone (`null` = don't label it). */
export function districtName(city: string, zone: MapZone): string | null {
  const names = ZONE_NAMES[city];
  if (names) return zone.id < names.length ? names[zone.id] : zone.kind;
  return zone.kind;
}

/** A district label. */
export interface DistrictLabel {
  zone: number;
  name: string;
  kind: MapZone["kind"];
  x: number;
  y: number;
}

function poly2(flat: number[]): [number, number][] {
  const out: [number, number][] = [];
  for (let i = 0; i + 1 < flat.length; i += 2) out.push([flat[i], flat[i + 1]]);
  return out;
}

/**
 * Label spot per named zone: sample a grid over the map, give each sample to the first zone that
 * contains it (the same rule the world uses), and put each label at the zone's sample nearest
 * its sample mean. That keeps the label of a ring-shaped zone (Midtown around Lantern Row) on
 * the ring, not in the hole. Zones of the same name share one label. Deterministic.
 */
export function districtLabels(map: MapStatic, grid = 48): DistrictLabel[] {
  const [x0, y0, x1, y1] = map.bounds;
  const polys = map.zones.map((z) => poly2(z.poly));
  const samples: [number, number][][] = map.zones.map(() => []);
  for (let j = 0; j < grid; j++) {
    for (let i = 0; i < grid; i++) {
      const p: [number, number] = [x0 + ((i + 0.5) / grid) * (x1 - x0), y0 + ((j + 0.5) / grid) * (y1 - y0)];
      const k = polys.findIndex((poly) => poly.length >= 3 && pointInPolygon(p, poly));
      if (k >= 0) samples[k].push(p);
    }
  }
  const byName = new Map<string, { zone: MapZone; pts: [number, number][] }>();
  map.zones.forEach((z, k) => {
    const name = districtName(map.city, z);
    if (!name || samples[k].length === 0) return;
    const e = byName.get(name);
    if (e) e.pts.push(...samples[k]);
    else byName.set(name, { zone: z, pts: [...samples[k]] });
  });
  const out: DistrictLabel[] = [];
  for (const [name, { zone, pts }] of byName) {
    const mx = pts.reduce((s, p) => s + p[0], 0) / pts.length;
    const my = pts.reduce((s, p) => s + p[1], 0) / pts.length;
    let best = pts[0];
    let bd = Infinity;
    for (const p of pts) {
      const d = (p[0] - mx) ** 2 + (p[1] - my) ** 2;
      if (d < bd) [bd, best] = [d, p];
    }
    out.push({ zone: zone.id, name, kind: zone.kind, x: best[0], y: best[1] });
  }
  return out;
}

/** Zone index at a world point (first containing zone), or -1. */
export function zoneAt(map: MapStatic, x: number, y: number): number {
  return map.zones.findIndex((z) => pointInPolygon([x, y], poly2(z.poly)));
}
