import { describe, expect, it } from "vitest";
import { ChaseEndDetector, isChased } from "./chaseEnd";

describe("ChaseEndDetector", () => {
  it("fires once when a pursuit is escaped", () => {
    const d = new ChaseEndDetector();
    expect(d.update("Wanted", false)).toBeNull();
    expect(d.update("Pursuit", false)).toBeNull();
    expect(d.update("Searching", false)).toBeNull();
    expect(d.update("Clear", false)).toBe("escaped");
    expect(d.update("Clear", false)).toBeNull();
  });

  it("fires once when the car is written off mid-chase", () => {
    const d = new ChaseEndDetector();
    d.update("Pursuit", false);
    expect(d.update("Pursuit", true)).toBe("wrecked");
    expect(d.update("Pursuit", true)).toBeNull();
  });

  it("never fires without a chase, and forgets on reset", () => {
    const d = new ChaseEndDetector();
    expect(d.update(null, false)).toBeNull();
    expect(d.update("Wanted", true)).toBeNull();
    d.update("Pursuit", false);
    d.reset();
    expect(d.update("Clear", false)).toBeNull();
    expect(isChased(undefined)).toBe(false);
  });
});
