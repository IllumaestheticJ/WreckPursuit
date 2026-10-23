// Title screen front door (owner-approved feature F5): the data the game menu's title and its
// Free roam screen show (ui-kit GameMenu: Continue career / Free roam (city + mode) / Races /
// Touge / Settings / Controls / Developer), and the loading-screen steps a session start runs.

import type { CareerSummary, CityChoice } from "../../../ui-kit/src/live";
import type { LoadStep } from "./loading";

/** Cities for Title → Free roam (ids = the developer drawer's `#city-map` values). */
export const CITIES: CityChoice[] = [
  { id: "vesper_v2", label: "Port Vesper", hint: "Hills, curves, the harbour and the Loop 9 highway" },
  { id: "ashpeak", label: "Port Vesper + Ashpeak County", hint: "The city plus mountain passes, dirt roads and countryside" },
  { id: "procedural_12x12", label: "Grid city", hint: "A flat 12 × 12 grid: long straights, easy to learn" },
];

/** Display name of a city id (the id itself when unknown). */
export const cityLabel = (id: string): string => CITIES.find((c) => c.id === id)?.label ?? id;

/** The fields of the career view the title line needs. */
export interface CareerLine {
  day: number;
  cash: number;
  rep: number;
  run_active: boolean;
  active_car: number | null;
  cars: { uid: number; name: string }[];
}

/** The title's career item: "Continue career" with night / cash / car, or "New career". */
export function careerSummary(v: CareerLine | null): CareerSummary {
  if (!v || v.cars.length === 0) return { label: "New career", detail: "Buy a car, run the night, earn rep and cash" };
  const car = v.cars.find((c) => c.uid === v.active_car) ?? v.cars[0];
  const cash = `$${Math.round(v.cash).toLocaleString("en-US")}`;
  return { label: "Continue career", detail: `Night ${v.day} · ${cash} · ${v.rep} rep · ${car.name}${v.run_active ? " · run in progress" : ""}` };
}

/** Steps of a session start behind the loading screen: build the session (blocking), wait for
 * the car meshes (bounded), then one painted frame. */
export function sessionSteps(start: () => void, meshes: () => Promise<unknown>): LoadStep[] {
  return [
    { label: "Building the city and its traffic…", run: start },
    { label: "Loading cars…", run: () => Promise.race([meshes(), new Promise((r) => setTimeout(r, 5000))]) },
    { label: "First frame…", run: () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))) },
  ];
}
