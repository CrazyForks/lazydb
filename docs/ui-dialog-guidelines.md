# TUI Dialog and Footer Guidelines

This document is the implementation contract for interactive LazyDB overlays.
It keeps contextual actions, window actions, status, and keyboard help visually
separate without changing the keyboard-first interaction model.

## Information hierarchy

Every interactive window should expose up to three independent regions:

1. **Context actions** belong next to the content they mutate. For example,
   column insertion and removal belong beside the `COLUMNS` heading.
2. **Window actions** belong in a stable bottom action row. These actions affect
   the complete form or overlay, such as `Cancel` and `Review SQL`.
3. **Keyboard help** belongs in its own low-emphasis row below the action row.
   It describes the current focus context; it is not another button row.

The standard shape is:

```text
│ COLUMNS · 6                         [ Add Column ] [ Remove ] │
│ #  NAME              TYPE          NULLABLE                   │
│ 1  id                integer       NOT NULL                   │
│ ...                                                           │
│ ───────────────────────────────────────────────────────────── │
│ 2 pending changes                  [ Cancel ] [ Review SQL ]  │
│ ───────────────────────────────────────────────────────────── │
│ Tab next  Shift+Tab previous  Enter review SQL  Esc cancel    │
```

The exact help text changes with focus. A focused action uses `Enter activate`;
the table editor's General and Columns contexts continue to use `Enter review
SQL`, because that is the actual keymap behavior.

## Visual contract

- Buttons use bracketed controls and a stable reserved focus marker.
- Primary emphasis, danger tone, and keyboard focus are independent concepts.
- The focused control uses reverse/background emphasis and a `>` marker; color
  alone is never the only focus indicator.
- Shortcut keys use bold normal text; shortcut descriptions use muted text.
- Shortcut help is left aligned and never rendered as a second button row.
- Disabled controls keep their measured position but do not create hit regions.
- A selected table row and a focused action are separate states and must remain
  distinguishable.

## Interaction contract

| Context | Primary keyboard behavior |
| --- | --- |
| Text field | text editing; `Tab`/`Shift+Tab` changes focus |
| Table columns | `j`/`k` or Up/Down changes row; `Tab` changes focus |
| Table General / Columns | `Enter` previews SQL |
| Table action | `Enter` activates; arrows move within the action group |
| Column details field | text editing or toggle behavior; `Enter` confirms where currently supported |
| Cancel / close action | `Enter` cancels or closes the current overlay |

The help row must describe the behavior of the currently focused context. It
must not claim that every Enter key activates a button, and it must not combine
opposite actions into one clickable hint.

## Layout contract

- Measure all labels using terminal cell width, never byte length.
- Use a fixed footer height for a given window size; focus changes must not move
  the content viewport.
- Reserve the maximum width of alternate labels such as `Remove Column` and
  `Restore Column` so toggling state does not move neighboring controls.
- At narrow widths, wrap an entire action item or move the action group to a
  pre-measured second row. Never split a key/description pair.
- At very small sizes, hide optional status and separators before hiding the
  primary action or escape guidance.
- All rendered and clickable rectangles must stay inside their parent area.

## Current-window migration matrix

| Window / overlay | Context actions | Window actions | Help / verification |
| --- | --- | --- | --- |
| Catalog table editor | Add, Remove/Restore beside Columns | Cancel, Review SQL | `src/ui/catalog_editor.rs`, `src/input/keymap.rs` |
| Column details | None | Confirm, Cancel | Separate type guidance/error from controls |
| Other catalog forms | Object-specific choices near fields | Cancel, Review SQL | Preserve object-specific focus rules |
| Profile manager | Test and field choices near form | Cancel, Save | Enter saves and connects; Space activates local options/buttons |
| Redis object/table editor | Type/toggle/row controls near data | Apply, Cancel | Separate field focus from action focus |
| Execution confirmation | SQL preview controls | Cancel, Execute | Preserve danger tone and default focus |
| Transaction confirmation | Preview navigation | Commit, Rollback, Cancel | Verify index-to-action mapping |
| Delete/unsaved confirmations | None | Confirm, Cancel or Discard | Top overlay owns all interaction |
| Read-only/detail overlays | Copy or view controls when present | Close | Use a low-emphasis help row |
| Main workspace footer | None | None | Continue using contextual shortcut catalog |

## Review checklist per migration

- [ ] The visual position identifies whether an item is contextual, global, or help.
- [ ] The focused control has one strong non-color indicator.
- [ ] The selected object and keyboard focus are both visible.
- [ ] Help text matches the actual `Keymap` behavior for the current context.
- [ ] Mouse hit regions use the same measured rectangles as rendering.
- [ ] Disabled or hidden items do not remain clickable.
- [ ] Footer height is stable while focus changes.
- [ ] Standard, compact, narrow, plain-color, and Unicode text cases are checked.
