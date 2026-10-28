import { describe, expect, it } from "vitest";
import { km, towDoneLine, towLedgerLabel } from "./Tow";

describe("tow labels", () => {
  it("names the ledger line after the destination and the distance", () => {
    expect(towLedgerLabel({ detail: { Tow: { to: "Gantry Garage", metres: 4200 } } })).toBe("Tow: Gantry Garage → 4.2 km");
    expect(towLedgerLabel({ detail: "Towing" })).toBeNull();
    expect(towLedgerLabel({ detail: null })).toBeNull();
    expect(km(700)).toBe("0.7 km");
  });

  it("says what the tow cost, and the debt it left", () => {
    const at = { kind: "Garage" as const, id: "Gantry Garage", name: "Gantry Garage" };
    expect(towDoneLine(at, 1300, 280, 500)).toBe("Towed to Gantry Garage: 1.3 km, $280.");
    expect(towDoneLine(at, 1300, 280, -240)).toBe("Towed to Gantry Garage: 1.3 km, $280. You owe $240.");
  });
});
