# Redis value preview formats

Redis string values are inspected from their original bytes. In **Auto** mode
LazyDB validates the value before selecting a structured decoder. The currently
supported serialization formats are:

- Java Object Serialization (`AC ED 00 05` streams)
- PHP serialization
- Python Pickle protocols 0–5, where supported by `serde-pickle`

The preview menu also provides Raw, JSON, YAML, Table, and Hex views. Raw and
Hex always use the original bytes and never run a serialization decoder.

## Automatic selection

Auto selects a format only after the candidate has parsed successfully. A
strong serialization header may still be reported as an incomplete or invalid
candidate, but it will not be shown as a successful structured preview.
Ordinary UTF-8 text falls back to Raw; unknown binary data falls back to Hex.

Selecting a format manually applies to the current key. Selecting another key
returns to Auto so one key's format does not affect another key. Collection
cells are detected independently because a Redis collection can contain mixed
value encodings.

## Lossless fallback and limits

The preview keeps the Redis bytes even when parsing fails. Binary strings,
Pickle bytes, PHP references, and non-JSON types are represented with explicit
type metadata where possible. Unsupported or over-budget data can still be
viewed with Raw or Hex.

Serialization parsing is bounded by a 4 MiB input budget and an 8 MiB rendered
output budget. These limits protect the TUI from unexpectedly large values;
they do not modify or delete the Redis value.
