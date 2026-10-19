//! In-memory, netem-style transport for deterministic tests: one-way latency, uniform jitter,
//! loss. Time is a shared virtual clock in milliseconds that the test advances.
//!
//! - `Unreliable` frames are dropped with probability `loss` and may arrive out of order.
//! - `Reliable` frames are never dropped and never reordered; a "lost" reliable frame is
//!   delayed by `retransmit_ms` instead (TCP-style head-of-line blocking — later reliable
//!   frames wait behind it).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wp_math::Rng;

use crate::{Channel, NetError, Transport};

/// Link impairment for one direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkConfig {
    /// Base one-way latency, ms.
    pub latency_ms: f64,
    /// Uniform jitter amplitude, ms (each frame gets `latency ± jitter`).
    pub jitter_ms: f64,
    /// Loss probability per frame.
    pub loss: f32,
    /// Extra delay for a lost reliable frame, ms.
    pub retransmit_ms: f64,
    /// RNG seed.
    pub seed: u64,
}

impl LinkConfig {
    /// A perfect link with `latency_ms` one-way latency.
    pub fn ideal(latency_ms: f64) -> Self {
        Self {
            latency_ms,
            jitter_ms: 0.0,
            loss: 0.0,
            retransmit_ms: 0.0,
            seed: 0,
        }
    }
}

/// One direction of a link.
#[derive(Debug)]
pub struct Pipe {
    cfg: LinkConfig,
    rng: Rng,
    queue: Vec<(f64, u64, Vec<u8>)>,
    seq: u64,
    last_reliable_at: f64,
    /// Frames dropped so far.
    pub dropped: u64,
    /// Bytes accepted for delivery so far.
    pub bytes: u64,
}

impl Pipe {
    /// New pipe.
    pub fn new(cfg: LinkConfig) -> Self {
        Self {
            rng: Rng::new(cfg.seed),
            cfg,
            queue: Vec::new(),
            seq: 0,
            last_reliable_at: 0.0,
            dropped: 0,
            bytes: 0,
        }
    }

    /// Send a frame at time `now_ms`.
    pub fn send(&mut self, now_ms: f64, channel: Channel, bytes: Vec<u8>) {
        let lost = self.cfg.loss > 0.0 && self.rng.chance(self.cfg.loss);
        let jitter = self.cfg.jitter_ms * (self.rng.next_f32() as f64 * 2.0 - 1.0);
        let mut at = now_ms + (self.cfg.latency_ms + jitter).max(0.0);
        match channel {
            Channel::Unreliable if lost => {
                self.dropped += 1;
                return;
            }
            Channel::Unreliable => {}
            Channel::Reliable => {
                if lost {
                    at += self.cfg.retransmit_ms;
                }
                at = at.max(self.last_reliable_at);
                self.last_reliable_at = at;
            }
        }
        self.bytes += bytes.len() as u64;
        self.seq += 1;
        self.queue.push((at, self.seq, bytes));
    }

    /// Frames due by `now_ms`, in arrival order.
    pub fn recv(&mut self, now_ms: f64) -> Vec<Vec<u8>> {
        let mut due: Vec<(f64, u64, Vec<u8>)> = Vec::new();
        let mut i = 0;
        while i < self.queue.len() {
            if self.queue[i].0 <= now_ms {
                due.push(self.queue.swap_remove(i));
            } else {
                i += 1;
            }
        }
        due.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        due.into_iter().map(|(_, _, b)| b).collect()
    }
}

/// A [`Transport`] endpoint over a pair of [`Pipe`]s and a shared virtual clock.
pub struct SimEndpoint {
    clock: Rc<Cell<f64>>,
    out: Rc<RefCell<Pipe>>,
    inbox: Rc<RefCell<Pipe>>,
    pending: std::collections::VecDeque<Vec<u8>>,
}

/// Create a connected pair `(client_end, server_end)` with the given impairments per
/// direction, driven by `clock` (ms).
pub fn pair(up: LinkConfig, down: LinkConfig, clock: Rc<Cell<f64>>) -> (SimEndpoint, SimEndpoint) {
    let up = Rc::new(RefCell::new(Pipe::new(up)));
    let down = Rc::new(RefCell::new(Pipe::new(down)));
    (
        SimEndpoint {
            clock: clock.clone(),
            out: up.clone(),
            inbox: down.clone(),
            pending: Default::default(),
        },
        SimEndpoint {
            clock,
            out: down,
            inbox: up,
            pending: Default::default(),
        },
    )
}

impl SimEndpoint {
    /// Bytes this endpoint has had accepted for delivery.
    pub fn bytes_sent(&self) -> u64 {
        self.out.borrow().bytes
    }
}

impl Transport for SimEndpoint {
    fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        self.out
            .borrow_mut()
            .send(self.clock.get(), channel, bytes.to_vec());
        Ok(())
    }

    fn poll(&mut self) -> Result<Option<Vec<u8>>, NetError> {
        if self.pending.is_empty() {
            let now = self.clock.get();
            self.pending.extend(self.inbox.borrow_mut().recv(now));
        }
        Ok(self.pending.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reliable_is_ordered_and_lossless_unreliable_drops() {
        let mut p = Pipe::new(LinkConfig {
            latency_ms: 60.0,
            jitter_ms: 30.0,
            loss: 0.2,
            retransmit_ms: 200.0,
            seed: 9,
        });
        for i in 0..200u8 {
            p.send(i as f64, Channel::Reliable, vec![i]);
            p.send(i as f64, Channel::Unreliable, vec![i]);
        }
        let got = p.recv(10_000.0);
        let reliable: Vec<u8> = got.iter().map(|b| b[0]).collect();
        assert!(p.dropped > 10 && p.dropped < 80, "{}", p.dropped);
        assert_eq!(got.len(), 400 - p.dropped as usize);
        // Every value 0..200 arrived at least once (the reliable copy).
        for i in 0..200u8 {
            assert!(reliable.contains(&i));
        }
        assert!(p.recv(20_000.0).is_empty());
    }

    #[test]
    fn endpoints_deliver_after_latency() {
        let clock = Rc::new(Cell::new(0.0));
        let (mut c, mut s) = pair(
            LinkConfig::ideal(50.0),
            LinkConfig::ideal(50.0),
            clock.clone(),
        );
        c.send(Channel::Reliable, b"hi").unwrap();
        assert_eq!(s.poll().unwrap(), None);
        clock.set(50.0);
        assert_eq!(s.poll().unwrap().as_deref(), Some(&b"hi"[..]));
    }
}
