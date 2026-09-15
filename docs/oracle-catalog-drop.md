# Oracle Catalog Drop

LazyDB supports dropping Oracle tables, views, and sequences from the Explorer.

## Supported objects

The catalog planner currently supports:

- `Table`
- `View`
- `Sequence`

Schema and service names are used to select the Oracle execution target. The
generated object SQL contains the schema and object name, not the service name.
For example, an object discovered as `SUPPORTDB / MFGSUPPORT / tt1` produces:

```sql
DROP TABLE "MFGSUPPORT"."tt1"
```

Names retain the case returned by Oracle metadata and are quoted as Oracle
identifiers. This matters for quoted lower-case objects.

## Safety and execution

The Explorer first creates and validates a drop plan, then shows the SQL in a
confirmation overlay. Cancelling the overlay does not execute SQL. The normal
Oracle drop operation does not add `CASCADE CONSTRAINTS`, `PURGE`, or
`IF EXISTS`; dependent-object and permission errors are returned by Oracle
instead of being hidden or converted into a destructive cascade.

The drop executes on a short-lived connection for the catalog target rather
than the shared console connection. This prevents Oracle DDL's implicit commit
behavior from committing unrelated uncommitted console work. A successful
`DROP` cannot be undone with `ROLLBACK`; whether the object can be recovered
from the recycle bin depends on Oracle configuration and the exact object and
drop options.

## Failure and refresh behavior

If planning, connection, permission, dependency, or SQL execution fails, the
object remains in the Explorer and the Oracle error is shown after terminal
text sanitization. A result from an old connection generation or catalog epoch
is ignored.

After success, LazyDB removes the object subtree from the Explorer, clears its
completion/search entries, reselects its former parent when possible, and
invalidates an open relation tab for the exact deleted `CatalogId`. A late
metadata or query response must not make that tab writable again.

## Integration test configuration

The Oracle integration test uses the same environment variables as the Oracle
adapter tests:

```text
LAZYDB_TEST_ORACLE_URL
LAZYDB_TEST_ORACLE_USER
LAZYDB_TEST_ORACLE_PASSWORD
```

Without these variables the test reports `SKIP`. With credentials configured,
connection failures other than a missing native Oracle client fail the test.

Run the integration test with:

```bash
cargo test --features driver-oracle --test oracle_catalog_drop -- --nocapture
```

The fixture creates a uniquely named table and removes only that table.
