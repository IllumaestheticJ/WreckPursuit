//! # wp_render
//!
//! Our own renderer, the first piece of a custom engine (see `docs/ENGINE_DECISION.md`). It
//! draws what the browser testbed draws: the city greybox ([`city`]), cars from the per-panel meshes in
//! `data/panels/meshes`, skinned to the damage lattice so every dent shows, with spinning and
//! steering wheels, static boxes and a ground grid, seen from a chase camera.
//!
//! Layers:
//! - CPU, always built: [`car_mesh`] (mesh file reader), [`skin`] (lattice skinning),
//!   [`scene`] (renderer-side copy of a `wp_sim::Sim`), [`camera`]. Unit-testable, no GPU.
//! - GPU, feature `gpu`: [`gpu::Renderer`] (wgpu; Vulkan/Metal/DX12 now, WebGPU later) and
//!   [`gpu::Offscreen`] for headless screenshots.
//!
//! Rendering only reads the sim; nothing here feeds back into simulation state, so wall-clock
//! frame time is fine on this side of the boundary.

#![warn(missing_docs)]

pub mod camera;
pub mod car_mesh;
pub mod city;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod scene;
pub mod skin;

pub use camera::{CameraView, ChaseCamera};
pub use car_mesh::{CarMesh, CarMeshLibrary};
pub use city::CityMesh;
pub use scene::{CarView, Scene, StaticBox};
pub use skin::{SkinnedBody, StaticMesh};

/// How a vertex is shaded (stored as a float in [`Vertex::shade`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Shade {
    /// Sun + sky lighting with flat (per-triangle) normals.
    Lit = 0,
    /// Unlit, full brightness (lamps).
    Emissive = 1,
    /// Ground plane with a procedural metre grid.
    Ground = 2,
}

/// One vertex as uploaded to the GPU (44 bytes). With a zero [`Vertex::normal`] the normal is
/// derived per triangle in the fragment shader, which gives the low-poly flat look and stays
/// right while a body deforms; smooth-shaded geometry (city terrain) carries its own.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "gpu", derive(bytemuck::Pod, bytemuck::Zeroable))]
#[repr(C)]
pub struct Vertex {
    /// Position (chassis, hub or world space depending on the mesh), m.
    pub pos: [f32; 3],
    /// Linear RGBA; alpha < 1 only on glass.
    pub color: [f32; 4],
    /// [`Shade`] as a float.
    pub shade: f32,
    /// Smooth-shading normal, or zero for a flat (per-triangle) normal.
    pub normal: [f32; 3],
}
