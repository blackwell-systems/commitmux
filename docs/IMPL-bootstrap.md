# commitmux — Bootstrap Architecture

### Project Architecture

**Language:** Rust (Cargo workspace)
**Project type:** MCP server (primary) + CLI admin tool
**Key concerns:** types (shared contracts), store (SQLite + FTS5), ingest (git2 walking + patch extraction), mcp (MCP server + tool handlers), cli (entry point + admin commands)
**Storage:** SQLite via `rusqlite`, FTS5 for full-text search, zstd for patch blob compression
**External integrations:** `git2` crate (libgit2), `rmcp` crate (MCP stdio server), `zstd` crate

---

### Package Structure

```
commitmux/
├── Cargo.toml              ← workspace root (members: types, store, ingest, mcp, commitmux)
├── src/
│   └── main.rs             ← CLI entry point, wires store + ingest + mcp together
└── crates/
    ├── types/              ← Wave 0: shared structs, traits, error types — no implementation
    │   ├── Cargo.toml
    │   └── src/lib.rs
    ├── store/              ← Wave 1B: SQLite + FTS5 implementation of Store trait
    │   ├── Cargo.toml
    │   └── src/lib.rs
    ├── ingest/             ← Wave 1C: git2 repo walking, patch extraction, ignore rules
    │   ├── Cargo.toml
    │   └── src/lib.rs
    └── mcp/                ← Wave 1D: rmcp stdio server, 4 MCP tool handlers
        ├── Cargo.toml
        └── src/lib.rs
```

---

### Suitability Assessment

**Verdict: SUITABLE**

Five concerns identified with clean seams: `types` defines contracts all others implement against; `store` owns all DB access and is never called by `ingest` directly (ingest receives a `&dyn Store`); `mcp` calls `store` only through the `Store` trait; `cli` wires everything at the boundary. Wave 0 is mandatory — `store`, `ingest`, and `mcp` all depend on shared structs and traits that must exist before any parallel work begins. Three agents can run fully in parallel in Wave 1 with zero file overlap.

**Estimated times:**
- Wave 0 (types): ~15 min (single agent, types only)
- Wave 1 (parallel): ~25 min (3 agents × ~25 min, fully parallel)
- Wave 2 (wiring): ~20 min (single agent)
- Total: ~60 min

Sequential baseline: ~120 min
Time savings: ~60 min (~50% faster)

---

### Interface Contracts

These are binding. Wave 1 agents implement against these exact signatures.

#### `crates/types/src/lib.rs`

```rust
use std::path::PathBuf;
use thiserror::Error;

// ── Error ─────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CommitmuxError {
    #[error("store error: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("ingest error: {0}")]
    Ingest(String),
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, CommitmuxError>;

// ── Domain types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Repo {
    pub repo_id: i64,
    pub name: String,
    pub local_path: PathBuf,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RepoInput {
    pub name: String,
    pub local_path: PathBuf,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub repo_id: i64,
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub committer_name: String,
    pub committer_email: String,
    pub author_time: i64,   // unix timestamp
    pub commit_time: i64,   // unix timestamp
    pub subject: String,
    pub body: Option<String>,
    pub parent_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Unknown,
}

impl FileStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileStatus::Added => "A",
            FileStatus::Modified => "M",
            FileStatus::Deleted => "D",
            FileStatus::Renamed => "R",
            FileStatus::Copied => "C",
            FileStatus::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommitFile {
    pub repo_id: i64,
    pub sha: String,
    pub path: String,
    pub status: FileStatus,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommitPatch {
    pub repo_id: i64,
    pub sha: String,
    pub patch_blob: Vec<u8>,    // zstd-compressed raw patch text
    pub patch_preview: String,  // first 500 chars, uncompressed, for FTS excerpt
}

#[derive(Debug, Clone)]
pub struct IngestState {
    pub repo_id: i64,
    pub last_synced_at: i64,    // unix timestamp
    pub last_synced_sha: Option<String>,
    pub last_error: Option<String>,
}

// ── Query option types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SearchOpts {
    pub since: Option<i64>,             // unix timestamp lower bound
    pub repos: Option<Vec<String>>,     // filter by repo name
    pub paths: Option<Vec<String>>,     // filter by path substring
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct TouchOpts {
    pub since: Option<i64>,
    pub repos: Option<Vec<String>>,
    pub limit: Option<usize>,
}

// ── MCP response types ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub repo: String,
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub date: i64,
    pub matched_paths: Vec<String>,
    pub patch_excerpt: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TouchResult {
    pub repo: String,
    pub sha: String,
    pub subject: String,
    pub date: i64,
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommitDetail {
    pub repo: String,
    pub sha: String,
    pub subject: String,
    pub body: Option<String>,
    pub author: String,
    pub date: i64,
    pub changed_files: Vec<CommitFileDetail>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommitFileDetail {
    pub path: String,
    pub status: String,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PatchResult {
    pub repo: String,
    pub sha: String,
    pub patch_text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IngestSummary {
    pub repo_name: String,
    pub commits_indexed: usize,
    pub commits_skipped: usize,
    pub errors: Vec<String>,
}

// ── Config ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IgnoreConfig {
    pub path_prefixes: Vec<String>,  // e.g. ["node_modules/", "vendor/", "dist/"]
    pub max_patch_bytes: usize,      // default: 1_048_576 (1MB)
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self {
            path_prefixes: vec![
                "node_modules/".into(),
                "vendor/".into(),
                "dist/".into(),
                ".git/".into(),
            ],
            max_patch_bytes: 1_048_576,
        }
    }
}

// ── Core traits ───────────────────────────────────────────────────────────

pub trait Store: Send + Sync {
    // Repo management
    fn add_repo(&self, input: &RepoInput) -> Result<Repo>;
    fn list_repos(&self) -> Result<Vec<Repo>>;
    fn get_repo_by_name(&self, name: &str) -> Result<Option<Repo>>;

    // Ingest writes
    fn upsert_commit(&self, commit: &Commit) -> Result<()>;
    fn upsert_commit_files(&self, files: &[CommitFile]) -> Result<()>;
    fn upsert_patch(&self, patch: &CommitPatch) -> Result<()>;
    fn get_ingest_state(&self, repo_id: i64) -> Result<Option<IngestState>>;
    fn update_ingest_state(&self, state: &IngestState) -> Result<()>;

    // MCP queries
    fn search(&self, query: &str, opts: &SearchOpts) -> Result<Vec<SearchResult>>;
    fn touches(&self, path_glob: &str, opts: &TouchOpts) -> Result<Vec<TouchResult>>;
    fn get_commit(&self, repo_name: &str, sha: &str) -> Result<Option<CommitDetail>>;
    fn get_patch(&self, repo_name: &str, sha: &str, max_bytes: Option<usize>) -> Result<Option<PatchResult>>;

    // Admin
    fn repo_stats(&self, repo_id: i64) -> Result<RepoStats>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepoStats {
    pub repo_name: String,
    pub commit_count: usize,
    pub last_synced_at: Option<i64>,
    pub last_synced_sha: Option<String>,
    pub last_error: Option<String>,
}

pub trait Ingester: Send + Sync {
    fn sync_repo(&self, repo: &Repo, store: &dyn Store, config: &IgnoreConfig) -> Result<IngestSummary>;
}
```

---

### File Ownership

| File/Directory | Agent | Wave | Depends On |
|----------------|-------|------|------------|
| `crates/types/` | A | 0 | nothing |
| `crates/store/` | B | 1 | types |
| `crates/ingest/` | C | 1 | types |
| `crates/mcp/` | D | 1 | types |
| `src/main.rs`, `Cargo.toml` (workspace) | E | 2 | types, store, ingest, mcp |

---

### Wave Structure

```
Wave 0: [A]        — types crate: all shared structs, traits, error types
              | gate: cargo build -p commitmux-types
Wave 1: [B][C][D]  — store, ingest, mcp (fully parallel, zero file overlap)
              | gate: cargo build --workspace + unit tests per crate
Wave 2: [E]        — workspace Cargo.toml, src/main.rs, CLI wiring, integration test
              | gate: cargo build + end-to-end: sync a repo, call MCP search
```

---

### Agent Prompts

---

#### Wave 0 — Agent A: Types Crate

**Task:** Create the `crates/types/` crate with all shared types, traits, and error definitions for commitmux. Write zero implementation — types and interfaces only.

**Codebase context:** New Rust workspace project. No existing code. The `types` crate is the foundation all other crates depend on.

**Files to create:**
- `crates/types/Cargo.toml`
- `crates/types/src/lib.rs`

**Do not touch:** anything outside `crates/types/`

**Requirements:**

1. `Cargo.toml` — name: `commitmux-types`, dependencies: `thiserror`, `serde` (features: derive), `rusqlite` (for error From impl), `git2` (for error From impl). No version pinning required, use `*` or latest.

2. `src/lib.rs` — copy the exact type definitions from the Interface Contracts section of this IMPL doc verbatim. Do not add, remove, or modify any field, method signature, or type. These are binding contracts.

3. Ensure the crate compiles cleanly on its own: `cargo build -p commitmux-types`

4. Add a `#[cfg(test)]` block with one smoke test: construct one instance of each major struct (`Repo`, `Commit`, `CommitFile`, `CommitPatch`, `IngestState`, `SearchOpts`, `IgnoreConfig`) to verify all fields are accessible.

**Completion report:** Write a `### Agent A — Completion Report` section at the bottom of `docs/IMPL-bootstrap.md` with: build status, any deviations from the interface contracts (there should be none), and the smoke test result.

---

#### Wave 1 — Agent B: Store Crate

**Task:** Implement the `Store` trait in `crates/store/` using SQLite (rusqlite) with FTS5 full-text search and zstd patch blob compression.

**Codebase context:** `crates/types/` already exists and compiles. Import it as a path dependency. Do not modify it.

**Files to create:**
- `crates/store/Cargo.toml`
- `crates/store/src/lib.rs`
- `crates/store/src/schema.rs`
- `crates/store/src/queries.rs`

**Do not touch:** anything outside `crates/store/`

**Requirements:**

1. `Cargo.toml` — name: `commitmux-store`, deps: `commitmux-types` (path), `rusqlite` (features: bundled, hooks), `zstd`, `anyhow`, `serde_json`.

2. Schema (in `schema.rs`): implement the exact tables defined in the plan:
   - `repos` — repo_id INTEGER PRIMARY KEY, name TEXT UNIQUE, local_path TEXT, remote_url TEXT, default_branch TEXT
   - `commits` — (repo_id, sha) PRIMARY KEY, author_name, author_email, committer_name, committer_email, author_time INTEGER, commit_time INTEGER, subject TEXT, body TEXT, parent_count INTEGER
   - `commit_files` — repo_id, sha, path, status TEXT, old_path TEXT; index on (repo_id, sha) and on path
   - `commit_patches` — (repo_id, sha) PRIMARY KEY, patch_blob BLOB (zstd-compressed), patch_preview TEXT
   - `ingest_state` — repo_id INTEGER PRIMARY KEY, last_synced_at INTEGER, last_synced_sha TEXT, last_error TEXT
   - `commits_fts` — FTS5 virtual table over: subject, body, patch_preview; content='commits' join with patch_preview from commit_patches via a trigger or denormalized column (simplest: include patch_preview in commits table and FTS over that)
   - Add a `PRAGMA journal_mode=WAL` on connection open.

3. `lib.rs` — `SqliteStore` struct holding a `rusqlite::Connection` (wrapped in `Mutex` for `Send + Sync`). Implement all methods of the `Store` trait.

4. Patch storage: compress with `zstd::encode_all`, decompress with `zstd::decode_all`. Store compressed bytes in `patch_blob`. `patch_preview` is the first 500 chars of the raw patch text, stored uncompressed.

5. `search` method: FTS5 query over `commits_fts`. Return `SearchResult` structs. `patch_excerpt` is `patch_preview` truncated to 300 chars.

6. `touches` method: query `commit_files` with a LIKE path match and optional filters. Return `TouchResult` structs.

7. `get_patch`: decompress `patch_blob`, truncate to `max_bytes` if provided.

8. Unit tests (in `#[cfg(test)]`):
   - `test_add_repo_and_list`: add two repos, list them, assert both present
   - `test_upsert_commit_idempotent`: upsert same commit twice, assert count == 1
   - `test_search_fts`: ingest a commit with a unique subject, search for it, assert result returned
   - `test_get_patch_roundtrip`: store a patch, retrieve it, assert text matches original

**Completion report:** Write a `### Agent B — Completion Report` section at the bottom of `docs/IMPL-bootstrap.md` with: build status, all 4 tests passing/failing, any schema deviations.

---

#### Wave 1 — Agent C: Ingest Crate

**Task:** Implement the `Ingester` trait in `crates/ingest/` using the `git2` crate to walk commits, extract patches, and apply ignore rules.

**Codebase context:** `crates/types/` already exists and compiles. Import it as a path dependency. Do not modify it.

**Files to create:**
- `crates/ingest/Cargo.toml`
- `crates/ingest/src/lib.rs`
- `crates/ingest/src/walker.rs`
- `crates/ingest/src/patch.rs`

**Do not touch:** anything outside `crates/ingest/`

**Requirements:**

1. `Cargo.toml` — name: `commitmux-ingest`, deps: `commitmux-types` (path), `git2` (features: vendored), `zstd`, `anyhow`.

2. `Git2Ingester` struct implementing `Ingester`. The `sync_repo` method:
   - Open the repo at `repo.local_path` using `git2::Repository::open`
   - Determine the branch to walk: `repo.default_branch` or fall back to `HEAD`
   - Walk commits from branch tip using `git2::Revwalk`, oldest-first
   - For each commit:
     - Extract metadata into `types::Commit`
     - Extract changed files into `Vec<types::CommitFile>` using diff against first parent (or empty tree for root commits)
     - Extract patch text using `git2::Diff::print` with `DiffFormat::Patch`
     - Apply ignore rules: skip files whose paths start with any `config.path_prefixes` prefix
     - Skip binary diffs (check `delta.is_binary()`)
     - Cap patch at `config.max_patch_bytes`; if over cap, store an empty patch_blob with a note in patch_preview
     - Compress patch text with `zstd::encode_all` → `CommitPatch`
     - Call `store.upsert_commit`, `store.upsert_commit_files`, `store.upsert_patch`
     - On error: record in `IngestSummary.errors`, continue to next commit
   - Update `IngestState` with `last_synced_at = now`, `last_synced_sha = tip sha`
   - Return `IngestSummary`

3. Handle merge commits: use first parent only for diff.

4. Handle root commits (no parents): diff against empty tree using `repo.find_tree(repo.treebuilder(None)?.write()?)` or equivalent.

5. Unit tests (in `#[cfg(test)]`):
   - `test_sync_empty_repo`: create an in-memory git repo with `git2::Repository::init` in a tempdir, sync it, assert `commits_indexed == 0`
   - `test_sync_single_commit`: init repo, create one commit with one file, sync, assert `commits_indexed == 1`
   - `test_ignore_rules`: init repo, create commit touching `node_modules/foo.js` and `src/main.rs`, sync with ignore config excluding `node_modules/`, assert `src/main.rs` in commit_files and `node_modules/foo.js` not present
   - For these tests, implement a `MockStore` (struct with `Mutex<Vec<Commit>>`) in the test module that satisfies `Store` trait with enough methods to record upserted commits/files. Unimplemented methods may panic.

**Completion report:** Write a `### Agent C — Completion Report` section at the bottom of `docs/IMPL-bootstrap.md` with: build status, all 3 tests passing/failing, any deviations from the ingestion spec.

---

#### Wave 1 — Agent D: MCP Crate

**Task:** Implement the MCP server in `crates/mcp/` using the `rmcp` crate, exposing 4 tools over stdio transport.

**Codebase context:** `crates/types/` already exists and compiles. Import it as a path dependency. Do not modify it. The MCP server will receive a `Arc<dyn Store>` at startup — it does not construct the store itself.

**Files to create:**
- `crates/mcp/Cargo.toml`
- `crates/mcp/src/lib.rs`
- `crates/mcp/src/tools.rs`

**Do not touch:** anything outside `crates/mcp/`

**Requirements:**

1. `Cargo.toml` — name: `commitmux-mcp`, deps: `commitmux-types` (path), `rmcp` (check crates.io for the latest version and correct feature flags for stdio transport), `serde` (features: derive), `serde_json`, `tokio` (features: full), `anyhow`.

2. The 4 MCP tools:

   **`commitmux_search`**
   - Input: `{ query: String, since?: i64, repos?: Vec<String>, paths?: Vec<String>, limit?: usize }`
   - Calls: `store.search(query, opts)`
   - Returns: JSON array of `SearchResult`

   **`commitmux_touches`**
   - Input: `{ path_glob: String, since?: i64, repos?: Vec<String>, limit?: usize }`
   - Calls: `store.touches(path_glob, opts)`
   - Returns: JSON array of `TouchResult`

   **`commitmux_get_commit`**
   - Input: `{ repo: String, sha: String }`
   - Calls: `store.get_commit(repo, sha)`
   - Returns: JSON `CommitDetail` or error if not found

   **`commitmux_get_patch`**
   - Input: `{ repo: String, sha: String, max_bytes?: usize }`
   - Calls: `store.get_patch(repo, sha, max_bytes)`
   - Returns: JSON `PatchResult` or error if not found

3. Register all 4 tools with the rmcp server. Use stdio transport (stdin/stdout). The server runs until the transport closes.

4. Expose a public `fn run_mcp_server(store: Arc<dyn Store + 'static>) -> anyhow::Result<()>` entry point that the CLI can call.

5. Tool errors (store errors, not-found) should return MCP error responses, not panic.

6. Unit tests (in `#[cfg(test)]`):
   - `test_search_tool_serialization`: construct a `SearchResult`, serialize to JSON, assert key fields present
   - `test_touch_tool_serialization`: same for `TouchResult`
   - Note: full MCP protocol tests require integration; unit tests cover serialization correctness only.

**Completion report:** Write a `### Agent D — Completion Report` section at the bottom of `docs/IMPL-bootstrap.md` with: build status, both serialization tests passing/failing, any rmcp API deviations or version issues encountered.

---

#### Wave 2 — Agent E: Workspace Wiring

**Task:** Create the workspace `Cargo.toml`, `src/main.rs` CLI entry point, and one end-to-end integration test that proves the system works together.

**Codebase context:** All four crates (`types`, `store`, `ingest`, `mcp`) exist and compile. Wire them together here. Do not modify any crate internals.

**Files to create/modify:**
- `Cargo.toml` (workspace root — create from scratch)
- `src/main.rs`

**Do not touch:** anything inside `crates/`

**Requirements:**

1. `Cargo.toml` (workspace root):
   ```toml
   [workspace]
   members = [".", "crates/types", "crates/store", "crates/ingest", "crates/mcp"]
   resolver = "2"

   [package]
   name = "commitmux"
   version = "0.1.0"
   edition = "2021"

   [dependencies]
   commitmux-types = { path = "crates/types" }
   commitmux-store = { path = "crates/store" }
   commitmux-ingest = { path = "crates/ingest" }
   commitmux-mcp = { path = "crates/mcp" }
   clap = { version = "4", features = ["derive"] }
   anyhow = "1"
   tokio = { version = "1", features = ["full"] }
   ```

2. `src/main.rs` — CLI with these subcommands (use `clap` derive):

   ```
   commitmux init [--db <path>]
     → create DB file + schema at path (default: ~/.commitmux/db.sqlite3)

   commitmux add-repo <path> [--name <name>] [--exclude <prefix>...]
     → register repo in DB; name defaults to directory name; exclude adds to ignore config

   commitmux sync [--repo <name>] [--db <path>]
     → sync all repos (or named repo) using Git2Ingester; print IngestSummary per repo

   commitmux show <repo> <sha> [--db <path>]
     → print CommitDetail as JSON to stdout

   commitmux status [--db <path>]
     → print RepoStats for all repos as a table (repo name, commit count, last synced)

   commitmux serve [--db <path>]
     → start MCP stdio server; blocks until stdin closes
   ```

3. DB path resolution: check `--db` flag → `COMMITMUX_DB` env var → `~/.commitmux/db.sqlite3`. Apply consistently across all subcommands.

4. `commitmux serve` must call `run_mcp_server(store)` from `commitmux-mcp`.

5. Integration test (in `src/main.rs` under `#[cfg(test)]` or as `tests/integration.rs`):
   - `test_end_to_end`:
     1. Init a DB in a tempdir
     2. Init a real git repo in a tempdir with `git2::Repository::init`, create 2 commits
     3. Call `add_repo` via the store directly
     4. Call `Git2Ingester.sync_repo` directly
     5. Call `store.search("initial commit", &SearchOpts::default())`
     6. Assert at least 1 result returned with the correct sha
   - This test proves the full ingest → store → search pipeline works.

**Completion report:** Write a `### Agent E — Completion Report` section at the bottom of `docs/IMPL-bootstrap.md` with: `cargo build` status, integration test passing/failing, any wiring issues encountered.

---

### Verification Gates

**Wave 0:**
```
cargo build -p commitmux-types
```
Must compile cleanly. No warnings required for MVP.

**Wave 1:**
```
cargo build -p commitmux-store && cargo test -p commitmux-store
cargo build -p commitmux-ingest && cargo test -p commitmux-ingest
cargo build -p commitmux-mcp && cargo test -p commitmux-mcp
```
All unit tests must pass.

**Wave 2:**
```
cargo build --workspace
cargo test --workspace
```
Full build and all tests including integration test must pass.

---

### Status

- [ ] Wave 0: Agent A — types crate (all shared structs, traits, error types)
- [ ] Wave 1: Agent B — store crate (SQLite + FTS5 + zstd)
- [ ] Wave 1: Agent C — ingest crate (git2 walking + patch extraction)
- [ ] Wave 1: Agent D — mcp crate (rmcp stdio server + 4 tools)
- [x] Wave 0: Agent A — types crate (all shared structs, traits, error types)
- [ ] Wave 1: Agent B — store crate (SQLite + FTS5 + zstd)
- [ ] Wave 1: Agent C — ingest crate (git2 walking + patch extraction)
- [ ] Wave 1: Agent D — mcp crate (rmcp stdio server + 4 tools)
- [ ] Wave 2: Agent E — workspace Cargo.toml + src/main.rs + integration test

---

### Agent A — Completion Report

**Isolation verification:** PASSED — confirmed working directory `/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-agent-A` on branch `wave0-agent-A`, confirmed in `git worktree list`.

**Files created:**

- `Cargo.toml` (worktree root) — minimal temporary workspace to enable `cargo -p commitmux-types`; out-of-scope but required by the verification gate. Members: `["crates/types"]`. Wave 2 Agent E will replace this with the real workspace Cargo.toml.
- `crates/types/Cargo.toml` — package `commitmux-types` v0.1.0, edition 2021
- `crates/types/src/lib.rs` — all types, traits, and error definitions per spec

**Build status:**

```
cargo build -p commitmux-types
Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.71s
```

Build succeeded cleanly.

**Test results:**

```
cargo test -p commitmux-types
running 2 tests
test tests::test_file_status_display ... ok
test tests::test_smoke_construct_all_types ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Both required tests pass.

**Deviations from type definitions:**

Two sets of additions vs. the base `IMPL-bootstrap.md` Interface Contracts section (which predates the agent prompt spec):

1. **`CommitmuxError::NotFound` variant added** — the agent task spec includes `NotFound(String)` in the error enum; the base contracts section omitted it. Added as specified in the task.

2. **`serde::Deserialize` added to MCP response types** — the base contracts section derives only `serde::Serialize` for `SearchResult`, `TouchResult`, `CommitDetail`, `CommitFileDetail`, `PatchResult`, and `IngestSummary`. The agent task spec adds `serde::Deserialize` to all of these. Added as specified; this makes the types usable for round-trip JSON serialization, which is harmless and strictly more capable.

3. **`RepoStats` moved before traits** — in the agent task spec `RepoStats` is declared in the Admin types section before the traits (the base contracts section placed it after the `Store` trait). Followed the agent task spec ordering; no functional difference.

**Feature flag adjustments:**

No adjustments were needed. The `rusqlite-errors` and `git2-errors` optional features compiled cleanly with both enabled (the default). The `#[cfg(feature = ...)]` attributes on `CommitmuxError::Store` and `CommitmuxError::Git` variants work correctly. No changes to `Cargo.toml` were required beyond what was specified.

**Out-of-scope files:**

- `Cargo.toml` (worktree root) — created as a minimal temporary workspace (`members = ["crates/types"]`, `resolver = "2"`) solely to satisfy the `cargo -p commitmux-types` verification gate. Wave 2 Agent E must replace this with the full workspace Cargo.toml that includes all crates.
