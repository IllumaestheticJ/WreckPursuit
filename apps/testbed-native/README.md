# testbed-native

**Owned by workstream 04.** Native stub so wheel feel can be judged: steps the same core as the
browser (`wp_wasm::session::Session` → `wp_sim::Sim`) and sends `FfbOutput::rack_torque_nm` to a
wheel. Deliberately minimal: "window + car box + FFB out".

```sh
cargo run -p testbed-native                                   # headless smoke run (what CI builds)
cargo run --release -p testbed-native --features window       # window: arrows/WASD, Space, R, Esc
cargo run --release -p testbed-native --features window,ffb   # + SDL2 constant-force wheel
cargo run -p testbed-native --features ffb -- --headless      # FFB without a window
```

- `window`: winit 0.30 + softbuffer (CPU framebuffer, no GPU), top-down view following the car,
  10 m grid, steered wheels, rack-torque bar (magenta) and speed bar.
- `gpu`: 3D view through our own renderer (`crates/wp_render`): `-- --gpu` opens a wgpu window
  with the real car meshes deforming, `-- --shot out.png [--scenario x.ron]` writes a headless
  screenshot. See `crates/wp_render/README.md`.
- `ffb`: raw `sdl2-sys` haptic API (the safe wrapper only does rumble): haptic device 0, one
  infinite constant-force effect on the steering axis, level = torque / 20 N·m; joystick axis 0
  is read as steering. Needs `libSDL2` at link time.

Runtime on Linux/X11 needs `libxkbcommon-x11`. Tests: none beyond `cargo clippy` (it is a stub).

## Known gaps
FFB not validated against a physical wheel yet; FFB is 0 until `wp_vehicle` (02) implements it.
