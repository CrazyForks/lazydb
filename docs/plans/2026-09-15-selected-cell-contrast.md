# Selected Cell Contrast Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Improve selected-cell text contrast consistently in Relation Data and Result Set grids.

**Architecture:** Both views already share `src/ui/data_grid.rs`; add the active-cell style to `Theme`, then apply it after row and cell semantic styles. Preserve row/cell status colors outside the active cell and use terminal reverse video in plain color mode.

**Tech Stack:** Rust, Ratatui, Cargo tests.

---

### Task 1: Add selected-cell contrast regression coverage

**Files:**
- Modify: `src/ui/data_grid.rs:965-1023`

Add assertions for active-cell foreground/background and ensure the selected cell remains readable for normal, NULL, and unsupported values.

### Task 2: Implement the shared active-cell style

**Files:**
- Modify: `src/ui/theme.rs:171`
- Modify: `src/ui/data_grid.rs:255-273`

Add `Theme::grid_active_cell_style`, including a `Color::Reset` fallback, and apply the full style to the active cell after row highlighting.

### Task 3: Validate the change

Run `cargo fmt --check`, the focused data-grid tests, and the full test suite.
