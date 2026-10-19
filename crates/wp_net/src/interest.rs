//! Interest management: which vehicles a client hears about, and how often.
//!
//! Priority (prompt 07): own car (always, via `OwnState`) > racers in the same race >
//! pursuers/pursued near the client > anything within 120 m (traffic included) > everything
//! else at minimap rate (players and police only; distant traffic is not replicated).
//!
//! Each net tick every candidate's accumulator grows by its category weight; eligible
//! candidates (respecting a per-category maximum rate) are sent in descending accumulator
//! order until the client's byte budget for the tick is spent, and sent entities reset to 0.
//! Starved low-priority entities therefore still trickle through.

use std::collections::BTreeMap;

use wp_contracts::EntityId;

use crate::protocol::EntityKind;

/// Radius within which everything (including traffic) is replicated, m.
pub const NEAR_RADIUS_M: f32 = 120.0;
/// Radius within which police↔racer pairs count as a pursuit, m.
pub const PURSUIT_RADIUS_M: f32 = 300.0;

/// Interest category of one entity for one viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// Racer in the viewer's race.
    SameRace,
    /// Police near a racer viewer, or a racer near a police viewer.
    Pursuit,
    /// Within [`NEAR_RADIUS_M`].
    Near,
    /// Everything else worth a minimap dot.
    Far,
}

impl Category {
    /// Accumulator weight per net tick.
    pub fn weight(self) -> f32 {
        match self {
            Self::SameRace => 1.0,
            Self::Pursuit => 0.8,
            Self::Near => 0.5,
            Self::Far => 0.05,
        }
    }

    /// Minimum net ticks between two sends (60 / 60 / 30 / 5 Hz caps).
    pub fn min_interval(self) -> u64 {
        match self {
            Self::SameRace | Self::Pursuit => 1,
            Self::Near => 2,
            Self::Far => 12,
        }
    }
}

/// What interest classification needs to know about the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewer {
    /// Viewer's own vehicle, if any.
    pub vehicle: Option<EntityId>,
    /// Viewer position (own vehicle, else origin), m.
    pub pos: [f32; 3],
    /// True if the viewer drives a police unit.
    pub police: bool,
    /// True if the viewer drives a racer.
    pub racer: bool,
    /// Race the viewer is in.
    pub race: Option<u32>,
}

/// What interest classification needs to know about a candidate entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subject {
    /// Entity.
    pub entity: EntityId,
    /// Kind.
    pub kind: EntityKind,
    /// Position, m.
    pub pos: [f32; 3],
    /// Race its driver is in.
    pub race: Option<u32>,
}

/// Category of `s` for viewer `v`, or `None` if the viewer should not hear about it at all.
pub fn classify(v: &Viewer, s: &Subject) -> Option<Category> {
    if v.vehicle == Some(s.entity) {
        return None; // own car travels as OwnState
    }
    let d2 = (0..3).map(|i| (s.pos[i] - v.pos[i]).powi(2)).sum::<f32>();
    if v.race.is_some() && v.race == s.race {
        return Some(Category::SameRace);
    }
    let pursuit_pair = (v.racer && s.kind.is_police()) || (v.police && s.kind == EntityKind::Racer);
    if pursuit_pair && d2 <= PURSUIT_RADIUS_M * PURSUIT_RADIUS_M {
        return Some(Category::Pursuit);
    }
    if d2 <= NEAR_RADIUS_M * NEAR_RADIUS_M {
        return Some(Category::Near);
    }
    match s.kind {
        EntityKind::Traffic => None,
        _ => Some(Category::Far),
    }
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    category: Category,
    accum: f32,
    last_sent: Option<u64>,
}

/// Per-client priority accumulators.
#[derive(Debug, Clone, Default)]
pub struct InterestState {
    entries: BTreeMap<EntityId, Entry>,
}

impl InterestState {
    /// Empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Entities currently in the interest set.
    pub fn members(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entries.keys().copied()
    }

    /// True if `e` is in the interest set.
    pub fn contains(&self, e: EntityId) -> bool {
        self.entries.contains_key(&e)
    }

    /// Category of a member.
    pub fn category(&self, e: EntityId) -> Option<Category> {
        self.entries.get(&e).map(|x| x.category)
    }

    /// Replace the interest set with `candidates` for this net tick. Returns
    /// `(entered, left)` entity lists (each ascending).
    pub fn update(
        &mut self,
        candidates: &[(EntityId, Category)],
    ) -> (Vec<EntityId>, Vec<EntityId>) {
        let mut entered = Vec::new();
        let new: BTreeMap<EntityId, Category> = candidates.iter().copied().collect();
        let left: Vec<EntityId> = self
            .entries
            .keys()
            .filter(|e| !new.contains_key(e))
            .copied()
            .collect();
        for e in &left {
            self.entries.remove(e);
        }
        for (e, category) in new {
            let entry = self.entries.entry(e).or_insert_with(|| {
                entered.push(e);
                Entry {
                    category,
                    // New entities go first.
                    accum: 1000.0,
                    last_sent: None,
                }
            });
            entry.category = category;
            entry.accum += category.weight();
        }
        (entered, left)
    }

    /// Members allowed to be sent at `net_tick`, highest priority first.
    pub fn prioritized(&self, net_tick: u64) -> Vec<EntityId> {
        let mut eligible: Vec<(f32, EntityId)> = self
            .entries
            .iter()
            .filter(|(_, x)| {
                x.last_sent
                    .is_none_or(|t| net_tick.saturating_sub(t) >= x.category.min_interval())
            })
            .map(|(e, x)| (x.accum, *e))
            .collect();
        eligible.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        eligible.into_iter().map(|(_, e)| e).collect()
    }

    /// Record that `e` was sent at `net_tick`.
    pub fn mark_sent(&mut self, e: EntityId, net_tick: u64) {
        if let Some(x) = self.entries.get_mut(&e) {
            x.accum = 0.0;
            x.last_sent = Some(net_tick);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer() -> Viewer {
        Viewer {
            vehicle: Some(EntityId(1)),
            pos: [0.0; 3],
            police: false,
            racer: true,
            race: Some(7),
        }
    }

    fn subject(e: u32, kind: EntityKind, x: f32, race: Option<u32>) -> Subject {
        Subject {
            entity: EntityId(e),
            kind,
            pos: [x, 0.0, 0.0],
            race,
        }
    }

    #[test]
    fn categories_follow_the_priority_rules() {
        let v = viewer();
        assert_eq!(
            classify(&v, &subject(1, EntityKind::Racer, 0.0, Some(7))),
            None
        );
        assert_eq!(
            classify(&v, &subject(2, EntityKind::Racer, 5000.0, Some(7))),
            Some(Category::SameRace)
        );
        assert_eq!(
            classify(&v, &subject(3, EntityKind::AiPolice, 250.0, None)),
            Some(Category::Pursuit)
        );
        assert_eq!(
            classify(&v, &subject(4, EntityKind::Traffic, 100.0, None)),
            Some(Category::Near)
        );
        assert_eq!(
            classify(&v, &subject(5, EntityKind::Traffic, 130.0, None)),
            None
        );
        assert_eq!(
            classify(&v, &subject(6, EntityKind::Racer, 4000.0, None)),
            Some(Category::Far)
        );
    }

    #[test]
    fn far_entities_respect_rate_cap_and_budget_order() {
        let mut s = InterestState::new();
        let cands = [
            (EntityId(2), Category::Far),
            (EntityId(3), Category::SameRace),
        ];
        let (entered, _) = s.update(&cands);
        assert_eq!(entered, vec![EntityId(2), EntityId(3)]);
        let mut far_sends = 0;
        for tick in 0..120 {
            if tick > 0 {
                s.update(&cands);
            }
            let order = s.prioritized(tick);
            // Budget for one entity per tick: send the first only.
            if let Some(first) = order.first() {
                if *first == EntityId(2) {
                    far_sends += 1;
                }
                s.mark_sent(*first, tick);
            }
        }
        // Far entity is starved by the race member but still gets a few updates per 2 s.
        assert!((2..=10).contains(&far_sends), "{far_sends}");
        let (_, left) = s.update(&[(EntityId(3), Category::SameRace)]);
        assert_eq!(left, vec![EntityId(2)]);
    }
}
