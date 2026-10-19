//! Damage-lattice replication helpers: the 32-bit lattice hash, delta-compressed lattice
//! snapshots and the [`ReplicaDamage`] trait clients re-run contacts through.
//!
//! Node positions never travel in normal play. Clients re-run the server's quantised contacts
//! through their own damage model; every 2 s the server sends a [`lattice_hash`] per visible
//! vehicle and a client that disagrees asks for a [`LatticeSnapshot`] of that one vehicle.
//! The hash quantises displacements to 1 mm so float noise below that never counts as a
//! mismatch; snapshots are exact so a resync is bit-exact.

use serde::{Deserialize, Serialize};
use wp_contracts::{ContactImpulse, HandlingEffects, ModeTable, SimEvent};
use wp_damage::FullDamageSnapshot;
use wp_math::StateHasher;

use crate::quant::LATTICE_SCALE;
use crate::wire::{ByteReader, ByteWriter};
use crate::NetError;

/// Largest lattice the protocol accepts (nodes).
pub const MAX_LATTICE_NODES: usize = 4096;

fn q_disp(node: f32, rest: f32) -> i32 {
    let v = ((node - rest) * LATTICE_SCALE).round();
    if v.is_nan() {
        0
    } else {
        v.clamp(-1e9, 1e9) as i32
    }
}

/// 32-bit hash of a lattice's displacement field: every node's displacement from its rest
/// position, quantised to 1 mm, in node order (FNV-1a 64 folded to 32 bits).
///
pub fn lattice_hash(nodes: &[[f32; 3]], rest: &[[f32; 3]]) -> u32 {
    let mut h = StateHasher::new();
    h.write_u32(nodes.len() as u32);
    for (n, r) in nodes.iter().zip(rest) {
        for axis in 0..3 {
            h.write_bytes(&q_disp(n[axis], r[axis]).to_le_bytes());
        }
    }
    let v = h.finish();
    (v as u32) ^ ((v >> 32) as u32)
}

/// Node positions of one vehicle at one tick, delta-compressed against the rest pose: only
/// nodes that differ from rest (bitwise) are carried, as exact `f32`s.
///
/// Exact rather than 1 mm-quantised on purpose: damage response is non-linear in node
/// position, so a replica restored from rounded positions drifts away again on the very next
/// contact and triggers another resync (measured in `tests/lockstep.rs`: one injected desync
/// caused 9 mismatches with 1 mm snapshots, 1 with exact ones). Resyncs are rare, so the
/// extra bytes (12 per dented node) are cheap.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LatticeSnapshot {
    /// Number of nodes in the lattice.
    pub node_count: usize,
    /// `(node index, position)` of every node not at its rest position, ascending.
    pub displaced: Vec<(usize, [f32; 3])>,
}

impl LatticeSnapshot {
    /// Capture `nodes` relative to `rest`.
    pub fn capture(nodes: &[[f32; 3]], rest: &[[f32; 3]]) -> Self {
        Self {
            node_count: nodes.len(),
            displaced: nodes
                .iter()
                .zip(rest)
                .enumerate()
                .filter(|(_, (n, r))| n.map(f32::to_bits) != r.map(f32::to_bits))
                .map(|(i, (n, _))| (i, *n))
                .collect(),
        }
    }

    /// True if no node is displaced (pristine car).
    pub fn is_pristine(&self) -> bool {
        self.displaced.is_empty()
    }

    /// Absolute node positions for a lattice with rest pose `rest`.
    pub fn positions(&self, rest: &[[f32; 3]]) -> Vec<[f32; 3]> {
        let mut out = rest.to_vec();
        for (i, p) in &self.displaced {
            if let Some(n) = out.get_mut(*i) {
                *n = *p;
            }
        }
        out
    }

    /// Encode: `varint node_count`, a presence bitmap (`ceil(n/8)` bytes, bit `i%8` of byte
    /// `i/8` set when node `i` is displaced), then `3 × f32` per displaced node. A pristine
    /// 120-node car costs 16 bytes.
    pub fn encode(&self, w: &mut ByteWriter) {
        let n = self.node_count;
        w.varint(n as u64);
        let mut bitmap = vec![0u8; n.div_ceil(8)];
        for (i, _) in &self.displaced {
            bitmap[i / 8] |= 1 << (i % 8);
        }
        for b in &bitmap {
            w.u8(*b);
        }
        for (_, p) in &self.displaced {
            p.iter().for_each(|v| w.f32(*v));
        }
    }

    /// Decode (see [`LatticeSnapshot::encode`]).
    pub fn decode(r: &mut ByteReader) -> Result<Self, NetError> {
        let n = r.len(MAX_LATTICE_NODES)?;
        let mut bitmap = vec![0u8; n.div_ceil(8)];
        for b in &mut bitmap {
            *b = r.u8()?;
        }
        let mut displaced = Vec::new();
        for i in 0..n {
            if bitmap[i / 8] & (1 << (i % 8)) != 0 {
                displaced.push((i, [r.f32()?, r.f32()?, r.f32()?]));
            }
        }
        // Bits past node_count must be clear.
        if n % 8 != 0 && bitmap.last().is_some_and(|b| b >> (n % 8) != 0) {
            return Err(NetError::Decode("lattice bitmap overflow".into()));
        }
        Ok(Self {
            node_count: n,
            displaced,
        })
    }
}

/// Sparse full damage state (NET-N5): `wp_damage`'s lossless [`FullDamageSnapshot`] with
/// every per-beam array reduced to the entries that differ (bitwise) from the pristine car,
/// and node positions left out (they travel in the [`LatticeSnapshot`] next to it). The
/// per-beam plastic-work ledger (`beam_energy_j`) is left out too: it only feeds the
/// diagnostic `total_plastic_energy_j`, never node motion, health or effects, and it is the
/// largest array of a dented car (a quarter of the bytes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SparseDamageState {
    beam_rest_m: Vec<(u32, f32)>,
    beam_plastic_strain: Vec<(u32, f32)>,
    beam_flags: Vec<(u32, u8)>,
    beam_comp_plastic_strain: Vec<(u32, f32)>,
    beam_energy_j: Vec<(u32, f32)>,
    /// Everything else (maps, sets, scalars); its large arrays are empty.
    base: FullDamageSnapshot,
}

fn diff_f32(a: &[f32], pristine: &[f32]) -> Vec<(u32, f32)> {
    a.iter()
        .zip(pristine)
        .enumerate()
        .filter(|(_, (x, p))| x.to_bits() != p.to_bits())
        .map(|(i, (x, _))| (i as u32, *x))
        .collect()
}

fn apply_f32(out: &mut [f32], diff: &[(u32, f32)]) -> Result<(), NetError> {
    for (i, v) in diff {
        *out.get_mut(*i as usize)
            .ok_or_else(|| NetError::Decode("damage state index".into()))? = *v;
    }
    Ok(())
}

/// Encode `state` sparsely against `pristine` (the same lattice, undamaged): postcard of the
/// per-beam differences plus the small remainder. A settled dent of a few hundred beams costs
/// a few kB; the server only sends it with lattice snapshots of damaged cars. The per-beam
/// energy ledger is not carried (see `SparseDamageState`): a decoded state has the pristine
/// (zero) ledger.
pub fn encode_damage_state(state: &FullDamageSnapshot, pristine: &FullDamageSnapshot) -> Vec<u8> {
    let (s, p) = (&state.snapshot, &pristine.snapshot);
    let (comp, energy) = match (&state.ext, &pristine.ext) {
        (Some(x), Some(px)) => (
            diff_f32(&x.beam_comp_plastic_strain, &px.beam_comp_plastic_strain),
            Vec::new(),
        ),
        _ => (Vec::new(), Vec::new()),
    };
    let mut base = state.clone();
    base.snapshot.node_pos_m.clear();
    base.snapshot.beam_rest_m.clear();
    base.snapshot.beam_plastic_strain.clear();
    base.snapshot.beam_flags.clear();
    if let Some(x) = base.ext.as_mut() {
        x.beam_comp_plastic_strain.clear();
        x.beam_energy_j.clear();
    }
    let sparse = SparseDamageState {
        beam_rest_m: diff_f32(&s.beam_rest_m, &p.beam_rest_m),
        beam_plastic_strain: diff_f32(&s.beam_plastic_strain, &p.beam_plastic_strain),
        beam_flags: s
            .beam_flags
            .iter()
            .zip(&p.beam_flags)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, (a, _))| (i as u32, *a))
            .collect(),
        beam_comp_plastic_strain: comp,
        beam_energy_j: energy,
        base,
    };
    postcard::to_allocvec(&sparse).expect("damage state serializes")
}

/// Decode [`encode_damage_state`] bytes against the same `pristine` state, with the node
/// positions from the accompanying [`LatticeSnapshot`].
pub fn decode_damage_state(
    bytes: &[u8],
    pristine: &FullDamageSnapshot,
    nodes: &[[f32; 3]],
) -> Result<FullDamageSnapshot, NetError> {
    let sparse: SparseDamageState =
        postcard::from_bytes(bytes).map_err(|e| NetError::Decode(format!("damage state: {e}")))?;
    let mut out = sparse.base;
    let p = &pristine.snapshot;
    out.snapshot.node_pos_m = nodes.to_vec();
    out.snapshot.beam_rest_m = p.beam_rest_m.clone();
    apply_f32(&mut out.snapshot.beam_rest_m, &sparse.beam_rest_m)?;
    out.snapshot.beam_plastic_strain = p.beam_plastic_strain.clone();
    apply_f32(
        &mut out.snapshot.beam_plastic_strain,
        &sparse.beam_plastic_strain,
    )?;
    out.snapshot.beam_flags = p.beam_flags.clone();
    for (i, v) in &sparse.beam_flags {
        *out.snapshot
            .beam_flags
            .get_mut(*i as usize)
            .ok_or_else(|| NetError::Decode("damage state index".into()))? = *v;
    }
    if let (Some(x), Some(px)) = (out.ext.as_mut(), pristine.ext.as_ref()) {
        x.beam_comp_plastic_strain = px.beam_comp_plastic_strain.clone();
        apply_f32(
            &mut x.beam_comp_plastic_strain,
            &sparse.beam_comp_plastic_strain,
        )?;
        x.beam_energy_j = px.beam_energy_j.clone();
        apply_f32(&mut x.beam_energy_j, &sparse.beam_energy_j)?;
    }
    Ok(out)
}

/// The contacts of one vehicle's damage step that can change its lattice, or nothing when the
/// whole batch is a no-op for `wp_damage::DamageModel::apply_contacts`.
///
/// Real Rapier contacts arrive every physics tick while cars lean on each other; replicating
/// all of them costs more than the snapshots. `wp_damage` skips a contact whose closing speed
/// is below `min_contact_speed_mps` or whose impulse is not positive, and skips a contact
/// group whose energy is below `min_contact_energy_j`, without touching any state. Dropping
/// those therefore leaves the replica bit-identical; the authority still applies everything.
/// The group test uses an upper bound (all contacts in one group, the largest closing share)
/// with a 1 % margin against float rounding. Mirrors `wp_damage/src/contact.rs`
/// (`add_contacts`); `tests::dropped_contacts_are_noops` fails if that logic changes.
pub fn damage_relevant(
    contacts: &[ContactImpulse],
    tuning: &wp_damage::DamageTuning,
    mode: &ModeTable,
) -> Vec<ContactImpulse> {
    let scale = mode.impulse_to_lattice_scale;
    let kept: Vec<ContactImpulse> = contacts
        .iter()
        .filter(|c| {
            let closing = c.rel_speed_mps.abs();
            let j = c.impulse_ns.max(0.0) * scale;
            closing.is_finite()
                && closing >= tuning.min_contact_speed_mps
                && j.is_finite()
                && j > 0.0
        })
        .copied()
        .collect();
    let share = 1.0f32
        .max(tuning.dynamic_closing_share)
        .max(tuning.dynamic_closing_share_side);
    let energy_bound: f32 = kept
        .iter()
        .map(|c| 0.5 * c.impulse_ns.max(0.0) * scale * c.rel_speed_mps.abs() * share)
        .sum();
    if energy_bound < 0.99 * tuning.min_contact_energy_j {
        Vec::new()
    } else {
        kept
    }
}

/// What the client needs from a damage model to replicate it. Implemented for
/// `wp_damage::DamageModel`; tests use [`crate::testkit::ToyLattice`].
pub trait ReplicaDamage: Send {
    /// Same contract as `wp_contracts::DamageSim::apply_contacts`.
    fn apply_contacts(
        &mut self,
        contacts: &[ContactImpulse],
        mode: &ModeTable,
        dt: f32,
        out: &mut Vec<SimEvent>,
    );
    /// Current node positions (chassis space), indexed by node id.
    fn node_positions(&self) -> &[[f32; 3]];
    /// Rest node positions (chassis space), indexed by node id.
    fn rest_positions(&self) -> Vec<[f32; 3]>;
    /// Overwrite node positions (from a [`LatticeSnapshot`]).
    fn restore_nodes(&mut self, nodes: &[[f32; 3]]);
    /// Current damage → handling effects.
    fn handling_effects(&self) -> HandlingEffects;
    /// The model's internal solver runs once every this many steps; a replica must be at the
    /// same step count modulo this as the server's model. Default 1 (every step).
    fn solver_period(&self) -> u64 {
        1
    }
    /// Full internal damage state for a resync ([`encode_damage_state`] bytes), or `None`
    /// when the model has none beyond node positions or is undamaged. Default: `None`.
    fn save_state(&self) -> Option<Vec<u8>> {
        None
    }
    /// Restore [`ReplicaDamage::save_state`] bytes, with `nodes` from the lattice snapshot.
    /// Returns false when unsupported or the bytes do not fit this model. Default: false.
    fn restore_state(&mut self, _bytes: &[u8], _nodes: &[[f32; 3]]) -> bool {
        false
    }
}

/// The undamaged state of `m`'s lattice (same counters, all damage reset).
fn pristine_of(m: &wp_damage::DamageModel) -> FullDamageSnapshot {
    let mut p = m.clone();
    p.reset();
    p.save_full()
}

impl ReplicaDamage for wp_damage::DamageModel {
    fn apply_contacts(
        &mut self,
        contacts: &[ContactImpulse],
        mode: &ModeTable,
        dt: f32,
        out: &mut Vec<SimEvent>,
    ) {
        wp_contracts::DamageSim::apply_contacts(self, contacts, mode, dt, out);
    }

    fn node_positions(&self) -> &[[f32; 3]] {
        wp_contracts::DamageSim::node_positions(self)
    }

    fn rest_positions(&self) -> Vec<[f32; 3]> {
        self.lattice().nodes.iter().map(|n| n.pos_m).collect()
    }

    fn restore_nodes(&mut self, nodes: &[[f32; 3]]) {
        self.restore_node_positions(nodes);
    }

    fn handling_effects(&self) -> HandlingEffects {
        wp_contracts::DamageSim::handling_effects(self)
    }

    fn solver_period(&self) -> u64 {
        self.tuning().solver_divider.max(1) as u64
    }

    fn save_state(&self) -> Option<Vec<u8>> {
        let pristine = pristine_of(self);
        let full = self.save_full();
        if full == pristine {
            return None;
        }
        Some(encode_damage_state(&full, &pristine))
    }

    fn restore_state(&mut self, bytes: &[u8], nodes: &[[f32; 3]]) -> bool {
        let pristine = pristine_of(self);
        match decode_damage_state(bytes, &pristine, nodes) {
            Ok(full) => self.restore_full(&full).is_ok(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Random contact batches, many of them gentle: applying only [`damage_relevant`] gives
    /// bit-identical lattices to applying everything.
    #[test]
    fn dropped_contacts_are_noops() {
        use wp_contracts::DamageSim as D;
        let dir = crate::testkit::repo_data_dir();
        let car = wp_contracts::load_car(dir.join("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(dir.join("modes/arcade.toml")).unwrap();
        let mut full = wp_damage::DamageModel::new(&car.lattice, &car.materials);
        let mut filtered = wp_damage::DamageModel::new(&car.lattice, &car.materials);
        let tuning = full.tuning().clone();
        let mut rng = wp_math::Rng::new(5);
        let (mut dropped, mut kept_batches) = (0, 0);
        let mut sink = Vec::new();
        for tick in 0..2400 {
            let n = if tick % 3 == 0 {
                rng.range_f32(0.0, 4.0) as usize
            } else {
                0
            };
            let batch: Vec<ContactImpulse> = (0..n)
                .map(|_| {
                    // Mostly resting / scraping contacts, now and then a real hit.
                    let hard = rng.range_f32(0.0, 1.0) < 0.03;
                    let speed = if hard {
                        rng.range_f32(3.0, 15.0)
                    } else {
                        rng.range_f32(0.0, 1.6)
                    };
                    let impulse = if hard {
                        rng.range_f32(2_000.0, 20_000.0)
                    } else {
                        rng.range_f32(-5.0, 400.0)
                    };
                    crate::quant::QContact::round_trip(&ContactImpulse {
                        point_local_m: [
                            rng.range_f32(-0.9, 0.9),
                            rng.range_f32(-2.2, 2.2),
                            rng.range_f32(0.2, 1.2),
                        ],
                        normal_local: if rng.range_f32(0.0, 1.0) < 0.5 {
                            [0.0, -1.0, 0.0]
                        } else {
                            [1.0, 0.0, 0.0]
                        },
                        impulse_ns: impulse,
                        tangent_impulse_local_ns: [rng.range_f32(-50.0, 50.0), 0.0, 0.0],
                        rel_speed_mps: speed,
                        other: None,
                        other_mass_kg: if rng.range_f32(0.0, 1.0) < 0.5 {
                            0.0
                        } else {
                            1400.0
                        },
                    })
                })
                .collect();
            let relevant = damage_relevant(&batch, &tuning, &mode);
            dropped += batch.len() - relevant.len();
            kept_batches += usize::from(!relevant.is_empty());
            D::apply_contacts(
                &mut full,
                &batch,
                &mode,
                wp_contracts::PHYSICS_DT,
                &mut sink,
            );
            D::apply_contacts(
                &mut filtered,
                &relevant,
                &mode,
                wp_contracts::PHYSICS_DT,
                &mut sink,
            );
        }
        assert!(dropped > 500, "test should drop many contacts ({dropped})");
        assert!(kept_batches > 5, "and keep some hits ({kept_batches})");
        assert_ne!(
            D::node_positions(&full),
            car.lattice
                .nodes
                .iter()
                .map(|n| n.pos_m)
                .collect::<Vec<_>>()
                .as_slice(),
            "the kept hits dent the car"
        );
        assert_eq!(D::node_positions(&full), D::node_positions(&filtered));
        assert_eq!(D::handling_effects(&full), D::handling_effects(&filtered));
    }

    #[test]
    fn snapshot_restore_hashes_identically() {
        let rest: Vec<[f32; 3]> = (0..50).map(|i| [i as f32 * 0.1, 0.5, 0.2]).collect();
        let mut nodes = rest.clone();
        nodes[3][1] -= 0.0734;
        nodes[17][0] += 0.20049;
        nodes[49][2] -= 0.00004; // below resolution
        let snap = LatticeSnapshot::capture(&nodes, &rest);
        assert_eq!(snap.displaced.len(), 3);
        let mut w = ByteWriter::new();
        snap.encode(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), 1 + 7 + 3 * 12, "bitmap + 3 dented nodes");
        let back = LatticeSnapshot::decode(&mut ByteReader::new(&bytes)).unwrap();
        assert_eq!(back, snap);
        let restored = back.positions(&rest);
        assert_eq!(restored, nodes, "restore is bit-exact");
        assert_eq!(lattice_hash(&restored, &rest), lattice_hash(&nodes, &rest));
        assert_ne!(lattice_hash(&nodes, &rest), lattice_hash(&rest, &rest));
        // Sub-millimetre noise does not change the hash.
        assert_eq!(lattice_hash(&nodes[..49], &rest[..49]), {
            let mut n = nodes.clone();
            n[10][0] += 0.0001;
            lattice_hash(&n[..49], &rest[..49])
        });
        assert!(LatticeSnapshot::capture(&rest, &rest).is_pristine());
    }
}
