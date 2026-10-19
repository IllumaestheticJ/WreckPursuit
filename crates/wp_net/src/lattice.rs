//! Damage-lattice replication helpers: the 32-bit lattice hash, delta-compressed lattice
//! snapshots and the [`ReplicaDamage`] trait clients re-run contacts through.
//!
//! Node positions never travel in normal play. Clients re-run the server's quantised contacts
//! through their own damage model; every 2 s the server sends a [`lattice_hash`] per visible
//! vehicle and a client that disagrees asks for a [`LatticeSnapshot`] of that one vehicle.
//! The hash quantises displacements to 1 mm so float noise below that never counts as a
//! mismatch; snapshots are exact so a resync is bit-exact.

use wp_contracts::{ContactImpulse, HandlingEffects, ModeTable, SimEvent};
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

/// What the client needs from a damage model to replicate it. Implemented for
/// `wp_damage::DamageModel`; tests use [`crate::testkit::ToyLattice`].
pub trait ReplicaDamage {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
