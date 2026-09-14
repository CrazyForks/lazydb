# Redis Browser

## Loading modes

The browser initially loads Redis keys progressively. `SCAN COUNT` is only a
server hint; an empty response with a non-zero cursor is not complete. Results
are de-duplicated by raw key bytes.

Small keyspaces remain in memory. Large keyspaces may use a temporary SQLite
index so that key pages and prefix tree pages stay bounded. The index is local,
temporary, and is removed when its owner closes.

## Search and paging

Local find searches keys already received by the current browser. Redis MATCH
patterns control a new server-side scan and start a new scan generation.

Value previews are typed and bounded. Strings use byte ranges; Hashes and Sets
use cursors; Lists and Sorted Sets use ranges. A page can be partial and can
have a next position. The browser preserves raw bytes and only escapes them at
display time.

Redis scans and value pages are not snapshots. A key may disappear or change
type between metadata and value reads. Old responses are discarded after a
database switch, refresh, tab close, or connection generation change.

## Navigation and panes

Opening a Redis database from Explorer focuses the Keys pane. As soon as the
first non-empty scan batch is available, the first top-level tree node is
selected automatically. Later scan batches preserve the current selection.

Redis tabs are labeled `Redis @connection-name`; the name belongs to the tab's
connection profile rather than to the currently active connection.

Use `j`/`k` to move through Keys, `o` or `Enter` to expand/collapse a folder,
and `Left`/`Right` to collapse or enter a folder. `h` and `l` switch between
the Keys and Preview panes. In Preview, `j`/`k` scroll the loaded value and
PageUp/PageDown page the focused pane. Keys folders use the same group icons
and selection colors as Explorer.

Keys and Preview maintain independent vertical scroll positions. Scrollbars
appear only when the loaded content overflows the pane; scrolling a Preview
does not change the selected key or issue a new Redis request.

## Limits

The `[redis]` section in `settings.toml` controls scan hints, value page sizes,
memory/index thresholds, preview debounce, metadata cache capacity, and disk
limits. Limits constrain client retention and display; Redis COUNT does not
guarantee a response element count.
