import { describe, expect, it } from "vitest";
import type { SimEvent } from "../generated/contracts";
import type { PoliceEvent } from "../model/police";
import type { LiveBillJson } from "./bill";
import { createLiveModel } from "./LiveModel";

const police = (p: PoliceEvent, kind: number): SimEvent => ({ PoliceEvent: { kind, payload: [...new TextEncoder().encode(JSON.stringify(p))] } });

const bill: LiveBillJson = {
  entity: 1,
  car_id: "debug_sedan",
  display_name: "Debug Sedan",
  class: "Compact",
  base_price: 18000,
  health: 0.8,
  total_loss: false,
  mode: "Arcade",
  estimate: { lines: [], total: 0 },
  quote: {
    lines: [{ item: { Region: "FrontBumper" }, severity: 0.5, parts: 100, labour: 50, total: 150 }],
    labour_multiplier: 1,
    gross: 150,
    insurance_covers: 0,
    claim_denied: false,
    player_pays: 150,
    salvage: 2000,
    reports_to_police: false,
  },
  error: null,
};

describe("live model", () => {
  it("folds live police events into the chase readout (level + pursuit, heat unknown)", () => {
    const m = createLiveModel();
    m.push({ tick: 10, player: 1, events: [police({ WantedLevelChanged: { racer: 1, level: "L2" } }, 2)] });
    expect(m.get().hud.heat.level).toBe("L2");
    expect(m.get().hud.heat.state).toBe("noticed");
    expect(m.get().hud.heat.heatKnown).toBe(false);
    m.push({ tick: 20, player: 1, events: [police({ PursuitStarted: { racer: 1, unit: 3 } }, 3), police({ Radio: { unit: 3, text: "Unit 3 in pursuit." } }, 1)] });
    expect(m.get().hud.heat.state).toBe("pursuit");
    expect(m.get().scanner.some((s) => s.text === "Unit 3 in pursuit.")).toBe(true);
    m.push({ tick: 30, player: 1, events: [police({ PursuitEnded: { racer: 1, outcome: "Escaped" } }, 4)] });
    expect(m.get().hud.heat.state).toBe("clear");
  });

  it("prefers the police record when the host provides it", () => {
    const m = createLiveModel();
    m.push({ tick: 1, player: 1, events: [], police: { level: "L3", heat: 60, status: "Searching", bounty: 1234.4, stalenessS: 7, escapeS: 14, unitsAssigned: 2 } });
    const h = m.get().hud.heat;
    expect(h.heatKnown).toBe(true);
    expect(h.heat).toBeCloseTo(0.5); // 60 within L3's 50..70 band
    expect(h.state).toBe("cooldown");
    expect(h.cooldown).toBeCloseTo(0.5);
    expect(h.bounty).toBe(1234);
    expect(h.unitsInPursuit).toBe(2);
  });

  it("prices the bill through the host and closes via back", () => {
    let asked = "";
    let closed = false;
    const m = createLiveModel({
      quote: (_p, r) => {
        asked = `${r.shop}/${r.option}/${r.wanted}`;
        return bill;
      },
      onBack: () => {
        closed = true;
      },
    });
    m.push({ tick: 1, player: 1, events: [police({ WantedLevelChanged: { racer: 1, level: "L1" } }, 2)] });
    m.openBill();
    expect(asked).toBe("Legit/RepairAll/L1");
    expect(m.get().bill?.lines[0].label).toBe("Front Bumper — panel");
    expect(m.get().bill?.playerPays).toBe(150);
    m.send({ type: "bill.request", request: { shop: "Crooked" } });
    expect(asked).toBe("Crooked/RepairAll/L1");
    m.send({ type: "back" });
    expect(closed).toBe(true);
  });
});

describe("live police readout (wp_wasm::police_status_json)", () => {
  it("drives level, bounty, units and the escape bar from the record", () => {
    const m = createLiveModel();
    const readout = { level: "L2" as const, heat: 40, status: "Searching" as const, bounty: 1234.4, stalenessS: 9, searchS: 6, escapeS: 12, phase: "Night" as const, unitsAssigned: 3 };
    m.push({ tick: 240, player: 1, events: [], police: readout });
    const h = m.get().hud.heat;
    expect(h.level).toBe("L2");
    expect(h.state).toBe("cooldown");
    expect(h.bounty).toBe(1234);
    expect(h.unitsInPursuit).toBe(3);
    expect(h.cooldown).toBeCloseTo(0.5);
    expect(h.heatKnown).toBe(true);
    expect(h.bountyKnown).toBe(true);
  });
});
