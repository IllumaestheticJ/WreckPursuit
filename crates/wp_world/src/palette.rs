//! Render palette of feature [`Material`]s: one greybox colour and a short snake-case name per
//! material, shared by the greybox writer (`tools/city-import`, prism features), the wasm
//! export (`wp_wasm::feature_palette`) and so the testbed's instanced boxes and cylinders.
//! Defined once here so every renderer agrees.
//!
//! Material and kind indices in packed arrays are the enum discriminants (`m as u8`, in
//! declaration order); [`MATERIALS`] lists them in that order.

use crate::network::Material;

/// Every material in declaration (index) order.
pub const MATERIALS: [Material; 24] = [
    Material::Concrete,
    Material::DarkConcrete,
    Material::Asphalt,
    Material::Steel,
    Material::Rust,
    Material::Wood,
    Material::Brick,
    Material::Glass,
    Material::Grass,
    Material::Foliage,
    Material::Dirt,
    Material::Gravel,
    Material::Sand,
    Material::PaintWhite,
    Material::PaintYellow,
    Material::SafetyOrange,
    Material::SafetyYellow,
    Material::Red,
    Material::Blue,
    Material::Green,
    Material::White,
    Material::Black,
    Material::CarPaint,
    Material::Light,
];

/// Linear-ish greybox base colour RGBA of a material (glTF `baseColorFactor`). `CarPaint` is
/// a neutral base the testbed tints per instance; `Light` is drawn emissive.
pub fn material_rgba(m: Material) -> [f32; 4] {
    match m {
        Material::Concrete => [0.68, 0.67, 0.64, 1.0],
        Material::DarkConcrete => [0.42, 0.42, 0.41, 1.0],
        Material::Asphalt => [0.20, 0.20, 0.21, 1.0],
        Material::Steel => [0.55, 0.57, 0.60, 1.0],
        Material::Rust => [0.50, 0.30, 0.20, 1.0],
        Material::Wood => [0.50, 0.36, 0.22, 1.0],
        Material::Brick => [0.58, 0.30, 0.24, 1.0],
        Material::Glass => [0.45, 0.55, 0.62, 1.0],
        Material::Grass => [0.33, 0.50, 0.27, 1.0],
        Material::Foliage => [0.22, 0.40, 0.20, 1.0],
        Material::Dirt => [0.45, 0.35, 0.24, 1.0],
        Material::Gravel => [0.52, 0.50, 0.46, 1.0],
        Material::Sand => [0.80, 0.72, 0.52, 1.0],
        Material::PaintWhite => [0.92, 0.92, 0.90, 1.0],
        Material::PaintYellow => [0.90, 0.72, 0.14, 1.0],
        Material::SafetyOrange => [0.95, 0.45, 0.10, 1.0],
        Material::SafetyYellow => [0.95, 0.78, 0.10, 1.0],
        Material::Red => [0.70, 0.16, 0.13, 1.0],
        Material::Blue => [0.16, 0.30, 0.62, 1.0],
        Material::Green => [0.18, 0.50, 0.25, 1.0],
        Material::White => [0.90, 0.90, 0.88, 1.0],
        Material::Black => [0.08, 0.08, 0.09, 1.0],
        Material::CarPaint => [0.60, 0.60, 0.62, 1.0],
        Material::Light => [1.00, 0.92, 0.70, 1.0],
    }
}

/// Short snake-case name of a material (greybox primitive `feature_<name>`).
pub fn material_name(m: Material) -> &'static str {
    match m {
        Material::Concrete => "concrete",
        Material::DarkConcrete => "dark_concrete",
        Material::Asphalt => "asphalt",
        Material::Steel => "steel",
        Material::Rust => "rust",
        Material::Wood => "wood",
        Material::Brick => "brick",
        Material::Glass => "glass",
        Material::Grass => "grass",
        Material::Foliage => "foliage",
        Material::Dirt => "dirt",
        Material::Gravel => "gravel",
        Material::Sand => "sand",
        Material::PaintWhite => "paint_white",
        Material::PaintYellow => "paint_yellow",
        Material::SafetyOrange => "safety_orange",
        Material::SafetyYellow => "safety_yellow",
        Material::Red => "red",
        Material::Blue => "blue",
        Material::Green => "green",
        Material::White => "white",
        Material::Black => "black",
        Material::CarPaint => "car_paint",
        Material::Light => "light",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materials_are_in_index_order_and_distinct() {
        for (i, m) in MATERIALS.iter().enumerate() {
            assert_eq!(*m as usize, i, "{m:?}");
        }
        for (i, a) in MATERIALS.iter().enumerate() {
            for b in &MATERIALS[i + 1..] {
                assert_ne!(material_rgba(*a), material_rgba(*b), "{a:?} vs {b:?}");
                assert_ne!(material_name(*a), material_name(*b));
            }
        }
    }
}
