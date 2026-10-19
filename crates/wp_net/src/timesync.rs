//! Client-side estimate of the server tick from ping/pong round trips.
//!
//! Each `Pong` gives `server_tick` at the moment the server handled the ping; assuming a
//! symmetric path the server is at `server_tick + rtt/2` *now*. The offset between that and
//! the client clock is smoothed (EMA, α = 0.1) and snapped when it is off by more than 1 s.
//! The client predicts `lead_ticks()` ahead of the estimate so inputs arrive before the server
//! simulates their tick.

use crate::{TICKS_PER_NET_TICK, TICK_HZ};

const TICKS_PER_MS: f64 = TICK_HZ as f64 / 1000.0;

/// Smoothed clock offset, RTT and jitter.
#[derive(Debug, Clone, Default)]
pub struct TimeSync {
    offset_ticks: Option<f64>,
    rtt_ms: f64,
    jitter_ms: f64,
    samples: u32,
    last_ping_ms: Option<f64>,
}

impl TimeSync {
    /// No samples yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wire form of a client timestamp (ms, wrapping `u32`).
    pub fn wire_ms(now_ms: f64) -> u32 {
        (now_ms.max(0.0) as u64 & 0xFFFF_FFFF) as u32
    }

    /// If a ping is due at `now_ms`, record it and return the timestamp to send.
    /// Pings go out every 100 ms for the first 5 samples, then every 500 ms.
    pub fn ping_due(&mut self, now_ms: f64) -> Option<u32> {
        let interval = if self.samples < 5 { 100.0 } else { 500.0 };
        if self.last_ping_ms.is_none_or(|t| now_ms - t >= interval) {
            self.last_ping_ms = Some(now_ms);
            Some(Self::wire_ms(now_ms))
        } else {
            None
        }
    }

    /// Feed a pong received at `now_ms`.
    pub fn on_pong(&mut self, now_ms: f64, client_ms: u32, server_tick: u64) {
        let rtt = Self::wire_ms(now_ms).wrapping_sub(client_ms) as f64;
        if rtt > 10_000.0 {
            return; // stale or bogus
        }
        if self.samples == 0 {
            self.rtt_ms = rtt;
            self.jitter_ms = 0.0;
        } else {
            self.jitter_ms += 0.25 * ((rtt - self.rtt_ms).abs() - self.jitter_ms);
            self.rtt_ms += 0.125 * (rtt - self.rtt_ms);
        }
        self.samples += 1;
        let sample = server_tick as f64 + rtt * 0.5 * TICKS_PER_MS - now_ms * TICKS_PER_MS;
        self.offset_ticks = Some(match self.offset_ticks {
            Some(o) if (sample - o).abs() <= TICK_HZ as f64 => o + 0.1 * (sample - o),
            _ => sample,
        });
    }

    /// True once at least one pong arrived.
    pub fn synced(&self) -> bool {
        self.offset_ticks.is_some()
    }

    /// Estimated server tick (fractional) at `now_ms`.
    pub fn server_tick(&self, now_ms: f64) -> Option<f64> {
        self.offset_ticks.map(|o| o + now_ms * TICKS_PER_MS)
    }

    /// Smoothed round-trip time, ms.
    pub fn rtt_ms(&self) -> f64 {
        self.rtt_ms
    }

    /// Smoothed RTT deviation, ms.
    pub fn jitter_ms(&self) -> f64 {
        self.jitter_ms
    }

    /// How far ahead of the server estimate the client predicts, ticks: one-way latency plus
    /// two jitter deviations, plus one net tick (the server simulates in batches of
    /// [`TICKS_PER_NET_TICK`]) and one tick of slack.
    pub fn lead_ticks(&self) -> f64 {
        (self.rtt_ms * 0.5 + 2.0 * self.jitter_ms + 4.0) * TICKS_PER_MS
            + TICKS_PER_NET_TICK as f64
            + 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_on_server_clock() {
        let mut ts = TimeSync::new();
        // Server clock runs 5000 ticks ahead of client ms*0.24; one-way 50 ms.
        let server_at = |client_ms: f64| 5000.0 + client_ms * TICKS_PER_MS;
        let mut now = 0.0;
        for _ in 0..50 {
            let sent = ts.ping_due(now).unwrap_or(TimeSync::wire_ms(now));
            let handled = server_at(now + 50.0) as u64;
            now += 100.0;
            ts.on_pong(now, sent, handled);
            now += 400.0;
        }
        let est = ts.server_tick(now).unwrap();
        assert!(
            (est - server_at(now)).abs() < 2.0,
            "{est} vs {}",
            server_at(now)
        );
        assert!((ts.rtt_ms() - 100.0).abs() < 1.0);
        assert!(ts.lead_ticks() > 12.0 && ts.lead_ticks() < 20.0);
    }
}
