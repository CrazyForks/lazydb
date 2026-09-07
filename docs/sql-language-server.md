# SQL Language Server Design Notes

This document records the implementation boundaries for the Neovim SQL and
MyBatis language server. It is intentionally separate from the user-facing
configuration documentation while the protocol is still being implemented.

## Task 1 Findings

### SQL parser locations

The pinned `sqlparser` version is `0.62.0`. Its public `ParserError` has three
variants, including a string-based `ParserError(String)` variant. It does not
carry a public structured source span. Parser messages currently contain line
and column wording for common syntax errors, but that message format is not a
stable source-location API.

The tokenizer does expose structured `Location` and `Span` values. Parser
errors do not reliably do so: for example, an incomplete predicate ending at
EOF is reported as `Expected: an expression, found: EOF` with no line or
column. The future diagnostic implementation must therefore use this order:

1. Tokenize first and use the structured tokenizer location when tokenization
   fails.
2. Parse the SQL and use a parser message only for its human-readable text;
   do not claim a token location unless a future parser version exposes a
   tested structured location.
3. If the parser cannot provide a reliable location, publish a diagnostic over the current SQL
   statement (or the mapped embedded unit), not a guessed token range.

The parser probe is covered by `tests/sql_parser_location_probe.rs`. A future
upgrade of `sqlparser` must rerun this probe before changing the diagnostic
adapter.

### Protocol dependency decision

Do not hand-roll JSON-RPC framing. Task 2 must select a maintained Rust LSP
protocol implementation after checking its current MSRV, Tokio integration,
stdio transport, cancellation, and license. The server stdout contract is
strict: only framed JSON-RPC messages may be written there; logs belong on
stderr.

### MyBatis XML recovery strategy

The first implementation will use a bounded, source-span-preserving scanner
for mapper XML rather than removing tags with a regular expression. It must
recognize XML comments, CDATA, entity references, CRUD statement boundaries,
and incomplete tags without fetching external DTDs or entities.

An XML parser may be used as an optional validation pass when it can recover
from incomplete input, but a strict parser cannot be the only source of SQL
units because completion must work while the user is typing. The scanner will
mark entity-decoded, parameter-replaced, synthesized, and unknown dynamic
segments separately so that unsafe edits and untrustworthy diagnostics can be
suppressed.

The initial XML fixture matrix is:

- multiple `select`, `insert`, `update`, and `delete` statements;
- ordinary text and CDATA SQL;
- `&lt;`, `&gt;`, `&amp;`, and numeric character references;
- `#{parameter}` and `${dynamic}` expressions;
- comments, DOCTYPE declarations, incomplete tags, and malformed attributes;
- non-SQL mapper elements and attributes containing SQL-looking words.

Dynamic tags and `<include>` are not considered statically valid merely
because their text can be concatenated. Until their source mapping is
implemented, affected units must provide only safe local completion and must
suppress whole-unit syntax diagnostics.

## M1/M2 diagnostic boundary

The first release provides offline SQL syntax diagnostics for plain SQL and
trusted static MyBatis SQL. It does not execute SQL, query the database for
validation, infer Java/OGNL types, or prove that every dynamic SQL branch is
valid.

## Current implementation boundary

The stdio server, document lifecycle, UTF-16 conversion, offline completion,
syntax diagnostics, static MyBatis extraction, and conservative dynamic-tag
handling are implemented and covered by protocol/integration tests. The
profile-backed catalog loader is still a separate follow-up: the current
server uses an in-process single-flight cache with an empty offline snapshot,
so it must not claim table or column completion from a live connection yet.
