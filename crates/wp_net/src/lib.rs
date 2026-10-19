//! # wp_net
//!
//! Networking (owned by workstream 07). **Current state: stub** — message encode/decode
//! (JSON for now; binary + quantisation later), the [`Transport`] abstraction, and behind the
//! `native` feature a websocket server that streams a tick counter.

#![warn(missing_docs)]

#[cfg(feature = "native")]
pub mod native;

use thiserror::Error;
use wp_contracts::{ClientToServer, ServerToClient};

/// Delivery guarantee requested for a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Ordered, guaranteed delivery.
    Reliable,
    /// Best effort (may be reliable on transports that cannot do better, e.g. WebSocket).
    Unreliable,
}

/// Network errors.
#[derive(Debug, Error)]
pub enum NetError {
    /// Message could not be decoded.
    #[error("decode error: {0}")]
    Decode(String),
    /// Transport failed or closed.
    #[error("transport error: {0}")]
    Transport(String),
}

/// A bidirectional message pipe. WebSocket now; UDP/WebTransport later.
pub trait Transport {
    /// Queue `bytes` for sending on `channel`.
    fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), NetError>;
    /// Return the next received message, if any. Never blocks.
    fn poll(&mut self) -> Result<Option<Vec<u8>>, NetError>;
}

/// Encode a client message (stub encoding: JSON bytes).
pub fn encode_client(msg: &ClientToServer) -> Vec<u8> {
    serde_json::to_vec(msg).expect("ClientToServer serializes")
}

/// Decode a client message.
pub fn decode_client(bytes: &[u8]) -> Result<ClientToServer, NetError> {
    serde_json::from_slice(bytes).map_err(|e| NetError::Decode(e.to_string()))
}

/// Encode a server message (stub encoding: JSON bytes).
pub fn encode_server(msg: &ServerToClient) -> Vec<u8> {
    serde_json::to_vec(msg).expect("ServerToClient serializes")
}

/// Decode a server message.
pub fn decode_server(bytes: &[u8]) -> Result<ServerToClient, NetError> {
    serde_json::from_slice(bytes).map_err(|e| NetError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::VehicleInput;

    #[test]
    fn client_message_round_trip() {
        let msg = ClientToServer::Input {
            tick: 42,
            input: VehicleInput {
                throttle: 0.5,
                ..Default::default()
            },
        };
        assert_eq!(decode_client(&encode_client(&msg)).unwrap(), msg);
    }
}
