import { describe, expect, it } from "vitest";
import type { PursuitOutcome, SimEvent, WantedLevel } from "../generated/contracts";
import { ChaseEndDetector, chaseEndOf, isChased } from "./chaseEnd";

const ended = (racer: number, outcome: PursuitOutcome, level: WantedLevel): SimEvent => ({
  Police: { PursuitEnded: { racer, outcome, level, capturer: null, assists: [] } },
});
const totalLoss = (vehicle: number): SimEvent => ({ TotalLoss: { vehicle } });

describe("ChaseEndDetector", () => {
  it("reports the typed PursuitEnded with its pre-reset level", () => {
    const d = new ChaseEndDetector();
    expect(d.update(1, "Pursuit", "L3")).toBeNull();
    // The police reset the level on the same tick; the event still carries L3.
    expect(d.update(1, "Clear", "None", [ended(1, "Captured", "L3")])).toEqual({ how: "captured", outcome: "Captured", level: "L3" });
    expect(d.update(1, "Clear", "None")).toBeNull();
  });

  it("ignores other racers' pursuits", () => {
    const d = new ChaseEndDetector();
    d.update(1, "Pursuit", "L2");
    expect(d.update(1, "Pursuit", "L2", [ended(7, "Escaped", "L2")])).toBeNull();
  });

  it("falls back to status transitions without police events", () => {
    const d = new ChaseEndDetector();
    expect(d.update(1, "Wanted", "L1")).toBeNull();
    expect(d.update(1, "Pursuit", "L2")).toBeNull();
    expect(d.update(1, "Searching", "L4")).toBeNull();
    expect(d.update(1, "Clear", "None")).toEqual({ how: "escaped", outcome: null, level: "L4" });
    expect(d.update(1, "Clear", "None")).toBeNull();
  });

  it("uses the host's fresh outcome when the chase ends by status", () => {
    const d = new ChaseEndDetector();
    d.update(1, "Pursuit", "L3");
    expect(d.update(1, "Clear", "None", [], "Captured")).toEqual({ how: "captured", outcome: "Captured", level: "L3" });
  });

  it("reports a total loss mid-chase once, even if PursuitEnded follows a frame later", () => {
    const d = new ChaseEndDetector();
    d.update(1, "Pursuit", "L2");
    expect(d.update(1, "Pursuit", "L2", [totalLoss(1)])).toEqual({ how: "wrecked", outcome: null, level: "L2" });
    expect(d.update(1, "Clear", "None", [ended(1, "CapturedTotalLoss", "L2")])).toBeNull();
    expect(d.update(1, "Clear", "None")).toBeNull();
  });

  it("does not re-fire when the status lags behind the event", () => {
    const d = new ChaseEndDetector();
    d.update(1, "Pursuit", "L2");
    expect(d.update(1, "Pursuit", "L2", [ended(1, "Escaped", "L2")])?.how).toBe("escaped");
    expect(d.update(1, "Searching", "L2")).toBeNull();
    expect(d.update(1, "Clear", "None")).toBeNull();
    // A new chase is detected normally.
    d.update(1, "Pursuit", "L1");
    expect(d.update(1, "Clear", "None")?.how).toBe("escaped");
  });

  it("never fires without a chase, and forgets on reset", () => {
    const d = new ChaseEndDetector();
    expect(d.update(1, null, "None")).toBeNull();
    expect(d.update(1, "Wanted", "L1", [totalLoss(1)])).toBeNull();
    d.update(1, "Pursuit", "L1");
    d.reset();
    expect(d.update(1, "Clear", "None")).toBeNull();
    expect(isChased(undefined)).toBe(false);
  });

  it("maps every police outcome", () => {
    expect(chaseEndOf("Escaped")).toBe("escaped");
    expect(chaseEndOf("Safehouse")).toBe("escaped");
    expect(chaseEndOf("Captured")).toBe("captured");
    expect(chaseEndOf("CapturedTotalLoss")).toBe("wrecked");
    expect(chaseEndOf("CalledOff")).toBe("calledOff");
  });
});
