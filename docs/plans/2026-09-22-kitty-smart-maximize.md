# Kitty Smart Maximize Implementation Plan

**Goal:** Synchronize the focused LazyDB pane and its Kitty tab's maximize/restore state from one configurable shortcut.

**Architecture:** A new `smart-toggle-pane-maximized` action is intercepted by the TUI runtime. Kitty remote control locates the owning tab by `KITTY_WINDOW_ID`, counts layout groups (overlays are not splits), and switches explicitly to stack or the previous layout. LazyDB adopts the resulting target state only after successful remote control; a single group or a non-Kitty terminal toggles locally. Overlays and Omni consume the shortcut without changing either layout.

**Tech Stack:** Rust, Tokio process/timeout, serde JSON, Crossterm, Kitty remote control, TOML.

## Task 1: Command and input contract

- Modify `src/action.rs`, `src/config.rs`, `config/default.toml`, `src/input/keymap.rs`, and `src/app.rs`.
- Add the opt-in command with no default shortcut, route only press events, and consume it before text entry and modal handlers.
- Test Explorer, Results, SQL Normal/Insert/Visual, pending sequences, overlays, and repeat/release events with `cargo test --lib smart_maximize`.

## Task 2: Kitty synchronization

- Modify `src/terminal/kitty.rs` and `src/runtime.rs`.
- Parse the owning tab, group count and layout from `kitten @ ls`. Use explicit tab matching for `goto-layout stack` and `last-used-layout`.
- Keep subprocesses bounded and killed on cancellation; surface failures without toggling the local state.
- Test multiple OS windows, an inactive owning tab, overlay groups, missing windows, malformed responses, single-pane behavior, and mismatched local state.
- Run `cargo test --lib smart_maximize`, `cargo test --test keymap`, and `cargo fmt --check`.

## Task 3: Documentation and local activation

- Document the command and Kitty forwarding in `docs/configuration.md` and `docs/keybindings.md`.
- Build a local executable and preserve the installed binary before activating it.
- Add a smart maximize binding to `~/lazydb/settings.toml`; forward Cmd+Shift+F and Cmd+Ctrl+F in `~/.config/kitty/kitty.conf` only for `IS_LAZYDB`.
- Validate in an isolated Kitty tab, covering repeated toggles, a pre-maximized LazyDB pane, an overlay, and a single layout group. Close only the temporary test windows afterward.
- Reload Kitty config and report that existing LazyDB processes must restart to use the new executable and settings.

## Verification results

- `cargo test --lib smart_maximize`: 3 passed.
- `cargo test --test keymap`: 137 passed.
- `cargo fmt --check` and `git diff --check`: passed.
- Live Kitty 0.47.4 isolated-tab checks: repeated maximize/restore, resynchronization after independent local maximize, Omni blocking, single pane, a single overlay group, and an overlay with another split all passed. The temporary tab was closed after each run.
- Kitty's configuration parser confirms both Cmd+Shift+F and Cmd+Ctrl+F conditional mappings.
- Optimized Release build passed the same live checks, plus non-Kitty local fallback and remote-control failure preserving the local layout.
- Activated the local Release build through the existing `~/.local/bin/lazydb` target. Original binary and both configuration files are backed up under `~/lazydb/backups/kitty-smart-maximize-20260922/`.
