//! Static city features (road format v4) for the testbed's instanced renderer: every box and
//! cylinder feature packed into one float array ([`pack`]), plus the shared material palette
//! ([`palette`], `wp_world::palette`, the same colours the greybox gives prism features).
//! Prisms are not packed: they are in the greybox.

use wp_world::network::{FeatureShape, RoadNetwork};

/// Floats per feature in [`pack`].
pub const FEATURE_STRIDE: usize = 11;

/// Shape codes (offset 0 of a packed feature).
pub mod shape {
    /// Oriented box.
    pub const BOX: f32 = 0.0;
    /// Upright cylinder.
    pub const CYLINDER: f32 = 1.0;
}

/// Box and cylinder features in network order, [`FEATURE_STRIDE`] floats each:
/// `[shape, material, kind, cx, cy, cz, hx, hy, hz, yaw, pitch]`, sim space (+Z up).
/// `material` / `kind` are the `Material` / `FeatureKind` discriminants. A box is its centre,
/// half extents (local x right, y forward, z up), yaw about +Z (0 = local +Y north, CCW) and
/// pitch about local +X applied first (positive = the +Y end up). A cylinder is centre =
/// base + height / 2, `hx = hy = radius`, `hz = height / 2`, yaw = pitch = 0.
pub fn pack(net: &RoadNetwork) -> Vec<f32> {
    let mut out = Vec::with_capacity(net.features.len() * FEATURE_STRIDE);
    for f in &net.features {
        let (m, k) = (f.material as u8 as f32, f.kind as u8 as f32);
        match &f.shape {
            FeatureShape::Box {
                centre,
                half,
                yaw,
                pitch,
            } => out.extend_from_slice(&[
                shape::BOX,
                m,
                k,
                centre[0],
                centre[1],
                centre[2],
                half[0],
                half[1],
                half[2],
                *yaw,
                *pitch,
            ]),
            FeatureShape::Cylinder {
                base,
                radius,
                height,
            } => out.extend_from_slice(&[
                shape::CYLINDER,
                m,
                k,
                base[0],
                base[1],
                base[2] + height * 0.5,
                *radius,
                *radius,
                height * 0.5,
                0.0,
                0.0,
            ]),
            FeatureShape::Prism { .. } => {}
        }
    }
    out
}

/// Material colours, 4 floats (linear RGBA) per material in index order
/// (`wp_world::palette::material_rgba`).
pub fn palette() -> Vec<f32> {
    wp_world::palette::MATERIALS
        .iter()
        .flat_map(|m| wp_world::palette::material_rgba(*m))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_world::network::{Feature, FeatureKind, Material};

    #[test]
    fn packs_boxes_and_cylinders_and_skips_prisms() {
        let mut net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig {
            nx: 4,
            ny: 4,
            ..Default::default()
        });
        net.features = vec![
            Feature {
                kind: FeatureKind::ParkingRamp,
                material: Material::Concrete,
                shape: FeatureShape::Box {
                    centre: [1.0, 2.0, 3.0],
                    half: [4.0, 5.0, 0.2],
                    yaw: 0.5,
                    pitch: 0.1,
                },
                solid: true,
            },
            Feature {
                kind: FeatureKind::Hangar,
                material: Material::Steel,
                shape: FeatureShape::Prism {
                    footprint: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                    base_z: 0.0,
                    top_z: 1.0,
                },
                solid: true,
            },
            Feature {
                kind: FeatureKind::Lamp,
                material: Material::Light,
                shape: FeatureShape::Cylinder {
                    base: [10.0, 20.0, 1.0],
                    radius: 0.2,
                    height: 6.0,
                },
                solid: false,
            },
        ];
        let p = pack(&net);
        assert_eq!(p.len(), 2 * FEATURE_STRIDE);
        assert_eq!(
            &p[..FEATURE_STRIDE],
            &[
                0.0,
                Material::Concrete as u8 as f32,
                FeatureKind::ParkingRamp as u8 as f32,
                1.0,
                2.0,
                3.0,
                4.0,
                5.0,
                0.2,
                0.5,
                0.1
            ]
        );
        assert_eq!(
            &p[FEATURE_STRIDE..],
            &[
                1.0,
                Material::Light as u8 as f32,
                FeatureKind::Lamp as u8 as f32,
                10.0,
                20.0,
                4.0,
                0.2,
                0.2,
                3.0,
                0.0,
                0.0
            ]
        );
        // Indices the testbed hard-codes (apps/testbed-web/src/wasm/layout.ts).
        assert_eq!(Material::CarPaint as u8, 22);
        assert_eq!(Material::Light as u8, 23);
        assert_eq!(palette().len(), 4 * 24);
    }
}
