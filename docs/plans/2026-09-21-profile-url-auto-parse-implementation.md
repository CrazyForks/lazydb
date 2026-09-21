# Connection URL Auto-Parse Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Automatically parse edited connection URLs after a short debounce while keeping Enter dedicated to save-and-connect.

**Architecture:** Keep URL editing in `ProfileDraft`; content changes schedule a trailing debounce deadline. The existing 33ms TUI ticker advances due drafts synchronously. Automatic parsing updates structured fields without rewriting the URL editor buffer; save validates only the latest synchronization state and never parses implicitly.

**Tech Stack:** Rust 1.94, Tokio ticker, crossterm/ratatui TUI, existing URL parser and profile reducer tests.

---

## Ordered implementation and review checkpoints

1. **Draft debounce state** — add a 300ms deadline, injectable time-based advancement, and tests for reset/due/invalid behavior.
2. **Non-destructive parser application** — separate parsing and field application from URL normalization; preserve cursor, selection, password redaction, and edit history; retain explicit commit behavior for blur/Test/Scope.
3. **All edit paths and lifecycle boundaries** — cover insert/paste/delete/undo/redo, structured-field changes, mouse focus, and stale draft disposal.
4. **Ticker integration** — advance only the active idle form draft and redraw only when state changes.
5. **Save semantics** — remove `commit_url()` from `save_profile_draft`; Pending/Invalid are rejected by validation, Synced follows the existing SaveProfile flow; Enter remains unchanged.
6. **UI feedback** — render pending/invalid/synced URL status without disabling Cancel; keep Save/Enter wording as save-and-connect.
7. **Regression and documentation** — update profile/keymap/reducer/UI tests and `docs/keybindings.md`.
8. **Final review** — run fmt, clippy, focused tests, all targets/all features where practical, `git diff --check`, and manual TUI acceptance.

Each item must be implemented, tested, and reviewed with `git diff` before the next item starts. Do not create commits unless explicitly requested.
