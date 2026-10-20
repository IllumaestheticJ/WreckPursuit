import { describe, expect, it } from "vitest";
import type { MapStatic } from "./data";
import { districtLabels, districtName, zoneAt } from "./districts";

const rect = (x0: number, y0: number, x1: number, y1: number) => [x0, y0, x1, y0, x1, y1, x0, y1];

// Port Vesper v2's first two zones (Core inside a Commercial ring) plus the outskirts.
const vesper: MapStatic = {
  city: "vesper_v2",
  bounds: [-1000, -1000, 1000, 1000],
  roads: [],
  zones: [
    { id: 0, kind: "Core", poly: rect(-250, -300, 250, 200) },
    { id: 1, kind: "Commercial", poly: rect(-350, -420, 350, 320) },
    { id: 10, kind: "Suburb", poly: rect(-1000, -1000, 1000, 1000) },
  ],
  pois: [],
  signals: [],
};

describe("districts", () => {
  it("names Port Vesper zones and skips the outskirts", () => {
    expect(districtName("vesper_v2", vesper.zones[0])).toBe("Lantern Row");
    expect(districtName("vesper_v2", vesper.zones[1])).toBe("Midtown");
    expect(districtName("vesper_v2", vesper.zones[2])).toBeNull();
    expect(districtName("procedural_12x12", vesper.zones[1])).toBe("Commercial");
  });

  it("puts a ring zone's label on the ring, not in the hole", () => {
    const labels = districtLabels(vesper);
    expect(labels.map((l) => l.name).sort()).toEqual(["Lantern Row", "Midtown"]);
    const mid = labels.find((l) => l.name === "Midtown")!;
    expect(zoneAt(vesper, mid.x, mid.y)).toBe(1);
    const core = labels.find((l) => l.name === "Lantern Row")!;
    expect(zoneAt(vesper, core.x, core.y)).toBe(0);
  });

  it("zoneAt uses the first containing zone", () => {
    expect(zoneAt(vesper, 0, 0)).toBe(0);
    expect(zoneAt(vesper, 300, 0)).toBe(1);
    expect(zoneAt(vesper, 900, 900)).toBe(2);
    expect(zoneAt(vesper, 5000, 0)).toBe(-1);
  });
});
