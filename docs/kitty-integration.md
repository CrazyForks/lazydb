# Kitty smart pane focus and resize

LazyDB can hand the same directional shortcut to Kitty only after it has
exhausted its own visible panes. The integration is opt-in and has two parts.

## LazyDB settings

Add this to the active LazyDB settings file:

```toml
[keybindings.panes]
smart-focus-pane-left = ["Cmd+Ctrl+h"]
smart-focus-pane-down = ["Cmd+Ctrl+j"]
smart-focus-pane-up = ["Cmd+Ctrl+k"]
smart-focus-pane-right = ["Cmd+Ctrl+l"]
```

## Kitty mappings

With `kitty_mod cmd+ctrl`, add mappings that pass the key through while
LazyDB owns the focused Kitty window:

```conf
map --when-focus-on var:IS_LAZYDB kitty_mod+h
map --when-focus-on var:IS_LAZYDB kitty_mod+j
map --when-focus-on var:IS_LAZYDB kitty_mod+k
map --when-focus-on var:IS_LAZYDB kitty_mod+l
```

Kitty must have remote control enabled and the running LazyDB process must
inherit `KITTY_LISTEN_ON` and `KITTY_WINDOW_ID`. LazyDB uses Kitty's `kitten
@ action` command and does not use a hard-coded socket path or a user-private
Neovim kitten.

If Kitty is unavailable, internal LazyDB navigation still works and a
boundary focus request is ignored.

## Smart pane resize

Add the four optional LazyDB bindings shown in the keybindings guide. Kitty
must pass the modified keys through while LazyDB owns the focused window:

```conf
map --when-focus-on var:IS_LAZYDB kitty_mod+shift+h
map --when-focus-on var:IS_LAZYDB kitty_mod+shift+j
map --when-focus-on var:IS_LAZYDB kitty_mod+shift+k
map --when-focus-on var:IS_LAZYDB kitty_mod+shift+l
```

Install the project's `contrib/kitty/lazydb_resize.py` in Kitty's config
directory. It is intentionally separate from Neovim's `relative_resize.py`.
LazyDB first moves a visible internal divider in the requested direction;
`h`/`l` move the Explorer/main divider and `k`/`j` move the editor/results
divider. At a divider that is hidden or cannot move, it invokes the helper
through Kitty remote control to resize the current Kitty window without
changing focus. The helper uses Kitty's neighboring windows to choose the
wider/narrower or taller/shorter operation.

Existing Neovim `IS_NVIM` mappings, Kitty's ordinary resize mappings, and
LazyDB `Ctrl-w` bindings remain independent. If Kitty is unavailable, internal
LazyDB resize still works and an external boundary request is ignored.
