# MariaDB Docker Test Fixture Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Provide a reproducible Docker Compose MariaDB instance with relational objects, representative rows, and broad MariaDB type coverage for LazyDB testing.

**Architecture:** Keep the fixture isolated under `docker/mariadb/` and use MariaDB's first-start initialization directory for one idempotent SQL script. Expose the database on host port `3307` to reduce conflicts with local MySQL/MariaDB installations, persist data in a named volume, and document start, connection, inspection, and reset commands.

**Tech Stack:** Docker Compose, MariaDB 11.4, SQL initialization script, Markdown documentation.

---

### Task 1: Add the MariaDB Compose service

**Files:**
- Create: `docker-compose.mariadb.yml`

**Steps:**
1. Define the MariaDB 11.4 service, credentials, database name, host port mapping, named volume, read-only init mount, and health check.
2. Use environment-variable overrides with safe test defaults so the fixture works immediately but does not require hard-coded production credentials.

### Task 2: Add broad-schema fixture data

**Files:**
- Create: `docker/mariadb/init/001-test-fixture.sql`

**Steps:**
1. Create tables covering numeric, exact numeric, temporal, character, binary/LOB, JSON, ENUM/SET, spatial, nullable, generated, key, and foreign-key behavior.
2. Insert deterministic rows including normal values, Unicode, empty values, boundary-ish values, NULLs, and spatial WKT.
3. Add a view, indexes, trigger, procedure, and function to exercise catalog browsing beyond base tables.

### Task 3: Document usage and LazyDB connection details

**Files:**
- Create: `docs/mariadb-test-database.md`
- Modify: `README.md`

**Steps:**
1. Document prerequisites, startup, readiness, connection fields/URL, useful verification queries, and destructive reset behavior.
2. Link the fixture guide from the README quickstart area.

### Task 4: Validate the fixture

**Files:**
- Test: `docker-compose.mariadb.yml`
- Test: `docker/mariadb/init/001-test-fixture.sql`

**Steps:**
1. Run Compose configuration validation.
2. Start the service and wait for its health check.
3. Query table counts, type metadata, relational rows, programmable objects, and spatial values from inside the container.
4. Stop the service without deleting the volume, then report the verification results.
