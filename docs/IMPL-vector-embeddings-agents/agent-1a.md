# Wave 1 Agent A: Types, Store, and MockStore Cascade

You are Wave 1 Agent A. Your task is to add new types and Store trait methods to
`crates/types/src/lib.rs`, implement them in SQL in `crates/store/src/queries.rs`,
add `embed_enabled` to the `Repo`/`RepoInput`/`RepoUpdate` structs, and update
`crates/ingest/src/lib.rs` MockStore with stub implementations of all new methods.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a"
if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"; echo "Actual: $ACTUAL_DIR"; exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-a" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: wave1-agent-a"; echo "Actual: $ACTUAL_BRANCH"; exit 1
fi

git worktree list | grep -q "wave1-agent-a" || { echo "ISOLATION FAILURE: Not in worktree list"; exit 1; }
echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `crates/types/src/lib.rs` — modify
- `crates/store/src/queries.rs` — modify
- `crates/ingest/src/lib.rs` — modify (MockStore cascade fix only — do not touch test logic)

Do NOT touch any other files.

## 2. Interfaces You Must Implement

### New types in `crates/types/src/lib.rs`

```rust
/// Lightweight commit info for embedding document construction.
/// Also carries the metadata fields written to vec0 auxiliary columns on store_embedding.
#[derive(Debug, Clone)]
pub struct EmbedCommit {
    pub repo_id: i64,
    pub sha: String,
    pub subject: String,
    pub body: Option<String>,
    pub files_changed: Vec<String>,    // file paths only
    pub patch_preview: Option<String>,
    // Auxiliary column fields (stored alongside the vector for join-free search)
    pub author_name: String,
    pub repo_name: String,
    pub author_time: i64,
}

/// Options for semantic (vector) search.
#[derive(Debug, Clone, Default)]
pub struct SemanticSearchOpts {
    pub repos: Option<Vec<String>>,   // filter by repo name
    pub since: Option<i64>,           // unix timestamp lower bound
    pub limit: Option<usize>,         // default 10
}
```

### New Store trait methods in `crates/types/src/lib.rs`

```rust
fn get_config(&self, key: &str) -> Result<Option<String>>;
fn set_config(&self, key: &str, value: &str) -> Result<()>;
fn get_commits_without_embeddings(&self, repo_id: i64, limit: usize) -> Result<Vec<EmbedCommit>>;
fn store_embedding(
    &self,
    repo_id: i64,
    sha: &str,
    subject: &str,
    author_name: &str,
    repo_name: &str,
    author_time: i64,
    patch_preview: Option<&str>,
    embedding: &[f32],
) -> Result<()>;
fn search_semantic(&self, embedding: &[f32], opts: &SemanticSearchOpts) -> Result<Vec<SearchResult>>;
```

### Struct field additions

Add `embed_enabled: bool` to:
- `Repo` struct
- `RepoInput` struct
- `RepoUpdate` struct (as `Option<bool>`)

### SQL implementations in `crates/store/src/queries.rs`

All 5 new Store methods implemented for `SqliteStore`.

## 3. Interfaces You May Call

All existing Store methods and helpers in queries.rs (e.g., `row_to_repo`, `parse_exclude_prefixes`).
The schema from Wave 0 (`config`, `commit_embed_map`, `commit_embeddings` tables) is present.

## 4. What to Implement

Read `crates/types/src/lib.rs`, `crates/store/src/queries.rs`, and `crates/ingest/src/lib.rs`
in full before making changes. Read `docs/vector-embeddings.md` for context.

### 4a. Add `embed_enabled` to structs

In `crates/types/src/lib.rs`:
- Add `pub embed_enabled: bool` to `Repo` (after `exclude_prefixes`)
- Add `pub embed_enabled: bool` to `RepoInput` (after `exclude_prefixes`)
- Add `pub embed_enabled: Option<bool>` to `RepoUpdate`

### 4b. Update `row_to_repo` in `queries.rs`

`row_to_repo` currently reads 8 columns (indices 0–7). Add column index 8 for `embed_enabled`:

```rust
fn row_to_repo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Repo> {
    Ok(Repo {
        repo_id: row.get(0)?,
        name: row.get(1)?,
        local_path: std::path::PathBuf::from(row.get::<_, String>(2)?),
        remote_url: row.get(3)?,
        default_branch: row.get(4)?,
        fork_of: row.get(5)?,
        author_filter: row.get(6)?,
        exclude_prefixes: parse_exclude_prefixes(row.get(7)?),
        embed_enabled: row.get::<_, i64>(8).unwrap_or(0) != 0,
    })
}
```

Update ALL SELECT statements that feed `row_to_repo` to include `embed_enabled` as column 8:
- `list_repos()`
- `get_repo_by_name()`
- `add_repo()` (the SELECT after INSERT)
- `update_repo()` (the SELECT after UPDATE)
- Any other query that calls `row_to_repo`

Also update `add_repo()` INSERT statement to include `embed_enabled`.
Also update `update_repo()` to handle `embed_enabled: Some(v)`.

### 4c. Update all `Repo`/`RepoInput` construction sites in `queries.rs`

Any `Repo { ... }` or `RepoInput { ... }` literal in queries.rs must include `embed_enabled`.
Set it to `false` as default where not explicitly provided.

### 4d. Implement `get_config` and `set_config`

```rust
fn get_config(&self, key: &str) -> Result<Option<String>> {
    let conn = self.conn.lock().unwrap();
    let result = conn.query_row(
        "SELECT value FROM config WHERE key = ?1",
        params![key],
        |row| row.get(0),
    ).optional()?;
    Ok(result)
}

fn set_config(&self, key: &str, value: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "INSERT INTO config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}
```

### 4e. Implement `get_commits_without_embeddings`

Returns commits that have no entry in `commit_embed_map` (i.e., not yet embedded).
Joins `repos` to get `repo_name` and reads `author_name`/`author_time` for aux column storage:

```rust
fn get_commits_without_embeddings(&self, repo_id: i64, limit: usize) -> Result<Vec<EmbedCommit>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT c.sha, c.subject, c.body, c.patch_preview,
                c.author_name, c.author_time, r.name
         FROM commits c
         JOIN repos r ON r.repo_id = c.repo_id
         LEFT JOIN commit_embed_map m ON m.repo_id = c.repo_id AND m.sha = c.sha
         WHERE c.repo_id = ?1
           AND m.embed_id IS NULL
         ORDER BY c.author_time DESC
         LIMIT ?2",
    )?;
    let result: rusqlite::Result<Vec<EmbedCommit>> = stmt
        .query_map(params![repo_id, limit as i64], |row| {
            Ok(EmbedCommit {
                repo_id,
                sha: row.get(0)?,
                subject: row.get(1)?,
                body: row.get(2)?,
                files_changed: vec![],  // empty for perf — patch_preview captures diff content
                patch_preview: row.get(3)?,
                author_name: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                author_time: row.get(5)?,
                repo_name: row.get(6)?,
            })
        })?
        .collect();
    Ok(result?)
}
```

### 4f. Implement `store_embedding`

```rust
fn store_embedding(
    &self,
    repo_id: i64,
    sha: &str,
    subject: &str,
    author_name: &str,
    repo_name: &str,
    author_time: i64,
    patch_preview: Option<&str>,
    embedding: &[f32],
) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    // Insert into key map (idempotent)
    conn.execute(
        "INSERT OR IGNORE INTO commit_embed_map (repo_id, sha) VALUES (?1, ?2)",
        params![repo_id, sha],
    )?;
    let embed_id: i64 = conn.query_row(
        "SELECT embed_id FROM commit_embed_map WHERE repo_id = ?1 AND sha = ?2",
        params![repo_id, sha],
        |row| row.get(0),
    )?;
    // Convert Vec<f32> to bytes for sqlite-vec
    let embedding_bytes: Vec<u8> = embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    conn.execute(
        "INSERT OR REPLACE INTO commit_embeddings
             (embed_id, embedding, sha, subject, repo_name, author_name, author_time, patch_preview)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![embed_id, embedding_bytes, sha, subject, repo_name, author_name, author_time, patch_preview],
    )?;
    Ok(())
}
```

Note: sqlite-vec accepts float vectors as raw little-endian bytes blobs OR as JSON arrays,
depending on the version. Check the sqlite-vec docs. If bytes don't work, try:
`serde_json::to_string(embedding)` as the value. Use whichever the version accepts.

### 4g. Implement `search_semantic`

The `commit_embeddings` vec0 table stores auxiliary columns (`+sha`, `+subject`, `+repo_name`,
`+author_name`, `+author_time`, `+patch_preview`) so this query is **fully join-free**.
No joins to `commits`, `commit_embed_map`, or `repos` are needed.

```rust
fn search_semantic(&self, embedding: &[f32], opts: &SemanticSearchOpts) -> Result<Vec<SearchResult>> {
    let conn = self.conn.lock().unwrap();
    let limit = opts.limit.unwrap_or(10);

    let embedding_bytes: Vec<u8> = embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();

    let repos_json = opts.repos.as_ref()
        .map(|r| serde_json::to_string(r).unwrap_or_else(|_| "[]".into()))
        .unwrap_or_else(|| "[]".into());
    let since = opts.since.unwrap_or(0);

    let sql =
        "SELECT ce.repo_name, ce.sha, ce.subject, ce.author_name, ce.author_time,
                ce.patch_preview, distance
         FROM commit_embeddings ce
         WHERE ce.embedding MATCH ?1
           AND k = ?2
           AND ('' = ?3 OR ce.repo_name IN (SELECT value FROM json_each(?3)))
           AND (?4 = 0 OR ce.author_time >= ?4)
         ORDER BY distance";

    let mut stmt = conn.prepare(sql)?;
    let results: rusqlite::Result<Vec<SearchResult>> = stmt
        .query_map(params![embedding_bytes, limit as i64, repos_json, since], |row| {
            Ok(SearchResult {
                repo: row.get(0)?,
                sha: row.get(1)?,
                subject: row.get(2)?,
                author: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                date: row.get(4)?,
                matched_paths: vec![],
                patch_excerpt: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?
        .collect();
    Ok(results?)
}
```

If the sqlite-vec `MATCH` / `k =` query syntax differs from the above, consult the sqlite-vec
docs and adapt. The key pattern: embedding match with limit + optional pre-filters on aux columns.

### 4h. Update MockStore in `crates/ingest/src/lib.rs`

The MockStore in the `#[cfg(test)]` block must implement all new Store trait methods.
Add stub implementations — `unimplemented!()` for methods not used by ingest tests,
real `Ok(...)` returns for methods that need to not panic:

```rust
fn get_config(&self, _key: &str) -> Result<Option<String>> { Ok(None) }
fn set_config(&self, _key: &str, _value: &str) -> Result<()> { Ok(()) }
fn get_commits_without_embeddings(&self, _repo_id: i64, _limit: usize) -> Result<Vec<commitmux_types::EmbedCommit>> { Ok(vec![]) }
fn store_embedding(&self, _repo_id: i64, _sha: &str, _subject: &str, _author_name: &str, _repo_name: &str, _author_time: i64, _patch_preview: Option<&str>, _embedding: &[f32]) -> Result<()> { Ok(()) }
fn search_semantic(&self, _embedding: &[f32], _opts: &commitmux_types::SemanticSearchOpts) -> Result<Vec<commitmux_types::SearchResult>> { Ok(vec![]) }
```

Also update the `use commitmux_types::{...}` import to include `EmbedCommit` and `SemanticSearchOpts`.

## 5. Tests to Write

Add to `crates/store/src/lib.rs` tests:

1. `test_get_set_config` — set a config key, get it back, verify value matches.
2. `test_get_commits_without_embeddings_returns_unembedded` — add a repo and 2 commits; call `get_commits_without_embeddings`; verify both returned. Then call `store_embedding` for one; call again; verify only 1 returned.
3. `test_store_embedding_idempotent` — store the same embedding twice; verify no error.
4. `test_embed_enabled_roundtrip` — add a repo with `embed_enabled: true` via `add_repo`, retrieve via `get_repo_by_name`, assert `embed_enabled == true`.
5. `test_update_repo_embed_enabled` — add repo with `embed_enabled: false`, update with `RepoUpdate { embed_enabled: Some(true), ..Default::default() }`, retrieve, assert `embed_enabled == true`.

Note: `test_search_semantic` is NOT required here — it requires a real vec0 query which needs
actual embedding bytes. Leave that to integration testing or a future test. A basic call
returning `Ok(vec![])` on an empty DB is sufficient to verify it doesn't panic.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a
cargo build
cargo clippy -- -D warnings
cargo test -p commitmux-types -p commitmux-store -p commitmux-ingest
```

All existing tests must pass. New tests must pass.

Note: Do NOT run `cargo test` for the full workspace — `crates/mcp` tests reference `StubStore`
which will fail to compile until Wave 2B updates it. Scoped test targets only.

## 7. Constraints

- `search_semantic` implementation: if sqlite-vec ANN query syntax is different from what's
  specified, adapt the SQL — but keep the same Rust method signature.
- Do NOT add the `embed_enabled` field to `src/main.rs` `RepoInput` construction sites — that
  is Wave 2A's ownership. Document as `out_of_scope_deps` in your report.
- `row_to_repo` must use `.unwrap_or(0)` for `embed_enabled` to handle existing DB rows
  that don't have the column yet (before migration). But since Wave 0 runs first, the column
  will exist. Using `unwrap_or` is still good defensive coding.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a
git add crates/types/src/lib.rs crates/store/src/queries.rs crates/ingest/src/lib.rs
git commit -m "wave1-agent-a: add embed types, Store trait methods, SQL implementations, MockStore cascade"
```

Append to `docs/IMPL-vector-embeddings.md` under `### Agent 1A — Completion Report`:

```yaml
### Agent 1A — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-a
commit: {sha}
files_changed:
  - crates/types/src/lib.rs
  - crates/store/src/queries.rs
  - crates/ingest/src/lib.rs
files_created: []
interface_deviations: []
out_of_scope_deps:
  - "file: src/main.rs, change: add embed_enabled field to all RepoInput construction sites and --embed/--no-embed flags, reason: Wave 2A owns src/main.rs"
tests_added:
  - test_get_set_config
  - test_get_commits_without_embeddings_returns_unembedded
  - test_store_embedding_idempotent
  - test_embed_enabled_roundtrip
  - test_update_repo_embed_enabled
verification: PASS | FAIL ({command} — N/N tests)
```
