"""Point-mass performance estimator for roster design and the spec-sheet table.

This is *not* the game's vehicle model (that is `wp_vehicle`, workstream 02). It is a
deliberately simple, legible calculator used to sanity-check car data before the real sim
exists, and to give every car a comparable "estimated" column in `data/cars/VALIDATION.md`.

Models used (all SI):
- 0-100 km/h: 1-D integration (dt = 5 ms) of drive force through the gear that gives the most
  wheel force, a shift-time torque cut on every upshift, a clutch-slip launch rpm in 1st
  (engine inertia only counts once the clutch is locked),
  traction limited by the driven axle's normal load (with longitudinal load transfer
  `m·a·h/L`) and the tire's load-sensitive peak friction, minus aero drag and rolling
  resistance. Effective mass includes engine and wheel rotational inertia.
- Top speed: highest speed at which the best gear's wheel force (rpm <= rev limit) still
  exceeds drag + rolling resistance.
- 100-0 km/h: constant deceleration with ideal ABS (95 % of peak friction) on both axles,
  each axle limited by min(brake torque / radius, friction at its dynamic load).
- Skidpad: steady-state lateral limit; per-axle lateral load transfer split by roll-stiffness
  share (springs + anti-roll bars), roll-centre heights 0.08 m front / 0.12 m rear, each axle's
  capacity = sum over its two tires of mu(Fz)·Fz with load sensitivity, times 0.95.

Tire friction comes from `data/tires/<tire_id>.json`. When a car's *intended* compound file
does not exist yet the car JSON points at `street`; pass `compound_scale` to scale the street
peak friction to the intended compound (see `COMPOUND_SCALE`).
"""

from __future__ import annotations

import json
import math
import os

RHO_AIR = 1.225
G = 9.81
KMH = 1.0 / 3.6

# Peak-friction multiplier relative to the `street` compound, used only while the intended
# compound's tire file does not exist yet. Values are class-typical ratios of peak lateral g
# between tire categories on the same car (engineering rule of thumb, see tools/specsheet/README).
COMPOUND_SCALE = {
    "street": 1.00,
    "sport": 1.10,
    "semi_slick": 1.22,
    "wet_biased": 0.95,
    "police_pursuit": 1.04,
    "all_terrain": 0.90,
    "vintage_bias": 0.85,
}

# Drivetrain efficiency (crank to contact patch), class-typical.
DRIVETRAIN_EFF = {"Fwd": 0.88, "Rwd": 0.85, "Awd": 0.80}


def layout_name(layout) -> str:
    """'Fwd' | 'Rwd' | 'Awd' from the JSON layout value."""
    if isinstance(layout, str):
        return layout
    return next(iter(layout))


def front_split(layout) -> float:
    """Fraction of drive torque to the front axle."""
    name = layout_name(layout)
    if name == "Fwd":
        return 1.0
    if name == "Rwd":
        return 0.0
    return float(layout["Awd"]["front_split"])


def torque_at(curve, rpm: float) -> float:
    """Linear interpolation of the full-load torque curve, clamped at both ends."""
    if rpm <= curve[0][0]:
        return curve[0][1]
    for (r0, t0), (r1, t1) in zip(curve, curve[1:]):
        if rpm <= r1:
            return t0 + (t1 - t0) * (rpm - r0) / (r1 - r0)
    return curve[-1][1]


def peak_power(curve):
    """(kW, rpm) of the highest power point on the curve (sampled every 25 rpm)."""
    best = (0.0, 0.0)
    rpm = curve[0][0]
    while rpm <= curve[-1][0]:
        kw = torque_at(curve, rpm) * rpm * 2.0 * math.pi / 60.0 / 1000.0
        if kw > best[0]:
            best = (kw, rpm)
        rpm += 25.0
    return best


def peak_torque(curve):
    """(Nm, rpm) of the curve's highest torque point."""
    rpm, nm = max(curve, key=lambda p: p[1])
    return nm, rpm


class Tire:
    """Load-sensitive peak friction of one tire file."""

    def __init__(self, data: dict, scale: float = 1.0):
        self.d_lat = data["lateral"]["d"] * scale
        self.d_long = data["longitudinal"]["d"] * scale
        self.ls = data["load_sensitivity"]
        self.fz0 = data["fz0_n"]
        self.rr = data["rolling_resistance"]

    def mu(self, fz: float, lateral: bool) -> float:
        """Peak friction coefficient at vertical load `fz` (N)."""
        d = self.d_lat if lateral else self.d_long
        return max(0.0, d * (1.0 - self.ls * (fz / self.fz0 - 1.0)))


def load_tire(data_root: str, tire_id: str, scale: float = 1.0) -> Tire:
    """Load `data/tires/<tire_id>.json` with an optional friction scale."""
    with open(os.path.join(data_root, "tires", tire_id + ".json")) as f:
        return Tire(json.load(f), scale)


class Car:
    """Derived quantities of one VehicleDef JSON used by the estimators."""

    def __init__(self, car: dict, data_root: str, compound_scale: float = 1.0):
        self.j = car
        self.m = car["mass_kg"]
        self.wf = car["front_weight_frac"]
        self.L = car["wheelbase_m"]
        self.h = car["cg_height_m"]
        self.tf = car["track_front_m"]
        self.tr = car["track_rear_m"]
        self.cda = car["drag_cd_a_m2"]
        e = car["engine"]
        self.curve = [tuple(p) for p in e["torque_curve_nm"]]
        self.redline = e["redline_rpm"]
        self.limit = e["rev_limit_rpm"]
        self.idle = e["idle_rpm"]
        self.i_eng = e["inertia_kgm2"]
        d = car["drivetrain"]
        self.layout = layout_name(d["layout"])
        self.split = front_split(d["layout"])
        self.gears = d["gear_ratios"]
        self.fd = d["final_drive"]
        self.shift = d["shift_time_s"]
        self.eff = DRIVETRAIN_EFF[self.layout]
        w = car["wheels"]
        self.rf = w[0]["radius_m"]
        self.rr_ = w[2]["radius_m"]
        self.m_wheel = sum(x["mass_kg"] for x in w)
        self.tire_f = load_tire(data_root, w[0]["tire_id"], compound_scale)
        self.tire_r = load_tire(data_root, w[2]["tire_id"], compound_scale)
        s = car["suspension"]
        # Roll stiffness per axle: springs k·t²/2 + bar k_arb·t² (N·m/rad).
        self.kroll_f = s[0]["spring_n_per_m"] * self.tf**2 / 2 + s[0]["arb_n_per_m"] * self.tf**2
        self.kroll_r = s[2]["spring_n_per_m"] * self.tr**2 / 2 + s[2]["arb_n_per_m"] * self.tr**2
        b = car["brakes"]
        self.tb_f = b["max_torque_front_nm"]
        self.tb_r = b["max_torque_rear_nm"]

    # Driven-wheel radius (rear for RWD; front for FWD; mean for AWD).
    def drive_radius(self) -> float:
        if self.layout == "Fwd":
            return self.rf
        if self.layout == "Rwd":
            return self.rr_
        return 0.5 * (self.rf + self.rr_)

    def resist(self, v: float) -> float:
        """Aero drag + rolling resistance at speed v (N)."""
        return 0.5 * RHO_AIR * self.cda * v * v + self.tire_f.rr * self.m * G

    def rpm_for(self, v: float, gear: int) -> float:
        r = self.drive_radius()
        return v / r * self.gears[gear] * self.fd * 60.0 / (2.0 * math.pi)

    def drive_force(self, v: float, gear: int, launch_rpm: float = 0.0) -> float:
        """Wheel force in `gear` at speed v, 0 if over the rev limit."""
        rpm = self.rpm_for(v, gear)
        if rpm > self.limit:
            return 0.0
        rpm = max(rpm, self.idle, launch_rpm if gear == 0 else 0.0)
        return torque_at(self.curve, rpm) * self.gears[gear] * self.fd * self.eff / self.drive_radius()

    def traction_force(self, a: float) -> float:
        """Max longitudinal drive force at current acceleration `a` (load transfer)."""
        wf = self.m * G * self.wf - self.m * a * self.h / self.L
        wr = self.m * G * (1 - self.wf) + self.m * a * self.h / self.L
        k = 1.0  # test-driver launch held at peak slip (as in published 0-100 figures)
        ff = k * self.tire_f.mu(wf / 2, False) * wf
        fr = k * self.tire_r.mu(wr / 2, False) * wr
        if self.layout == "Fwd":
            return ff
        if self.layout == "Rwd":
            return fr
        return min(ff / self.split, fr / (1.0 - self.split))

    def accel_time(self, v_target: float = 100 * KMH) -> float:
        """Estimated 0 → v_target time, s."""
        dt = 0.005
        launch = min(max(0.5 * self.redline, 2500.0), 4500.0)
        v = t = a = 0.0
        gear = 0
        shifting = 0.0
        r = self.drive_radius()
        while v < v_target and t < 60.0:
            best = max(range(len(self.gears)), key=lambda g: self.drive_force(v, g, launch))
            if best > gear:
                gear = best
                shifting = self.shift
            if shifting > 0.0:
                f = 0.0
                shifting -= dt
            else:
                f = min(self.drive_force(v, gear, launch), self.traction_force(a))
            ratio = self.gears[gear] * self.fd
            # Engine inertia only loads the car once the clutch is locked (not while it slips
            # at the launch rpm in 1st).
            locked = gear > 0 or self.rpm_for(v, 0) >= launch
            m_eff = self.m + (self.i_eng * (ratio / r) ** 2 if locked else 0.0) + 0.7 * self.m_wheel
            a = max(0.0, (f - self.resist(v)) / m_eff)
            v += a * dt
            t += dt
        return t

    def top_speed(self) -> float:
        """Estimated top speed, m/s."""
        v = 1.0
        top = 0.0
        while v < 120.0:
            f = max(self.drive_force(v, g) for g in range(len(self.gears)))
            if f >= self.resist(v):
                top = v
            elif v > 10.0:
                break
            v += 0.1
        return top

    def brake_decel(self) -> float:
        """Estimated steady ABS deceleration from 100 km/h, m/s²."""
        a = G
        for _ in range(30):
            wf = self.m * G * self.wf + self.m * a * self.h / self.L
            wr = self.m * G * (1 - self.wf) - self.m * a * self.h / self.L
            ff = min(2 * self.tb_f / self.rf, 0.95 * self.tire_f.mu(wf / 2, False) * wf)
            fr = min(2 * self.tb_r / self.rr_, 0.95 * self.tire_r.mu(max(wr, 1.0) / 2, False) * wr)
            a = 0.5 * a + 0.5 * (ff + fr) / self.m
        return a

    def brake_distance(self, v0: float = 100 * KMH) -> float:
        """Estimated 100-0 km/h distance, m."""
        return v0 * v0 / (2.0 * self.brake_decel())

    def skidpad_g(self) -> float:
        """Estimated steady-state lateral limit, g."""
        hrc_f, hrc_r = 0.08, 0.12
        share_f = self.kroll_f / (self.kroll_f + self.kroll_r)
        wf = self.m * G * self.wf
        wr = self.m * G * (1 - self.wf)
        h_arm = self.h - (hrc_f * self.wf + hrc_r * (1 - self.wf))

        def ok(ay_g: float) -> bool:
            fy = self.m * ay_g * G
            for w, t, share, hrc, tire in (
                (wf, self.tf, share_f, hrc_f, self.tire_f),
                (wr, self.tr, 1 - share_f, hrc_r, self.tire_r),
            ):
                dfz = fy * (share * h_arm + (w / (self.m * G)) * hrc) / t
                outer = w / 2 + dfz
                inner = max(0.0, w / 2 - dfz)
                cap = 0.95 * (tire.mu(outer, True) * outer + tire.mu(inner, True) * inner)
                if cap < fy * w / (self.m * G):
                    return False
            return True

        lo, hi = 0.0, 2.0
        for _ in range(40):
            mid = 0.5 * (lo + hi)
            if ok(mid):
                lo = mid
            else:
                hi = mid
        return lo

    def power_to_weight(self) -> float:
        """Peak power per tonne, kW/t."""
        return peak_power(self.curve)[0] / (self.m / 1000.0)


def class_index(t0100: float, lat_g: float, vmax_kmh: float) -> int:
    """Rough performance index (100-999) for design-doc bracketing only.

    `600 · (0.5·(6 s / t0100) + 0.3·lat_g + 0.2·vmax/300 km/h)`. The authoritative
    `class_index` is workstream 08's; this exists so the roster table has a comparable column.
    """
    x = 0.5 * (6.0 / t0100) + 0.3 * lat_g + 0.2 * vmax_kmh / 300.0
    return max(100, min(999, round(600 * x)))


def estimate(car: dict, data_root: str, compound_scale: float = 1.0) -> dict:
    """All estimates for one car JSON dict."""
    c = Car(car, data_root, compound_scale)
    kw, kw_rpm = peak_power(c.curve)
    nm, nm_rpm = peak_torque(c.curve)
    t = c.accel_time()
    vmax = c.top_speed() * 3.6
    lat = c.skidpad_g()
    return {
        "peak_kw": kw,
        "peak_kw_rpm": kw_rpm,
        "peak_nm": nm,
        "peak_nm_rpm": nm_rpm,
        "kw_per_t": c.power_to_weight(),
        "t0100_s": t,
        "vmax_kmh": vmax,
        "brake_100_0_m": c.brake_distance(),
        "brake_decel_g": c.brake_decel() / G,
        "skidpad_g": lat,
        "class_index": class_index(t, lat, vmax),
    }
