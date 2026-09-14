# Value preview fixtures

These fixtures are deterministic byte samples for the Redis preview decoder.
They are intentionally documented before implementation so format detection
can distinguish complete input, truncated input, and invalid input.

Planned fixture groups:

- UTF-8 text, JSON objects/arrays/scalars, malformed and truncated JSON.
- YAML mappings, sequences, block scalars, anchors, aliases, and multi-document input.
- Java serialization beginning with `AC ED 00 05`, including objects and collections.
- PHP serialization with multibyte strings, binary strings, arrays, objects, and references.
- Pickle protocols 2–5 with bytes, tuple/set values, and non-string dictionary keys.
- Protobuf wire values with multi-byte tags, repeated fields, empty length-delimited values,
  malformed varints, and nested binary payloads.

Binary samples should be added as small checked-in files or byte arrays with their
origin and expected normalized structure recorded in the test that consumes them.
No fixture should contain credentials or production data.
