# wp_vehicle

**Owned by workstream 02 (vehicle dynamics).**

Vehicle dynamics. **Currently a kinematic bicycle-model stub** implementing `wp_contracts::VehicleSim` so the sim loop and testbed drive. Prompt 02 replaces the internals (Rapier rigid body, raycast wheels, Magic Formula tires, drivetrain, assists, FFB) behind the same surface: `Vehicle::new(&VehicleDef, &TireDb, pos, yaw)`, `Vehicle::reset`, the `VehicleSim` impl. `AssistProfile` is re-exported from `wp_contracts`.

Note: rapier3d 0.36 uses glam-based math (`rapier3d::math::Vector` = `Vec3`), not nalgebra.

## Known gaps
- Everything physical. The stub ignores tires, suspension, weight transfer and `WorldQuery`.

## Perf
Stub: negligible. Budget: ≤ 0.15 ms/car/frame native.

## Tests

```sh
cargo test -p wp_vehicle
```
