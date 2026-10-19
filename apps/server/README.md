# wp_server (`wp-server` binary)

**Owned by workstream 07 (multiplayer netcode).**

Headless authoritative server. **Stub:** `just server [port]` accepts websocket connections, streams `{"tick":N}` at 60 Hz and echoes text frames.

## Known gaps
- No simulation, roles, snapshots or config yet.

## Tests

```sh
cargo test -p wp_server
```
