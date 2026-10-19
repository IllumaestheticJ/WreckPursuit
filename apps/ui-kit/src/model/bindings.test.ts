import { describe, expect, it } from "vitest";
import { bind, codeLabel, conflictFor, defaultBindings, detectPadInput, deviceOf, unbind } from "./bindings";

describe("input bindings", () => {
  it("swaps codes on conflict", () => {
    const b = defaultBindings();
    expect(conflictFor(b, "horn", "keyboard", "KeyW")).toBe("throttle");
    const next = bind(b, "horn", "keyboard", "KeyW");
    expect(next.horn.keyboard).toBe("KeyW");
    expect(next.throttle.keyboard).toBe("KeyH");
    expect(b.horn.keyboard).toBe("KeyH"); // input untouched
  });

  it("unbinds the other action when the rebound one had no code", () => {
    const b = defaultBindings();
    const next = bind(b, "clutch", "gamepad", "button:0");
    expect(next.clutch.gamepad).toBe("button:0");
    expect(next.handbrake.gamepad).toBeUndefined();
  });

  it("refuses to unbind required keyboard actions", () => {
    const b = defaultBindings();
    expect(unbind(b, "pause", "keyboard")).toBe(b);
    expect(unbind(b, "horn", "keyboard").horn.keyboard).toBeUndefined();
  });

  it("detects new pad input against a baseline", () => {
    const base = { buttons: [0, 0, 0], axes: [0, -1] };
    expect(detectPadInput({ buttons: [0, 1, 0], axes: [0, -1] }, base)).toBe("button:1");
    // A pedal resting at -1 does not bind; pressing it to +1 does.
    expect(detectPadInput({ buttons: [0, 0, 0], axes: [0, -0.9] }, base)).toBeNull();
    expect(detectPadInput({ buttons: [0, 0, 0], axes: [0, 0.5] }, base)).toBe("axis:1+");
  });

  it("classifies devices and labels codes", () => {
    expect(deviceOf("Generic USB Racing Wheel (Vendor: 1234 Product: 5678)")).toBe("wheel");
    expect(deviceOf("Generic Gamepad (STANDARD GAMEPAD)")).toBe("gamepad");
    expect(codeLabel("KeyW")).toBe("W");
    expect(codeLabel("ArrowLeft")).toBe("←");
    expect(codeLabel("axis:2-")).toBe("Axis 2−");
    expect(codeLabel(undefined)).toBe("—");
  });
});
