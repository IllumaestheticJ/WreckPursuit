# tools/meta-cli

Command-line client for the meta service (`apps/meta`). Built on `wp_meta::client::MetaClient`.

```sh
cargo run -p meta-cli -- --help
export WP_META_URL=http://127.0.0.1:8090

meta-cli signup me@example.com 'password123' Racer1     # prints a session token
WP_META_TOKEN=wps_... meta-cli me
WP_META_TOKEN=wps_... meta-cli queue --role racer --mode arcade --bracket C --region eu-west
WP_META_TOKEN=wps_... meta-cli status
meta-cli leaderboard clean_rating --period weekly
meta-cli leaderboard time_attack:docks_sprint

# admin (token = WP_META_ADMIN_TOKEN; --actor names you in the audit log)
export WP_META_TOKEN=$WP_META_ADMIN_TOKEN WP_META_ACTOR=alice
meta-cli admin health
meta-cli admin ban 42 "wallhack" --duration-s 86400
meta-cli admin adjust 42 -500 "refund reversal"
meta-cli admin flag-set illegal_day_race_frequency 0.2
meta-cli admin audit --limit 20

# deployment smoke test (instance key = WP_META_INSTANCE_KEY)
meta-cli smoke --instance-key "$WP_META_INSTANCE_KEY" --bots 8
```

Output is pretty-printed JSON; errors go to stderr with exit code 1.

Tests: the client is exercised by `apps/meta/tests` (`cargo test -p wp_meta`); the CLI itself is
a thin argument layer. Known gaps: no interactive login / token storage — pass tokens via
`WP_META_TOKEN`.
