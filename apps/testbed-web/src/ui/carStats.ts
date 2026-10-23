// Car stat bars under the car picker: acceleration, top speed and cornering for the selected
// car, measured on the physics rig (`simcli car-stats` → data/stats/cars.json, embedded in
// wp_wasm; `sim.carStats()`).
import type { CarStatsTable } from "../wasm/sim";

const BARS = [
  { key: "acceleration", label: "accel", value: (s: CarStat) => (s.zero_to_100_s === null ? "—" : `${s.zero_to_100_s.toFixed(1)} s 0-100`) },
  { key: "top_speed", label: "top speed", value: (s: CarStat) => `${Math.round(s.top_speed_kmh)} km/h` },
  { key: "cornering", label: "cornering", value: (s: CarStat) => `${s.cornering_g.toFixed(2)} g` },
] as const;

type CarStat = CarStatsTable["cars"][string];

/** One-line summary for tooltips. */
export function statLine(s: CarStat): string {
  return BARS.map((b) => `${b.label} ${b.value(s)}`).join(" · ");
}

/** The three bars for one car, redrawn by `show(carId)`. */
export class CarStatBars {
  private readonly fills: HTMLElement[] = [];
  private readonly values: HTMLElement[] = [];
  private readonly name: HTMLElement;
  /** The car's menu thumbnail (data/cars/thumbs/<car>.png, `car-assets gen-thumbs`). */
  private readonly thumb: HTMLImageElement;

  constructor(
    private readonly el: HTMLElement,
    private readonly table: CarStatsTable,
  ) {
    el.classList.add("car-stats");
    this.thumb = document.createElement("img");
    this.thumb.className = "car-stats-thumb";
    this.thumb.alt = "";
    this.thumb.width = 160;
    this.thumb.height = 90;
    this.thumb.onerror = () => (this.thumb.style.display = "none");
    el.append(this.thumb);
    this.name = document.createElement("div");
    this.name.className = "car-stats-name";
    el.append(this.name);
    for (const b of BARS) {
      const row = document.createElement("div");
      row.className = "car-stat";
      row.dataset.stat = b.key;
      const label = document.createElement("span");
      label.className = "dim";
      label.textContent = b.label;
      const track = document.createElement("span");
      track.className = "car-stat-track";
      const fill = document.createElement("i");
      track.append(fill);
      const value = document.createElement("span");
      value.className = "car-stat-value";
      row.append(label, track, value);
      el.append(row);
      this.fills.push(fill);
      this.values.push(value);
    }
    el.title = "Measured on the physics rig: 0-100 km/h, top speed, 50 m skidpad grip (data/stats/cars.json)";
  }

  /** Show `carId`'s bars (hidden when the car has no stats). */
  show(carId: string): void {
    const s = this.table.cars[carId];
    this.el.classList.toggle("hidden", !s);
    if (!s) return;
    this.el.dataset.car = carId;
    this.thumb.style.display = "";
    this.thumb.src = `data/cars/thumbs/${carId}.png`;
    this.name.textContent = s.display_name;
    BARS.forEach((b, i) => {
      const v = Math.min(1, Math.max(0, s.bars[b.key]));
      this.fills[i].style.width = `${(v * 100).toFixed(1)}%`;
      this.values[i].textContent = b.value(s);
    });
  }
}
