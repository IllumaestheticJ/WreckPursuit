# wp_net

**Owned by workstream 07 (multiplayer netcode).**

Networking. **Currently a stub**: JSON encode/decode of `ClientToServer`/`ServerToClient`, the `Transport` trait, and (feature `native`, tokio + tokio-tungstenite) `native::serve_tick_echo`, a websocket server streaming `{"tick":N}` at 60 Hz and echoing text frames.

The `native` feature must never be enabled by anything reachable from `wp_wasm`.

## Known gaps
- Binary encoding, quantisation, prediction, interest management, roles.

## Tests

```sh
cargo test -p wp_net
```
