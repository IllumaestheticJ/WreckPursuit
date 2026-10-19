//! Collision audio: which material was hit, which impact layers that produces, and per-pair
//! bookkeeping (impact debounce, sustained-contact scrapes).
//!
//! `SimEvent::Contact` carries no material, so [`MaterialMap`] recovers it from the car's
//! damage lattice: the lattice node nearest the contact point (in chassis space) and the
//! dominant material kind of the beams at that node. Contract request A-1 asks for
//! `material_kind` / `region` on the contact event so this lookup can go away.

use std::collections::BTreeMap;

use wp_contracts::{BodyRegion, LatticeDef, MaterialDef, MaterialKind};

use crate::data::{cues, ImpactMix};

/// Material (and region) of one lattice node.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NodeMat {
    pos: [f32; 3],
    kind: MaterialKind,
    region: BodyRegion,
}

/// Nearest-node material lookup for one car.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MaterialMap {
    nodes: Vec<NodeMat>,
}

/// Rank used to break ties between beam kinds at a node (louder / more distinctive wins).
fn kind_rank(k: MaterialKind) -> u8 {
    match k {
        MaterialKind::Glass => 4,
        MaterialKind::Plastic => 3,
        MaterialKind::Cast => 2,
        MaterialKind::Panel => 1,
        MaterialKind::Structural => 0,
    }
}

impl MaterialMap {
    /// Build from a lattice and its materials. Each node takes the most common material kind
    /// among its beams; glass-region nodes with any glass beam are glass.
    pub fn from_lattice(lattice: &LatticeDef, materials: &BTreeMap<String, MaterialDef>) -> Self {
        let mut counts: Vec<BTreeMap<MaterialKind, u32>> =
            vec![BTreeMap::new(); lattice.nodes.len()];
        for b in &lattice.beams {
            let kind = materials
                .get(&b.material_id)
                .map(|m| m.kind)
                .unwrap_or(MaterialKind::Panel);
            for n in [b.a, b.b] {
                if let Some(c) = counts.get_mut(n as usize) {
                    *c.entry(kind).or_default() += 1;
                }
            }
        }
        let nodes = lattice
            .nodes
            .iter()
            .zip(counts)
            .map(|(n, c)| {
                let glass_region =
                    matches!(n.region, BodyRegion::Windshield | BodyRegion::RearGlass);
                let kind = if glass_region && c.contains_key(&MaterialKind::Glass) {
                    MaterialKind::Glass
                } else {
                    c.iter()
                        .max_by(|a, b| a.1.cmp(b.1).then(kind_rank(*a.0).cmp(&kind_rank(*b.0))))
                        .map(|(k, _)| *k)
                        .unwrap_or(MaterialKind::Panel)
                };
                NodeMat {
                    pos: n.pos_m,
                    kind,
                    region: n.region,
                }
            })
            .collect();
        Self { nodes }
    }

    /// Mean rest position (chassis space) of the nodes of `region`, if it has any.
    pub fn region_centroid(&self, region: BodyRegion) -> Option<[f32; 3]> {
        let mut sum = [0.0f32; 3];
        let mut n = 0u32;
        for node in self.nodes.iter().filter(|x| x.region == region) {
            for (acc, v) in sum.iter_mut().zip(node.pos) {
                *acc += v;
            }
            n += 1;
        }
        (n > 0).then(|| [sum[0] / n as f32, sum[1] / n as f32, sum[2] / n as f32])
    }

    /// Material kind and region nearest to chassis-space point `p` (ties: lowest node id).
    /// `None` for an empty lattice.
    pub fn at(&self, p: [f32; 3]) -> Option<(MaterialKind, BodyRegion)> {
        let mut best: Option<(f32, &NodeMat)> = None;
        for n in &self.nodes {
            let d = [n.pos[0] - p[0], n.pos[1] - p[1], n.pos[2] - p[2]];
            let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if best.is_none_or(|(b, _)| d2 < b) {
                best = Some((d2, n));
            }
        }
        best.map(|(_, n)| (n.kind, n.region))
    }
}

/// One impact sound layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImpactLayer {
    /// Soft low-speed thud.
    Thud,
    /// Plastic crack.
    PlasticCrack,
    /// Steel panel dent.
    PanelDent,
    /// Structural crunch.
    StructuralCrunch,
    /// Glass shatter.
    GlassShatter,
    /// Cast-metal clank.
    CastClank,
}

impl ImpactLayer {
    /// Cue name in a world pack.
    pub fn cue(self) -> &'static str {
        match self {
            ImpactLayer::Thud => cues::THUD,
            ImpactLayer::PlasticCrack => cues::PLASTIC_CRACK,
            ImpactLayer::PanelDent => cues::PANEL_DENT,
            ImpactLayer::StructuralCrunch => cues::STRUCTURAL_CRUNCH,
            ImpactLayer::GlassShatter => cues::GLASS_SHATTER,
            ImpactLayer::CastClank => cues::CAST_CLANK,
        }
    }
}

/// Layers for an impact on `kind` at closing speed `speed` (m/s). `glass_intact` = the glass
/// region has not shattered yet. Empty below `min_speed_mps`.
pub fn impact_layers(
    mix: &ImpactMix,
    kind: MaterialKind,
    speed: f32,
    glass_intact: bool,
) -> Vec<ImpactLayer> {
    if speed < mix.min_speed_mps {
        return Vec::new();
    }
    if speed < mix.thud_max_speed_mps {
        return vec![ImpactLayer::Thud];
    }
    let crunch = speed >= mix.panel_crunch_speed_mps;
    match kind {
        MaterialKind::Plastic => {
            if crunch {
                vec![ImpactLayer::PlasticCrack, ImpactLayer::StructuralCrunch]
            } else {
                vec![ImpactLayer::PlasticCrack]
            }
        }
        MaterialKind::Panel => {
            if crunch {
                vec![ImpactLayer::PanelDent, ImpactLayer::StructuralCrunch]
            } else {
                vec![ImpactLayer::PanelDent]
            }
        }
        MaterialKind::Structural => {
            if speed >= mix.structural_crunch_speed_mps {
                vec![ImpactLayer::StructuralCrunch]
            } else {
                vec![ImpactLayer::PanelDent]
            }
        }
        MaterialKind::Glass => {
            if glass_intact && speed >= mix.glass_break_speed_mps {
                vec![ImpactLayer::GlassShatter, ImpactLayer::PanelDent]
            } else {
                vec![ImpactLayer::PanelDent]
            }
        }
        MaterialKind::Cast => vec![ImpactLayer::CastClank],
    }
}

/// Impact loudness from normal impulse and closing speed, 0..≈1.5 × `impact_gain`.
pub fn impact_gain(mix: &ImpactMix, impulse_ns: f32, speed: f32) -> f32 {
    let by_impulse = (impulse_ns.max(0.0) / mix.impulse_ref_ns.max(1.0)).sqrt();
    let by_speed = (speed / (mix.structural_crunch_speed_mps * 2.0).max(1.0)).min(1.0);
    mix.impact_gain * (0.5 * by_impulse + 0.5 * by_speed).min(1.5)
}

/// State of one contact pair `(lo, hi)` entity ids.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PairState {
    /// Tick of the last impact one-shot.
    pub last_impact_tick: Option<u64>,
    /// Closing speed of that impact.
    pub last_impact_speed: f32,
    /// Tick of the last contact event.
    pub last_contact_tick: u64,
    /// Scrape level 0..=1 at the last frame with contact (fades after contact ends).
    pub scrape: f32,
    /// Highest scrape target seen in the current frame's contacts.
    pub frame_scrape: f32,
    /// World point of the latest contact.
    pub point: [f32; 3],
    /// Scrape is metal (else plastic).
    pub metal: bool,
}

impl PairState {
    /// Whether a new impact at `speed` on `tick` should play (debounce).
    pub fn should_impact(
        &self,
        mix: &ImpactMix,
        speed: f32,
        tick: u64,
        debounce_ticks: u64,
    ) -> bool {
        match self.last_impact_tick {
            None => true,
            Some(t) => {
                tick >= t + debounce_ticks || speed > self.last_impact_speed * mix.louder_factor
            }
        }
    }
}

/// Ordered pair key.
pub fn pair_key(a: u32, b: u32) -> (u32, u32) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mix() -> ImpactMix {
        ImpactMix {
            min_speed_mps: 0.6,
            thud_max_speed_mps: 1.8,
            panel_crunch_speed_mps: 9.0,
            structural_crunch_speed_mps: 4.0,
            glass_break_speed_mps: 6.0,
            impulse_ref_ns: 3000.0,
            debounce_s: 0.12,
            louder_factor: 1.6,
            scrape_min_speed_mps: 0.8,
            scrape_speed_ref_mps: 12.0,
            scrape_release_s: 0.15,
            impact_gain: 1.0,
            scrape_gain: 0.7,
        }
    }

    #[test]
    fn layers_by_material_and_speed() {
        let m = mix();
        assert!(impact_layers(&m, MaterialKind::Panel, 0.3, true).is_empty());
        assert_eq!(
            impact_layers(&m, MaterialKind::Glass, 1.0, true),
            vec![ImpactLayer::Thud]
        );
        assert_eq!(
            impact_layers(&m, MaterialKind::Plastic, 3.0, true),
            vec![ImpactLayer::PlasticCrack]
        );
        assert_eq!(
            impact_layers(&m, MaterialKind::Panel, 12.0, true),
            vec![ImpactLayer::PanelDent, ImpactLayer::StructuralCrunch]
        );
        assert_eq!(
            impact_layers(&m, MaterialKind::Structural, 5.0, true),
            vec![ImpactLayer::StructuralCrunch]
        );
        assert!(
            impact_layers(&m, MaterialKind::Glass, 8.0, true).contains(&ImpactLayer::GlassShatter)
        );
        assert!(!impact_layers(&m, MaterialKind::Glass, 8.0, false)
            .contains(&ImpactLayer::GlassShatter));
        assert_eq!(
            impact_layers(&m, MaterialKind::Cast, 5.0, true),
            vec![ImpactLayer::CastClank]
        );
    }

    #[test]
    fn gain_grows_with_impulse() {
        let m = mix();
        assert!(impact_gain(&m, 6000.0, 10.0) > impact_gain(&m, 500.0, 10.0));
        assert!(impact_gain(&m, 1e9, 100.0) <= 1.5);
    }

    #[test]
    fn debounce_allows_louder_hits() {
        let m = mix();
        let st = PairState {
            last_impact_tick: Some(100),
            last_impact_speed: 5.0,
            ..Default::default()
        };
        assert!(!st.should_impact(&m, 5.0, 110, 29));
        assert!(st.should_impact(&m, 9.0, 110, 29));
        assert!(st.should_impact(&m, 5.0, 129, 29));
    }

    #[test]
    fn material_map_nearest_node() {
        use wp_contracts::{LatticeBeam, LatticeNode};
        let node = |id, pos_m, region| LatticeNode {
            id,
            pos_m,
            mass_kg: 1.0,
            pinned_to_chassis: false,
            region,
        };
        let beam = |a, b, m: &str| LatticeBeam {
            a,
            b,
            material_id: m.into(),
            area_m2: 1e-4,
            breakable: true,
            detach_group: None,
        };
        let lattice = LatticeDef {
            id: "t".into(),
            nodes: vec![
                node(0, [0.0, 2.0, 0.5], BodyRegion::FrontBumper),
                node(1, [0.0, 1.0, 0.5], BodyRegion::FrameFront),
                node(2, [0.0, 0.8, 1.1], BodyRegion::Windshield),
            ],
            beams: vec![
                beam(0, 1, "pp"),
                beam(1, 2, "steel"),
                beam(2, 1, "glass"),
                beam(0, 1, "pp"),
            ],
            detachables: vec![],
            components: vec![],
            skin_binding: vec![],
        };
        let mat = |id: &str, kind| MaterialDef {
            id: id.into(),
            density_kg_m3: 1.0,
            young_mpa: 1.0,
            yield_mpa: 1.0,
            plastic_modulus_mpa: 1.0,
            failure_strain: 0.1,
            restitution: 0.1,
            friction: 0.5,
            kind,
        };
        let mut materials = BTreeMap::new();
        materials.insert("pp".to_string(), mat("pp", MaterialKind::Plastic));
        materials.insert("steel".to_string(), mat("steel", MaterialKind::Structural));
        materials.insert("glass".to_string(), mat("glass", MaterialKind::Glass));
        let m = MaterialMap::from_lattice(&lattice, &materials);
        assert_eq!(m.at([0.0, 2.3, 0.5]).unwrap().0, MaterialKind::Plastic);
        assert_eq!(
            m.at([0.0, 0.7, 1.2]).unwrap(),
            (MaterialKind::Glass, BodyRegion::Windshield)
        );
    }
}
