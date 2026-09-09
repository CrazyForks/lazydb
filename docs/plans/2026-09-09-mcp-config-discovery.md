# MCP Configuration Discovery Implementation Plan

**Goal:** Register LazyDB in existing client configurations with explicit scope and format-preserving edits.

**Architecture:** A shared client configuration adapter discovers sources, reads server entries, and generates incremental changes. Setup selects and writes one target per client; doctor uses the same discovery and parsing without executing client commands.

**Tech Stack:** Rust, clap, jsonc-parser CST, toml_edit, tempfile test fixtures.

## Steps

1. Extend `src/cli.rs` and `src/main.rs` with optional scope and client-config selection; retain non-interactive project defaults and the existing Rust setup entry point.
2. Add `src/agent/client_config.rs` for environment-aware discovery, structured entry inspection, JSONC/TOML insertion, and atomic writes with changed-file checks. Test using injected configuration directories, never modifying process environment or real client files.
3. Update `src/agent/setup.rs` to show discovered targets, select scope, distinguish create/add/unchanged/conflict/invalid, and preserve existing entries.
4. Update `src/agent/doctor.rs` to inspect all discovered sources, report layering/disabled entries and the limits of static inspection.
5. Update `tests/mcp_setup.rs`, `tests/mcp_doctor.rs` and `docs/coding-agent-access.md` with scope, idempotency, formatting, invalid-config, custom-path and project-context cases.
6. Run `cargo test --test mcp_setup --test mcp_doctor`, adapter unit tests, `cargo fmt --check`, and `cargo check --all-targets`. Inspect the final diff.

No commits or changes to the user's installed client configurations are part of this implementation.
