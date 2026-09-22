# Kitty smart pane focus

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
boundary focus request is ignored. The feature changes focus only; it does
not add cross-application pane resizing. Existing Neovim `IS_NVIM` mappings
and LazyDB `Ctrl-w` bindings remain independent.
