# Redis Browser Performance Checks

The Redis browser uses progressive SCAN results, bounded value pages, and a
temporary SQLite key index for large keyspaces. These checks are intentionally
separate from the normal unit-test suite.

## Baseline

```bash
cargo test --test redis_scan --test redis_values --test redis_key_tree \
  --test redis_browser_tabs --test redis_loading_lifecycle
```

## Synthetic scale check

This does not connect to a user Redis instance and is ignored by default:

```bash
cargo test --release --test redis_scale -- --ignored --nocapture
```

Record wall time and peak RSS for 10,000, 100,000, and 1,000,000 keys. The
important measurements are first page latency, keyset page latency, retained
bytes, and whether the process stays responsive while the index is built.

## Real Redis fixture

Use an isolated Redis server and load binary-safe keys, deep prefixes, large
Strings, Hash/Set/ZSet values, and keys that expire during scanning. Record:

- SCAN request count and average batch size;
- first visible page time;
- value page P95 latency;
- memory/index bytes and pending queue depth;
- behavior after refresh, database switch, disconnect, and reconnect.

SCAN is not a snapshot. Key additions, deletions, expiration, and duplicate
results during a scan are expected and must be reflected in the UI state.
