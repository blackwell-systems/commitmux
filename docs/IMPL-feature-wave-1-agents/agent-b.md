# Wave 1 Agent B: Store schema migrations and query implementations

You are Wave 1 Agent B. Your task is to extend the SQLite store with schema
changes for the new repo metadata columns and implement all new `Store` trait
methods in the query layer.

**Prerequisite:** Wave 0 Agent A must complete and the workspace must build
cleanly before you start. Read Agent A's completion report in the index IMPL
doc (`docs/IMPL-feature-wave-1.md`) before proceeding.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual:   $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave1-agent-b"

if [ "$ACTUAL_BRANCH" != "$EXPECTED_BRANCH" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: $EXPECTED_BRANCH"
  echo "Actual:   $ACTUAL_BRANCH"
  exit 1
fi

git worktree list | grep -q "$EXPECTED_BRANCH" || {
  echo "ISOLATION FAILURE: Worktree not in git worktree list"
  exit 1
}

echo "Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `crates/store/src/schema.rs` — modify
- `crates/store/src/queries.rs` — modify
- `crates/store/src/lib.rs` — modify (tests and `make_repo_input` helper only)

Do not touch any other files.

## 2. Interfaces You Must Implement

All in `crates/store/src/queries.rs` as `impl Store for SqliteStore`:

```rust
fn remove_repo(&self, name: &str) -> Result<()>;
fn commit_exists(&self, repo_id: i64, sha: &str) -> Result<bool>;
fn update_repo(&self, repo_id: i64, update: &RepoUpdate) -> Result<Repo>;
fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>>;

// CHANGED: sha_prefix matched with LIKE '<prefix>%'
fn get_commit(&self, repo_name: &str, sha_prefix: &str) -> Result<Option<CommitDetail>>;
```

Also update `add_repo`, `list_repos`, and `get_repo_by_name` to handle the
three new columns on `repos`.

## 3. Interfaces You May Call

From `commitmux-types` (Agent A's output):
```rust
Repo { ..., fork_of: Option<String>, author_filter: Option<String>, exclude_prefixes: Vec<String> }
RepoInput { ..., fork_of, author_filter, exclude_prefixes }
RepoUpdate { fork_of, author_filter, exclude_prefixes, default_branch }
RepoListEntry { name, commit_count, last_synced_at }
CommitmuxError::NotFound(String)
```

From `rusqlite`: standard query APIs. `zstd` for patch decompression (unchanged).

## 4. What to Implement

Read `crates/store/src/schema.rs`, `crates/store/src/queries.rs`, and
`crates/store/src/lib.rs` in full before editing.

### Schema (`crates/store/src/schema.rs`)

Add three `ALTER TABLE` statements after the existing `CREATE TABLE` block for
`repos`. Use `IF NOT EXISTS` (supported in SQLite 3.37+, and the bundled SQLite
version used by `rusqlite = "0.31"` is ≥ 3.45). Place them directly in
`SCHEMA_SQL` so they run on every `init()`:

```sql
ALTER TABLE repos ADD COLUMN IF NOT EXISTS fork_of TEXT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS author_filter TEXT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS exclude_prefixes TEXT;
```

`exclude_prefixes` stores a JSON array string. NULL → empty `Vec<String>`.

### Queries (`crates/store/src/queries.rs`)

**Helper function** — add a private helper for parsing `exclude_prefixes` from
a nullable JSON column:

```rust
fn parse_exclude_prefixes(s: Option<String>) -> Vec<String> {
    match s {
        None => vec![],
        Some(j) => serde_json::from_str::<Vec<String>>(&j).unwrap_or_default(),
    }
}
```

**`add_repo`** — update INSERT to include new columns. Serialize
`exclude_prefixes` as JSON (`serde_json::to_string(&input.exclude_prefixes)
.unwrap_or_else(|_| "[]".to_string())`). Serialize empty vec as `"[]"` (always
store non-NULL). Return `Repo` with new fields set.

**`list_repos` and `get_repo_by_name`** — update SELECT to include columns 5,
6, 7 (`fork_of`, `author_filter`, `exclude_prefixes`). Update `row_to_repo` to
parse them.

**`remove_repo`**:

```rust
fn remove_repo(&self, name: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    // 1. Look up repo_id
    let repo_id: Option<i64> = conn.query_row(
        "SELECT repo_id FROM repos WHERE name = ?1",
        params![name],
        |row| row.get(0),
    ).optional()?;

    let repo_id = repo_id.ok_or_else(|| CommitmuxError::NotFound(
        format!("repo '{}' not found", name)
    ))?;

    // 2. Delete patches
    conn.execute("DELETE FROM commit_patches WHERE repo_id = ?1", params![repo_id])?;

    // 3. Delete files
    conn.execute("DELETE FROM commit_files WHERE repo_id = ?1", params![repo_id])?;

    // 4. Delete ingest state
    conn.execute("DELETE FROM ingest_state WHERE repo_id = ?1", params![repo_id])?;

    // 5. Delete commits (drop FTS entries first via rebuild after delete)
    conn.execute("DELETE FROM commits WHERE repo_id = ?1", params![repo_id])?;

    // 6. Rebuild FTS to reflect deleted commits
    conn.execute("INSERT INTO commits_fts(commits_fts) VALUES('rebuild')", [])?;

    // 7. Delete repo
    conn.execute("DELETE FROM repos WHERE repo_id = ?1", params![repo_id])?;

    Ok(())
}
```

**`commit_exists`**:

```rust
fn commit_exists(&self, repo_id: i64, sha: &str) -> Result<bool> {
    let conn = self.conn.lock().unwrap();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM commits WHERE repo_id = ?1 AND sha = ?2",
        params![repo_id, sha],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
```

**`update_repo`**:

Build a dynamic UPDATE. Use a `Vec<String>` for the SET clauses and a
`Vec<Box<dyn rusqlite::types::ToSql>>` for bind values. Skip fields where
`RepoUpdate` field is `None`.

```rust
fn update_repo(&self, repo_id: i64, update: &RepoUpdate) -> Result<Repo> {
    let conn = self.conn.lock().unwrap();

    let mut set_clauses: Vec<String> = Vec::new();
    let mut bind_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1usize;

    if let Some(ref v) = update.fork_of {
        set_clauses.push(format!("fork_of = ?{}", idx));
        bind_vals.push(match v { Some(s) => Box::new(s.clone()), None => Box::new(rusqlite::types::Null) });
        idx += 1;
    }
    if let Some(ref v) = update.author_filter {
        set_clauses.push(format!("author_filter = ?{}", idx));
        bind_vals.push(match v { Some(s) => Box::new(s.clone()), None => Box::new(rusqlite::types::Null) });
        idx += 1;
    }
    if let Some(ref v) = update.exclude_prefixes {
        let json = serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string());
        set_clauses.push(format!("exclude_prefixes = ?{}", idx));
        bind_vals.push(Box::new(json));
        idx += 1;
    }
    if let Some(ref v) = update.default_branch {
        set_clauses.push(format!("default_branch = ?{}", idx));
        bind_vals.push(match v { Some(s) => Box::new(s.clone()), None => Box::new(rusqlite::types::Null) });
        idx += 1;
    }

    if !set_clauses.is_empty() {
        let sql = format!(
            "UPDATE repos SET {} WHERE repo_id = ?{}",
            set_clauses.join(", "),
            idx
        );
        bind_vals.push(Box::new(repo_id));
        let params: Vec<&dyn rusqlite::types::ToSql> =
            bind_vals.iter().map(|b| b.as_ref()).collect();
        conn.execute(&sql, params.as_slice())?;
    }

    // Re-fetch
    let repo = conn.query_row(
        "SELECT repo_id, name, local_path, remote_url, default_branch, fork_of, author_filter, exclude_prefixes FROM repos WHERE repo_id = ?1",
        params![repo_id],
        |row| {
            Ok(Repo {
                repo_id: row.get(0)?,
                name: row.get(1)?,
                local_path: std::path::PathBuf::from(row.get::<_, String>(2)?),
                remote_url: row.get(3)?,
                default_branch: row.get(4)?,
                fork_of: row.get(5)?,
                author_filter: row.get(6)?,
                exclude_prefixes: parse_exclude_prefixes(row.get(7)?),
            })
        },
    )?;

    Ok(repo)
}
```

**`list_repos_with_stats`**:

```rust
fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT r.name, COUNT(c.sha), i.last_synced_at
         FROM repos r
         LEFT JOIN commits c ON c.repo_id = r.repo_id
         LEFT JOIN ingest_state i ON i.repo_id = r.repo_id
         GROUP BY r.repo_id
         ORDER BY r.repo_id"
    )?;
    let rows: rusqlite::Result<Vec<RepoListEntry>> = stmt.query_map([], |row| {
        Ok(RepoListEntry {
            name: row.get(0)?,
            commit_count: row.get::<_, i64>(1)? as usize,
            last_synced_at: row.get(2)?,
        })
    })?.collect();
    Ok(rows?)
}
```

**`get_commit` (changed)**:

Replace `c.sha = ?2` with `c.sha LIKE ?2 || '%'`. Add ORDER BY to ensure
deterministic result when multiple commits share a prefix:
```sql
... WHERE r.name = ?1 AND c.sha LIKE ?2 || '%'
ORDER BY c.author_time DESC
```

## 5. Tests to Write

In `crates/store/src/lib.rs`:

1. `test_remove_repo_deletes_all` — add a repo, upsert a commit with files
   and a patch, call `remove_repo(&name)`, verify `list_repos()` is empty,
   `get_commit` returns `None`, and FTS search for the commit subject is empty.

2. `test_remove_repo_not_found` — call `remove_repo("nonexistent")`, verify
   `Err` containing "not found".

3. `test_commit_exists` — upsert a commit, verify `commit_exists` returns
   `true` for that SHA and `false` for `"unknown"`.

4. `test_update_repo_author_filter` — add a repo, call `update_repo` with
   `author_filter: Some(Some("alice@example.com".into()))`, verify returned
   `Repo.author_filter == Some("alice@example.com")`. Then call with
   `author_filter: Some(None)` and verify it is `None`.

5. `test_list_repos_with_stats` — add two repos, upsert one commit for the
   first and two for the second, call `list_repos_with_stats`, verify names and
   commit counts are correct.

6. `test_get_commit_short_sha` — upsert a commit where SHA starts with a known
   prefix. Call `get_commit(repo_name, "deadbe")` and verify it returns the
   correct commit.

7. `test_exclude_prefixes_roundtrip` — add a repo with
   `exclude_prefixes: vec!["dist/".into(), "vendor/".into()]`, call
   `get_repo_by_name`, verify returned `Repo.exclude_prefixes` matches input.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test -p commitmux-store
```

All store tests must pass (4 existing + 7 new = 11 total).

## 7. Constraints

- Schema migration must be idempotent: `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`.
- `exclude_prefixes` stored as JSON array string; NULL → `vec![]`.
- `remove_repo` must rebuild FTS after deleting commits (`VALUES('rebuild')`).
- Do not modify `crates/ingest/`, `crates/mcp/`, or `src/main.rs`.
- `add_repo` must store `exclude_prefixes` as non-NULL JSON (store `"[]"` for
  empty vec, not NULL).

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b
git add crates/store/src/schema.rs crates/store/src/queries.rs crates/store/src/lib.rs
git commit -m "wave1-agent-b: schema migrations, remove_repo, commit_exists, update_repo, list_repos_with_stats, short-sha get_commit"
```

Append your completion report to
`/Users/dayna.blackwell/code/commitmux/docs/IMPL-feature-wave-1.md`
under `### Agent B — Completion Report`.

Include:
- What you implemented
- Test results (pass/fail, count)
- Deviations from spec
- Interface contract changes
- Out-of-scope dependencies
