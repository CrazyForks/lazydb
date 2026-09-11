# Native Driver Validation Record

This record is intentionally a gate, not a support claim. Oracle and Db2
implementation must not proceed until a prepared environment supplies the
vendor client libraries, a real server, credentials, and cancellation tests.

## Current Worktree Result

- Oracle JDBC probe: passed against the supplied service URL using the locally
  cached `ojdbc11` driver. The password is intentionally not recorded.
- Oracle authenticated identity: `MFGSUPPORT`; current schema: `MFGSUPPORT`.
- Oracle database/service reported by the server: `SUPPORTD` / `supportdb`.
- Oracle metadata probe: passed through `ALL_TAB_COLUMNS` for the authenticated
  schema, including zero-client-side assumptions about table names.
- Oracle type probe: passed for `NUMBER(38,0)`, fractional `TIMESTAMP`, and
  `CLOB` length retrieval. A combined BLOB expression was rejected with Oracle
  error `932`; this must be tested later with a real BLOB column or an explicit
  Oracle conversion expression.
- Oracle write/transaction probe: not run. The supplied account's write
  permission and authorization to create test objects were not established.
- Oracle native Rust driver probe: compiled successfully with
  `driver-oracle` (now the default Cargo feature), but runtime execution is blocked on this macOS host with
  `DPI-1047` because `libclntsh.dylib` is not installed. The adapter currently
  supports connect/probe and a basic unbound query path; catalog, transaction,
  binding, typed values and cancellation work remains gated by follow-up tests.
- The client is now installed at `~/.local/share/lazydb/oracle/current`; native
  Oracle tests pass without `DYLD_LIBRARY_PATH` by using explicit application
  discovery.
- Oracle native integration tests skip only when the configured environment
  reports `DPI-1047`; authentication and query failures remain test failures
  when the Oracle Client is installed.
- Db2 CLI probe: blocked. `db2` is not installed.
- ODBC probe: blocked. `isql` and `odbcinst` are not installed.
- Cargo dependency probe: optional `oracle 0.6.3` is declared, but its feature
  build remains blocked until ODPI-C and an Oracle Client are installed.

## Required Evidence Before P3/P4

1. Connect and authenticate against a supported server version.
2. Read zero-row and non-empty result metadata.
3. Round-trip NULL, exact decimal, date/time, binary and bounded large values.
4. Execute DML, rollback it, and verify connection reuse.
5. Verify statement cancellation and distinguish cancellation from unknown
   commit outcome.
6. Capture sanitized vendor error code, SQLSTATE and message categories.
7. Build and smoke-test every release target that claims the native feature.

Until all seven checks pass for a driver, its profile may be designed but the
driver must remain unavailable at runtime and absent from the supported driver
claim.
