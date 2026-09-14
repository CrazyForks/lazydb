# Redis Preview Value Panel Implementation Plan

**Goal:** Give Redis values a focused, bordered viewer with DDL-style line numbers and scrollbars, default wrapping, a format picker, and structured syntax colors.

**Architecture:** Reuse the read-only editor renderer and its source-aware selection and scrollbar interactions. Keep wrap state on the Redis preview, and use the existing overlay/action conventions for format selection.

**Tech Stack:** Rust, Ratatui, shared editor snapshots.

### Steps
1. Update `src/editor/mod.rs` and `src/ui/read_only_sql.rs` for source-preserving wrapped preview rows and editor scrolling.
2. Update `src/ui/redis_browser.rs` to place metadata above a standalone VALUE border and render through the shared viewer.
3. Add format-picker and wrap actions in `src/action.rs`, `src/app.rs`, `src/model/workspace.rs`, `src/input/keymap.rs`, and `src/input/mouse.rs`; render the picker in `src/ui/mod.rs`.
4. Verify structured highlighting and add regression coverage for wrapping, controls, selection, and scrolling.
5. Run `cargo fmt --check`, targeted tests, and `cargo clippy --all-targets -- -D warnings`.
