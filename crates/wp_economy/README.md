# wp_economy

**Owned by workstream 08 (economy, progression, mods).**

Money, rep, repair billing, impound, mods, garage. **Currently a stub**: `PlayerAccount`, the `AccountStore` trait with a `BTreeMap`-backed `InMemoryStore`, and `repair_bill`.

## Known gaps
- Everything else (billing, impound, mods, payouts, SQLite store).

## Tests

```sh
cargo test -p wp_economy
```
