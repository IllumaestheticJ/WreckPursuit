/**
 * What the career screen may do where the player is (owner 2026-10-27: dealers, garages and
 * workshops have to be driven to; the menu only sets a waypoint). The host tells the screen
 * where the player pulled in (`CareerApi.place`); everything that needs a garage or a dealer
 * is open only there:
 *
 * - a repair shop: workshop, mods, tune, tyres, repairs at that shop;
 * - the player's own garage: workshop, mods, tune, tyres (no mechanic: repairs are shop work);
 * - a safehouse: the workshop (as its pull-in panel offers);
 * - a car lot: selling to the dealer, walking that lot.
 *
 * A car that cannot be driven is worked on where it sits (`CarView.spot`), not where the player
 * is: at a repair shop the tow took it to, everything of that shop; anywhere else (a wreck left
 * on the road, a dead car at home) it needs the tow service first (`tow`: "Tow to …", owner
 * 2026-10-28). Hosts that do not say where cars sit keep the old rule (a wreck's repairs are
 * open anywhere). A taxi to any car lot is always open, car or no car (owner 2026-10-29); the
 * player's car stays where it is, and the next run starts from it (`lotActions`).
 * Insurance, the pound, informants and the bounty are phone calls: open anywhere.
 */
import type { CareerPlace, CarSpotView, CarView } from "./types";

/** What is open for one car at one place. */
export interface Access {
  workshop: boolean;
  mods: boolean;
  tune: boolean;
  tires: boolean;
  sell: boolean;
  repair: boolean;
  /** Repairs are at this shop (by name) only; null = the panel's own pick (old hosts). */
  repairShop: string | null;
  /** The car cannot be driven and is not at a repair shop: offer the tow service ("Tow to …"). */
  tow: boolean;
}

/** Everything open, repairs at any shop: hosts that do not say where the player is. */
export const OPEN: Access = { workshop: true, mods: true, tune: true, tires: true, sell: true, repair: true, repairShop: null, tow: false };

/** What a place offers (the player, or a car that cannot move, is there). */
function atPlace(place: CareerPlace | null): Access {
  const kind = place?.kind ?? null;
  const garage = kind === "Garage" || kind === "Home";
  const shop = kind === "Garage";
  return {
    workshop: garage || kind === "Safehouse",
    mods: garage,
    tune: garage,
    tires: garage,
    sell: kind === "Lot",
    repair: shop,
    repairShop: shop ? place!.name : null,
    tow: false,
  };
}

/** What is open for `car` with the player at `place` (`undefined`: the host does not track it). */
export function garageAccess(place: CareerPlace | null | undefined, car: Pick<CarView, "drivable"> & { spot?: CarSpotView | null }): Access {
  if (place === undefined) return OPEN;
  if (car.drivable) return atPlace(place);
  if (car.spot === undefined) {
    // Old hosts: the wreck is towed in by whichever shop the player picks.
    const a = atPlace(place);
    return a.repair ? a : { ...a, repair: true, repairShop: null, tow: true };
  }
  if (car.spot === null) return atPlace(null);
  const where = car.spot.kind === "Road" ? null : { kind: car.spot.kind, id: car.spot.id, name: car.spot.name };
  return { ...atPlace(where), tow: car.spot.kind !== "Garage" };
}

/** What the career screen offers for one car lot. */
export interface LotActions {
  /** "Visit": walk the rows here (the player is at this lot; or the host does not track places). */
  walk: boolean;
  /** "Taxi": go there without the car (owner 2026-10-29: always, car or no car; the car stays
   * where it is and the next run starts from it). */
  taxi: boolean;
  /** "Waypoint": mark it on the map to drive there (only with a car that can drive). */
  waypoint: boolean;
}

/** What is open for lot `lotId` with the player at `place` (`undefined`: not tracked). Owner
 * report 2026-10-29 ("i also can't taxi to dealers once i own a car"): the taxi used to be the
 * fallback for players without a drivable car only; it is now always offered next to the
 * waypoint. */
export function lotActions(place: CareerPlace | null | undefined, lotId: string, hasDrivableCar: boolean): LotActions {
  const here = place === undefined || (place?.kind === "Lot" && place.id === lotId);
  return { walk: here, taxi: !here, waypoint: !here && hasDrivableCar };
}

/** One line on where the player is, for the career screen's head. */
export function placeLine(place: CareerPlace | null | undefined): string | null {
  if (place === undefined) return null;
  if (!place) return "Out on the road. Garages, workshops and car lots are drive-to: set a waypoint and pull in.";
  switch (place.kind) {
    case "Garage":
      return `At ${place.name}: repairs, mods, tuning and the workshop.`;
    case "Home":
      return `In your garage, ${place.name}: mods, tuning and the workshop. Repairs are at the shops.`;
    case "Safehouse":
      return `At ${place.name}: the workshop is here. Repairs and mods are at a garage.`;
    case "Lot":
      return `At ${place.name}: walk the lot, or sell a car to the dealer.`;
  }
}
