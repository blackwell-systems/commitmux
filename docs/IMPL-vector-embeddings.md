# IMPL: Vector Embeddings Feature

**Design spec:** `docs/vector-embeddings.md`

---

## Suitability Assessment

**Verdict: SUITABLE**

The work decomposes cleanly across 5 agents in 3 waves with fully disjoint file ownership. All
interface contracts can be specified before implementation starts — the design doc defines exact
Rust signatures for new Store trait methods, `Embedder`, `EmbedSummary`, and `SemanticSearchOpts`.
No investigation-first items exist; the codebase is well-understood and all dependencies are
identified. Build/test cycle is ~5s, agents touch 2–4 files each with non-trivial logic — SAW
provides clear speedup over sequential implementation.

Pre-implementation scan: 0 of the planned features are implemented. All agents proceed as planned.

Estimated times:
- Scout phase: ~15 min
- Agent execution: ~55 min (Wave 0: 10 min, Wave 1: 20 min parallel, Wave 2: 25 min parallel)
- Merge & verification: ~15 min (3 waves × 5 min)
- Total SAW time: ~85 min

Sequential baseline: ~120 min
Time savings: ~35 min (30% faster)

Recommendation: Clear speedup. Proceed.

---

## Known Issues

None identified. All 46+1 tests passing at time of scout. Zero clippy warnings.

---

## Critical Implementation Notes (Read Before Implementing)

### 1. MCP server is deliberately synchronous

`crates/mcp/src/lib.rs` lines 1–6 explain: tokio was intentionally excluded to avoid complexity.
The `commitmux_search_semantic` tool must embed the query string synchronously. Use:
```rust
tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()?
    .block_on(embedder.embed(query))
```
Do NOT add tokio as an async runtime to the whole MCP crate — only use it for this one blocking call.

### 2. sqlite-vec extension loading

The `commit_embeddings` virtual table uses the `sqlite-vec` extension (`vec0` module). It must be
loaded into the connection before schema DDL runs. In `SqliteStore::init()`, the order must be:
1. Load sqlite-vec extension via the `sqlite-vec` crate's load API
2. Execute `SCHEMA_SQL` (which includes `CREATE VIRTUAL TABLE IF NOT EXISTS commit_embeddings`)
3. Apply `REPO_MIGRATIONS`
4. Apply `EMBED_MIGRATIONS`

The `rusqlite` `bundled` feature is already enabled. The `sqlite-vec` crate provides static
linking. Add `sqlite-vec = "0.1"` to `crates/store/Cargo.toml` and load it with:
```rust
sqlite_vec::load(&conn).map_err(|e| CommitmuxError::Config(e.to_string()))?;
```
If the exact API differs, check the crate docs — but it will be a one-liner load function.

**Note (from Wave 0 completion):** The crate v0.1.6 does NOT expose a high-level `load()` —
only the raw FFI symbol `sqlite3_vec_init`. Wave 0 used `sqlite3_auto_extension` from
`rusqlite::ffi`, called BEFORE `Connection::open()` in a `register_vec_extension()` helper.
Wave 1A inherits this; no further changes needed for loading.

### 3. `commits` table has a composite primary key

`commits` uses `PRIMARY KEY (repo_id, sha)` — no single integer rowid. The `vec0` table requires
an integer primary key. Use a `commit_embed_map` auxiliary table:
```sql
CREATE TABLE IF NOT EXISTS commit_embed_map (
    embed_id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id  INTEGER NOT NULL,
    sha      TEXT NOT NULL,
    UNIQUE(repo_id, sha)
);
```
When storing an embedding, INSERT OR IGNORE into `commit_embed_map` first to get/assign a stable
`embed_id`, then write to `commit_embeddings`. When querying, join back through `commit_embed_map`.

### 4. Wave 1 Agent B expected build blockers

Agent B (`crates/embed`) depends on new Store trait methods added by Agent A. In Agent B's
isolated worktree those methods won't exist yet. Agent B should note this as
`out_of_scope_build_blockers` and mark `verification: FAIL (build blocked)`. This is expected
and resolved at post-Wave-1 merge.

---

## Dependency Graph

```
crates/store/src/schema.rs    ← Wave 0 (root — schema gates everything)
crates/store/src/lib.rs       ← Wave 0 (loads sqlite-vec, runs new migrations)
        |
        ├── crates/types/src/lib.rs      ← Wave 1A (new Store trait methods)
        │   crates/store/src/queries.rs  ← Wave 1A (SQL impl of new methods)
        │   crates/ingest/src/lib.rs     ← Wave 1A (MockStore cascade fix)
        │
        └── crates/embed/src/lib.rs      ← Wave 1B (new crate, blocked on 1A trait)
            crates/embed/Cargo.toml      ← Wave 1B
            Cargo.toml (workspace)       ← Wave 1B
                |
                ├── src/main.rs          ← Wave 2A (--embed flags, config subcommand)
                └── crates/mcp/src/lib.rs  ← Wave 2B (search_semantic tool)
                    crates/mcp/src/tools.rs ← Wave 2B
                    crates/mcp/Cargo.toml   ← Wave 2B (add tokio dep)
```

Cascade candidates (files outside agent scope that reference changed interfaces):
- `crates/ingest/src/lib.rs` — MockStore: **assigned to Wave 1A** to prevent cascade at merge
- `crates/mcp/src/lib.rs` — StubStore: **assigned to Wave 2B**; expected to have build blockers until Wave 1A merges

---

## Interface Contracts

### New Store trait methods (Wave 1A delivers, Wave 1B + 2A + 2B consume)

```rust
// In crates/types/src/lib.rs — Store trait:

/// Read global config key (e.g. "embed.model", "embed.endpoint")
fn get_config(&self, key: &str) -> Result<Option<String>>;

/// Write global config key
fn set_config(&self, key: &str, value: &str) -> Result<()>;

/// Returns commits that have no entry in commit_embeddings, up to `limit`.
/// Used as the backfill queue.
fn get_commits_without_embeddings(
    &self,
    repo_id: i64,
    limit: usize,
) -> Result<Vec<EmbedCommit>>;

/// Write a single embedding vector alongside commit metadata for join-free search.
/// Creates commit_embed_map entry if needed.
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

/// Semantic ANN search. Returns top-limit results ordered by cosine distance.
fn search_semantic(
    &self,
    embedding: &[f32],
    opts: &SemanticSearchOpts,
) -> Result<Vec<SearchResult>>;
```

### New types (Wave 1A delivers)

```rust
// In crates/types/src/lib.rs:

/// Lightweight commit info for embedding document construction.
/// Also carries the metadata fields written to vec0 auxiliary columns on store_embedding.
#[derive(Debug, Clone)]
pub struct EmbedCommit {
    pub repo_id: i64,
    pub sha: String,
    pub subject: String,
    pub body: Option<String>,
    pub files_changed: Vec<String>,   // paths only
    pub patch_preview: Option<String>,
    // Auxiliary column fields
    pub author_name: String,
    pub repo_name: String,
    pub author_time: i64,
}

/// Options for semantic search.
#[derive(Debug, Clone, Default)]
pub struct SemanticSearchOpts {
    pub repos: Option<Vec<String>>,   // filter by repo name
    pub since: Option<i64>,           // unix timestamp lower bound
    pub limit: Option<usize>,         // default 10
}
```

### Embedder API (Wave 1B delivers, Wave 2A + 2B consume)

```rust
// In crates/embed/src/lib.rs:

pub struct EmbedConfig {
    pub model: String,       // e.g. "nomic-embed-text"
    pub endpoint: String,    // e.g. "http://localhost:11434/v1"
}

impl EmbedConfig {
    /// Returns EmbedConfig from store config keys, falling back to defaults.
    /// Defaults: model="nomic-embed-text", endpoint="http://localhost:11434/v1"
    pub fn from_store(store: &dyn Store) -> Result<Self, anyhow::Error>;
}

pub struct Embedder {
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
    pub model: String,
}

impl Embedder {
    pub fn new(config: &EmbedConfig) -> Self;
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

pub struct EmbedSummary {
    pub embedded: usize,
    pub skipped: usize,    // already had embeddings
    pub failed: usize,
}

/// Builds the text document for a commit. Pure function, no I/O.
pub fn build_embed_doc(commit: &EmbedCommit) -> String;

/// Backfill: embeds all commits without embeddings for a repo.
/// Resilient — logs failures but does not abort on per-commit errors.
pub async fn embed_pending(
    store: &dyn Store,
    embedder: &Embedder,
    repo_id: i64,
    batch_size: usize,
) -> anyhow::Result<EmbedSummary>;
```

### `Repo` struct addition (Wave 1A delivers, cascade to `row_to_repo`)

```rust
// In crates/types/src/lib.rs — Repo struct:
pub embed_enabled: bool,

// In crates/types/src/lib.rs — RepoInput struct:
pub embed_enabled: bool,

// In crates/types/src/lib.rs — RepoUpdate struct:
pub embed_enabled: Option<bool>,
```

Note: `row_to_repo` in `crates/store/src/queries.rs` must read the new column. All `RepoInput`
construction sites in `src/main.rs` must add `embed_enabled: false` (or from CLI flag).
`src/main.rs` is owned by Wave 2A — Agent 1A should document this in out_of_scope_deps.
Actually Wave 1A owns queries.rs and types/src/lib.rs. The `row_to_repo` function and all
queries that construct `RepoInput` in `queries.rs` are within Wave 1A's scope.
But `src/main.rs` constructs `RepoInput` too — that's Wave 2A's concern. Agent 1A should
set `embed_enabled: false` as a default and document that Wave 2A sets it from the CLI flag.

---

## File Ownership

| File | Agent | Wave | Depends On |
|------|-------|------|------------|
| `crates/store/src/schema.rs` | Wave 0 | 0 | — |
| `crates/store/src/lib.rs` | Wave 0 | 0 | — |
| `crates/types/src/lib.rs` | Wave 1A | 1 | Wave 0 |
| `crates/store/src/queries.rs` | Wave 1A | 1 | Wave 0 |
| `crates/ingest/src/lib.rs` | Wave 1A | 1 | Wave 0 (cascade fix) |
| `crates/embed/Cargo.toml` | Wave 1B | 1 | Wave 1A (expected build blockers) |
| `crates/embed/src/lib.rs` | Wave 1B | 1 | Wave 1A (expected build blockers) |
| `Cargo.toml` (workspace) | Wave 1B | 1 | — |
| `src/main.rs` | Wave 2A | 2 | Wave 1A + 1B merged |
| `crates/mcp/src/lib.rs` | Wave 2B | 2 | Wave 1A + 1B merged |
| `crates/mcp/src/tools.rs` | Wave 2B | 2 | Wave 1A + 1B merged |
| `crates/mcp/Cargo.toml` | Wave 2B | 2 | — |

---

## Wave Structure

```
Wave 0:  [Schema]                      ← 1 agent, gates all downstream verification
              |
Wave 1:  [1A: Types+Store+Cascade]  [1B: Embed crate]   ← parallel; 1B has expected build blockers
              |  (both merged + full build passes)
Wave 2:  [2A: CLI]  [2B: MCP]          ← parallel
```

---

## Agent Prompts

Per-agent files:
- Wave 0: `docs/IMPL-vector-embeddings-agents/agent-wave0.md`
- Wave 1A: `docs/IMPL-vector-embeddings-agents/agent-1a.md`
- Wave 1B: `docs/IMPL-vector-embeddings-agents/agent-1b.md`
- Wave 2A: `docs/IMPL-vector-embeddings-agents/agent-2a.md`
- Wave 2B: `docs/IMPL-vector-embeddings-agents/agent-2b.md`

---

## Wave Execution Loop

After each wave completes:
1. Read completion reports from agent files. Check `interface_deviations` and `out_of_scope_deps`.
2. Merge in dependency order. For Wave 1: merge 1A first (defines types), then 1B.
3. Run full verification gate: `cargo build && cargo clippy -- -D warnings && cargo test`
4. Fix cascade issues (see Known Cascade Candidates above). Commit fixes.
5. Update status checkboxes below.
6. Launch next wave.

Wave 0 → Wave 1 gate: Schema tables exist; `sqlite-vec` loads without error; all 46 existing tests still pass.
Wave 1 → Wave 2 gate: New Store trait methods compile; `crates/embed` builds; 46+ tests pass.
Wave 2 → Done gate: Full build, clippy clean, all tests pass; smoke test `commitmux config set embed.model nomic-embed-text`.

---

## Status

- [x] Wave 0 — Schema migration (embed_enabled, config table, commit_embed_map, commit_embeddings)
- [x] Wave 1A — types/store: new Store trait methods, EmbedCommit, SemanticSearchOpts, Repo.embed_enabled, SQL implementations, MockStore cascade
- [x] Wave 1B — crates/embed: Embedder, embed_pending, build_embed_doc, EmbedSummary, EmbedConfig
- [x] Wave 2A — src/main.rs: --embed/--no-embed, config subcommand, --embed-only sync, status EMBED column
- [x] Wave 2B — crates/mcp: commitmux_search_semantic tool, SemanticSearchInput, StubStore cascade

---

### Agent Wave 0 — Completion Report

```yaml
status: complete
worktree: main branch (solo run, no worktree isolation)
commit: 1fdb5dc75ba9c5e3601a6d253473dd9d3bcc7877
files_changed:
  - crates/store/src/schema.rs
  - crates/store/src/lib.rs
  - crates/store/Cargo.toml
files_created: []
interface_deviations:
  - sqlite-vec crate does not expose a high-level load() function; only exposes
    sqlite3_vec_init() as a raw FFI symbol. Used sqlite3_auto_extension() from
    rusqlite::ffi instead. The auto_extension call must happen before the Connection
    is opened (not inside init()), so it was extracted to a register_vec_extension()
    helper called at the top of open() and open_in_memory(). Added
    #[allow(clippy::missing_transmute_annotations)] to silence clippy on the transmute.
out_of_scope_deps: []
tests_added:
  - test_config_table_exists
  - test_commit_embed_map_table_exists
  - test_embed_migrations_idempotent
verification: PASS (cargo build, cargo clippy -D warnings, cargo test — 19/19 tests)
```

### Agent 1A — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-a
commit: a82214c536a7e864129a557e33194b4a83e993de
files_changed:
  - crates/types/src/lib.rs
  - crates/store/src/queries.rs
  - crates/store/src/lib.rs
  - crates/ingest/src/lib.rs
  - src/main.rs
files_created: []
interface_deviations:
  - "INSERT OR REPLACE not supported by sqlite-vec vec0 tables; used DELETE + INSERT
    for idempotent store_embedding. Semantics are identical."
out_of_scope_deps:
  - "file: src/main.rs, change: add embed_enabled field to all RepoInput construction
    sites and --embed/--no-embed flags, reason: Wave 2A owns src/main.rs. Minimal
    cascade fix applied (embed_enabled: false defaults + embed_enabled: None for
    RepoUpdate) to keep cargo build passing; real flag wiring deferred to Wave 2A."
tests_added:
  - test_get_set_config
  - test_get_commits_without_embeddings_returns_unembedded
  - test_store_embedding_idempotent
  - test_embed_enabled_roundtrip
  - test_update_repo_embed_enabled
verification: PASS (cargo build, cargo clippy -D warnings, cargo test -p commitmux-types -p commitmux-store -p commitmux-ingest — 37/37 tests)
```

### Agent 1B — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-b
commit: d75bc2c892954ea3dcd60498e55572b46936b7aa
files_changed:
  - Cargo.toml
files_created:
  - crates/embed/Cargo.toml
  - crates/embed/src/lib.rs
interface_deviations:
  - async-openai 0.33 uses `async_openai::types::embeddings::CreateEmbeddingRequestArgs`
    not `async_openai::types::CreateEmbeddingRequestArgs` as the agent prompt suggested;
    corrected to the 0.33 actual path.
  - async-openai 0.33 requires the `embedding` feature flag to enable Client, config module,
    and CreateEmbeddingRequestArgs builder; added `features = ["embedding"]` to Cargo.toml.
out_of_scope_build_blockers:
  - "EmbedCommit type not found — owned by Wave 1A (crates/types/src/lib.rs)"
  - "Store::get_commits_without_embeddings not found — owned by Wave 1A"
  - "Store::store_embedding not found — owned by Wave 1A"
  - "Store::get_config not found — owned by Wave 1A"
  - "SemanticSearchOpts type not found in tests — owned by Wave 1A (crates/types/src/lib.rs)"
  - "E0282 type annotations needed (cascade from missing EmbedCommit)"
tests_added:
  - test_build_embed_doc_subject_only
  - test_build_embed_doc_full
  - test_build_embed_doc_truncates_patch
  - test_embed_config_defaults
verification: FAIL (build blocked on Wave 1A: missing EmbedCommit and new Store methods)
```

### Agent 2A — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave2-agent-a
commit: 6b8a71b
files_changed:
  - src/main.rs
  - Cargo.toml
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_config_set_get_roundtrip
  - test_embed_sync_tip_logic
verification: PASS (cargo build, cargo clippy -D warnings, cargo test -p commitmux — 12/12 unit + 1/1 integration tests)
```

### Agent 2B — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave2-agent-b
commit: 04f3611
files_changed:
  - crates/mcp/src/lib.rs
  - crates/mcp/src/tools.rs
  - crates/mcp/Cargo.toml
files_created: []
interface_deviations:
  - call_search_semantic returns Result<String, String> (matching the existing tool method
    pattern used by handle_tools_call) rather than anyhow::Result<serde_json::Value> as
    specified in the agent prompt. The content envelope wrapping is handled by handle_tools_call
    uniformly for all tools. Semantics are identical.
out_of_scope_deps: []
tests_added:
  - test_tools_list_includes_semantic
  - test_search_semantic_missing_query
verification: PASS (cargo build, cargo clippy -D warnings, cargo test -p commitmux-mcp — 12/12 tests)
```
