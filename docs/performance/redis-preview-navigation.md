# Redis Preview Navigation Performance

This document records the baseline and final measurements for the Redis value
preview navigation optimization. Measurements are intentionally kept separate
from correctness tests because terminal speed and CPU scheduling vary by host.

## Baseline setup

- Worktree: `task/redis-preview-navigation-performance`
- Rust: `1.94`
- Profile: `--release`
- Viewport: `80 x 24` cells
- Mode: JSON-like text, Wrap enabled
- Samples: 30 warm navigation/snapshot samples per fixture
- Fixture: synthetic Java-deserialization-shaped JSON with `class`, `fields`,
  `annotations`, nested maps, and numeric values
- Not measured yet: terminal diff/write latency and full Redis App rendering

Run the editor baseline with:

```bash
cargo test --release --lib preview_navigation_baseline -- \
  --ignored --nocapture --test-threads=1
```

The benchmark prints the fixture size, cold snapshot duration, and P95 values
for navigation and wrapped preview snapshot generation. The correctness tests
near the benchmark verify that wrapping, cursor visibility, and source text are
unchanged.

## Baseline results

To be filled after running the release benchmark on the target development
machine. Record the CPU, terminal size, commit, and complete command together
with the output; do not compare values collected with different fixtures.

| Logical lines | Bytes | Cold snapshot | Navigation P95 | Snapshot P95 |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 7,911 | 1.053 ms | 24.7 µs | 438.7 µs |
| 1,000 | 84,404 | 3.756 ms | 53.8 µs | 3.135 ms |
| 10,000 | 858,397 | 27.860 ms | 466.9 µs | 27.651 ms |

Measured with `cargo test --release` on the development machine at the
baseline commit. The benchmark did not include terminal diff/write latency.

## Interpretation

The baseline is expected to include full-document projection, highlighting, and
wrapped-row construction on every snapshot. It is a reference for later
changes, not a CI timing threshold. After the cache and visible-row work lands,
the same command and fixtures should be rerun and the table should include the
worktree commit and before/after workload counters.

## Optimized editor-path results

After the preview document, highlight, wrap-index, visible-row, cursor-location,
and read-only navigation changes, the same release benchmark produced:

| Logical lines | Bytes | Cold snapshot | Navigation P95 | Snapshot P95 |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 7,911 | 0.423 ms | 49.0 µs | 28.5 µs |
| 1,000 | 84,404 | 1.437 ms | 22.1 µs | 55.8 µs |
| 10,000 | 858,397 | 15.116 ms | 57.1 µs | 361.4 µs |

The cold path remains proportional to document size because it builds the
revision-scoped projection/highlight/wrap caches. Warm snapshot work is bounded
by the visible viewport and no longer tracks the full document size. Full
terminal diff/write latency was not measured, so redraw coalescing remains
intentionally unimplemented.

The optional long-line shared-span optimization was measured but not added in
this iteration: the current editor-path result did not justify introducing a
more complex segment ownership model without a real terminal trace.
