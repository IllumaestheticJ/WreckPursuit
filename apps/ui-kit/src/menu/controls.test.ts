import { describe, expect, it } from "vitest";
import { defaultControls, defaultDeviceControls, normalizeControls, normalizeDeviceControls, shapePedal, shapePedals, shapeSteer } from "./controls";
import { defaultGameSettings, normalizeSettings } from "./settings";

const SAMPLES = [-1, -0.75, -0.3, -0.01, 0, 0.01, 0.2, 0.5, 0.9, 1];

describe("input shaping", () => {
  it("defaults: owner keyboard values, identity steer and pedals on every device", () => {
    const d = defaultControls();
    expect(d.keyboard).toEqual({
      steerDeadzone: 0,
      steerSaturation: 1,
      steerSensitivity: 1,
      steerSpeedSensitivity: 0.75,
      throttleDeadzone: 0,
      throttleSaturation: 1,
      brakeDeadzone: 0,
      brakeSaturation: 1,
      clutchDeadzone: 0,
      clutchSaturation: 1,
    });
    expect(d.gamepad.steerSpeedSensitivity).toBe(0.7);
    expect(d.wheel.steerSpeedSensitivity).toBe(0);
    for (const c of Object.values(d)) {
      for (const x of SAMPLES) expect(shapeSteer(x, c)).toBeCloseTo(x, 12);
      for (const x of SAMPLES.filter((v) => v >= 0)) {
        expect(shapePedals({ throttle: x, brake: x, clutch: x }, c)).toEqual({ throttle: x, brake: x, clutch: x });
      }
    }
  });

  it("dead zone: nothing at or below it, then rescaled to full range", () => {
    const c = { ...defaultDeviceControls("keyboard"), steerDeadzone: 0.3 };
    expect(shapeSteer(0.2, c)).toBe(0);
    expect(shapeSteer(0.3, c)).toBe(0);
    expect(shapeSteer(0.65, c)).toBeCloseTo(0.5);
    expect(shapeSteer(1, c)).toBe(1);
    expect(shapePedal(0.1, 0.2, 1)).toBe(0);
    expect(shapePedal(0.6, 0.2, 1)).toBeCloseTo(0.5);
  });

  it("saturation: full output from the saturation point on", () => {
    const c = { ...defaultDeviceControls("keyboard"), steerSaturation: 0.5 };
    expect(shapeSteer(0.25, c)).toBeCloseTo(0.5);
    expect(shapeSteer(0.5, c)).toBe(1);
    expect(shapeSteer(0.8, c)).toBe(1);
    expect(shapePedal(0.4, 0, 0.8)).toBeCloseTo(0.5);
    expect(shapePedal(0.9, 0, 0.8)).toBe(1);
    // A degenerate window (sat <= dz) still returns a finite 0..1 value.
    const z = shapeSteer(0.6, { ...c, steerDeadzone: 0.5, steerSaturation: 0.5 });
    expect(z).toBe(1);
  });

  it("sensitivity: exponent 1 + 2·(1 − sensitivity)", () => {
    const base = defaultDeviceControls("keyboard");
    expect(shapeSteer(0.5, { ...base, steerSensitivity: 0.5 })).toBeCloseTo(0.25);
    expect(shapeSteer(0.5, { ...base, steerSensitivity: 0 })).toBeCloseTo(0.125);
    expect(shapeSteer(1, { ...base, steerSensitivity: 0 })).toBe(1);
    // Dead zone + saturation first, then the exponent.
    expect(shapeSteer(0.6, { ...base, steerDeadzone: 0.2, steerSaturation: 1, steerSensitivity: 0.5 })).toBeCloseTo(0.25);
  });

  it("keeps the sign of the steering", () => {
    const c = { ...defaultDeviceControls("gamepad"), steerDeadzone: 0.1, steerSaturation: 0.9, steerSensitivity: 0.3 };
    for (const x of [0.2, 0.5, 0.95]) {
      expect(shapeSteer(-x, c)).toBeCloseTo(-shapeSteer(x, c), 12);
      expect(shapeSteer(x, c)).toBeGreaterThan(0);
    }
    expect(shapeSteer(Number.NaN, c)).toBe(0);
    expect(shapePedal(-0.5, 0, 1)).toBe(0);
  });

  it("normalises untrusted stored values into range", () => {
    const base = defaultDeviceControls("keyboard");
    const n = normalizeDeviceControls({ steerDeadzone: 0.9, steerSaturation: 0.1, steerSensitivity: "max", steerSpeedSensitivity: -2, brakeSaturation: Infinity }, base);
    expect(n.steerDeadzone).toBe(0.5);
    expect(n.steerSaturation).toBe(0.5);
    expect(n.steerSensitivity).toBe(base.steerSensitivity);
    expect(n.steerSpeedSensitivity).toBe(0);
    expect(n.brakeSaturation).toBe(base.brakeSaturation);
    const all = normalizeControls({ gamepad: { steerDeadzone: 0.2 }, wheel: "bad" }, defaultControls());
    expect(all.keyboard).toEqual(defaultControls().keyboard);
    expect(all.gamepad.steerDeadzone).toBe(0.2);
    expect(all.wheel).toEqual(defaultControls().wheel);
  });

  it("old stored settings without controls load with the defaults", () => {
    const base = defaultGameSettings(true);
    const n = normalizeSettings({ version: 1, units: "mph" }, base);
    expect(n.controls).toEqual(defaultControls());
    const m = normalizeSettings({ controls: { keyboard: { steerSpeedSensitivity: 0.5 } } }, base);
    expect(m.controls.keyboard.steerSpeedSensitivity).toBe(0.5);
    expect(m.controls.keyboard.steerSaturation).toBe(1);
  });
});
