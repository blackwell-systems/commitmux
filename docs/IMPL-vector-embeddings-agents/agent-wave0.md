# Wave 0 Agent: Schema Migration

You are Wave 0. Your task is to extend the SQLite schema to support vector embeddings:
add `embed_enabled` to `repos`, create the `config` table, create `commit_embed_map`
(stable integer ID per commit for vec0 compatibility), create the `commit_embeddings`
vec0 virtual table, load the `sqlite-vec` extension, and apply all migrations idempotently.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

**Step 1: Attempt environment correction**

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-schema 2>/dev/null || true
```

**Step 2: Verify isolation**

```bash
ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-schema"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual: $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave0-schema"

if [ "$ACTUAL_BRANCH" != "$EXPECTED_BRANCH" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: $EXPECTED_BRANCH"
  echo "Actual: $ACTUAL_BRANCH"
  exit 1
fi

git worktree list | grep -q "$EXPECTED_BRANCH" || {
  echo "ISOLATION FAILURE: Worktree not in git worktree list"
  exit 1
}

echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

If verification fails: write failure to completion report in `docs/IMPL-vector-embeddings.md` under `### Agent Wave 0 — Completion Report` and stop.

## 1. File Ownership

- `crates/store/src/schema.rs` — modify
- `crates/store/src/lib.rs` — modify
- `crates/store/Cargo.toml` — modify (add sqlite-vec dependency)

Do NOT touch any other files.

## 2. Interfaces You Must Implement

None — this is pure schema and extension loading. No trait changes.

## 3. Interfaces You May Call

Existing pattern in `crates/store/src/lib.rs`:
```rust
// REPO_MIGRATIONS pattern — idempotent ALTER TABLE:
pub const REPO_MIGRATIONS: &[&str] = &[
    "ALTER TABLE repos ADD COLUMN fork_of TEXT",
    // ...
];

// In init(), applied with "duplicate column name" error suppression:
for &sql in schema::REPO_MIGRATIONS {
    match conn.execute_batch(sql) {
        Ok(()) => {}
        Err(rusqlite::Error::SqliteFailure(_, Some(ref msg)))
            if msg.contains("duplicate column name") => {}
        Err(e) => return Err(e.into()),
    }
}
```

## 4. What to Implement

Read `crates/store/src/schema.rs` and `crates/store/src/lib.rs` in full before making changes.
Read `docs/vector-embeddings.md` for context.

### 4a. Add `sqlite-vec` dependency

In `crates/store/Cargo.toml`, add:
```toml
sqlite-vec = "0.1"
```

### 4b. New tables in `SCHEMA_SQL`

Append to `SCHEMA_SQL` in `crates/store/src/schema.rs`:

```sql
CREATE TABLE IF NOT EXISTS config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS commit_embed_map (
    embed_id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id  INTEGER NOT NULL,
    sha      TEXT NOT NULL,
    UNIQUE(repo_id, sha)
);

CREATE VIRTUAL TABLE IF NOT EXISTS commit_embeddings USING vec0(
    embed_id INTEGER PRIMARY KEY,
    embedding FLOAT[768]
);
```

### 4c. New migration for `embed_enabled` column

Add a new `EMBED_MIGRATIONS` constant:

```rust
/// Migration statements for embedding support columns.
pub const EMBED_MIGRATIONS: &[&str] = &[
    "ALTER TABLE repos ADD COLUMN embed_enabled INTEGER NOT NULL DEFAULT 0",
];
```

### 4d. Update `SqliteStore::init()` to load sqlite-vec and run embed migrations

In `crates/store/src/lib.rs`, the `init()` function currently:
1. Runs `SCHEMA_SQL`
2. Applies `REPO_MIGRATIONS`

Change it to:
1. **Load sqlite-vec extension first** (before SCHEMA_SQL, since SCHEMA_SQL creates the vec0 virtual table which requires the extension)
2. Run `SCHEMA_SQL`
3. Apply `REPO_MIGRATIONS`
4. Apply `EMBED_MIGRATIONS` (same pattern as REPO_MIGRATIONS — ignore "duplicate column name")

Loading sqlite-vec:
```rust
use sqlite_vec;

// In init(), before execute_batch(SCHEMA_SQL):
sqlite_vec::load(&conn).map_err(|e| {
    commitmux_types::CommitmuxError::Config(format!("Failed to load sqlite-vec: {e}"))
})?;
```

Check the `sqlite-vec` crate docs for the exact API — it may be `sqlite_vec::load(&conn)` or
`sqlite_vec::sqlite3_vec_init(...)`. Use whatever the crate exposes. If the crate API differs
slightly, adapt — the goal is: extension loaded before schema DDL.

### 4e. Add schema mod import if needed

Ensure `schema::EMBED_MIGRATIONS` is accessible from `lib.rs` — it's in the same `schema` module,
so no import changes are needed as long as `EMBED_MIGRATIONS` is `pub const`.

## 5. Tests to Write

Add to `crates/store/src/lib.rs` tests:

1. `test_config_table_exists` — open an in-memory store, call `store.conn.lock().unwrap()`,
   query `SELECT name FROM sqlite_master WHERE type='table' AND name='config'`, assert it exists.

2. `test_commit_embed_map_table_exists` — same pattern, check `commit_embed_map` exists.

3. `test_embed_migrations_idempotent` — call `SqliteStore::open_in_memory()` twice (separate
   calls) to verify `init()` is idempotent (no panic on duplicate column).

Note: Do NOT write tests that use `vec0` queries directly — that requires embeddings to be loaded
which is Wave 1B's job. Just verify the tables exist.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-schema
cargo build
cargo clippy -- -D warnings
cargo test -p commitmux-store
```

All existing store tests must still pass. New schema tests must pass.

## 7. Constraints

- `CREATE VIRTUAL TABLE IF NOT EXISTS commit_embeddings USING vec0(...)` will fail if sqlite-vec
  is not loaded. Verify the load happens before SCHEMA_SQL in `init()`.
- The `embedding FLOAT[768]` dimension must match `nomic-embed-text` (768 dims). It is baked into
  the schema; changing models to a different dimension requires a schema migration. This is
  acceptable — the design doc designates `nomic-embed-text` as the default.
- Do not change `SCHEMA_SQL`'s `commits` table structure. The `commit_embed_map` table provides
  the integer key bridge; `commits` itself is unchanged.
- All schema changes must be idempotent (`IF NOT EXISTS` for tables, "duplicate column name"
  suppression for ALTER TABLE).

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-schema
git add crates/store/src/schema.rs crates/store/src/lib.rs crates/store/Cargo.toml
git commit -m "wave0-schema: add embed schema (config, commit_embed_map, commit_embeddings, embed_enabled)"
```

Append to `docs/IMPL-vector-embeddings.md` under `### Agent Wave 0 — Completion Report`:

```yaml
### Agent Wave 0 — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave0-schema
commit: {sha}
files_changed:
  - crates/store/src/schema.rs
  - crates/store/src/lib.rs
  - crates/store/Cargo.toml
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_config_table_exists
  - test_commit_embed_map_table_exists
  - test_embed_migrations_idempotent
verification: PASS | FAIL ({command} — N/N tests)
```
