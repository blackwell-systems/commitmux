# IMPL: Semantic Memory Search

```
feature_slug: memory-search
repo: commitmux
language: rust
test_command: cargo test
lint_command: cargo clippy -- -D warnings
```

---

## Suitability Assessment

**Verdict: SUITABLE**

The work decomposes into 4 agents across 2 waves with fully disjoint file ownership. The
feature extends the existing embedding infrastructure (Embedder, sqlite-vec, vec0 tables) with
a new domain (memory files) while reusing 100% of the embedding pipeline. Interface contracts
are precise: new Store trait methods, new types, new schema DDL, new CLI command, new MCP tool.
No investigation-first items; the codebase embedding pattern is well-established and this
feature mirrors it exactly.

Pre-implementation scan: 0 of the planned features are implemented. All agents proceed as planned.

Estimated times:
- Scout phase: ~15 min
- Agent execution: ~50 min (Wave 1: 25 min parallel, Wave 2: 25 min parallel)
- Merge & verification: ~10 min (2 waves x 5 min)
- Total SAW time: ~75 min

Sequential baseline: ~110 min
Time savings: ~35 min (32% faster)

Recommendation: Clear speedup. Proceed.

---

## Known Issues

None identified. Existing tests pass. Zero clippy warnings after recent `style: apply rustfmt` commit.

---

## Critical Implementation Notes (Read Before Implementing)

### 1. MCP server is deliberately synchronous

`crates/mcp/src/lib.rs` uses manual stdio JSON-RPC with no async runtime. The existing
`commitmux_search_semantic` tool shows the pattern: use a one-shot tokio runtime for the
embedding call only:
```rust
tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()?
    .block_on(embedder.embed(&query))
```
Do NOT make the MCP crate async.

### 2. sqlite-vec extension is already loaded

`SqliteStore::register_vec_extension()` calls `sqlite3_auto_extension` before any connection
opens. New `memory_embeddings` vec0 table DDL will work automatically -- no additional extension
loading needed.

### 3. Memory table uses its own embed_map, separate from commit_embed_map

The existing `commit_embed_map` maps embed_id -> (repo_id, sha) for commits. Memory documents
need their own mapping: `memory_embed_map` maps embed_id -> doc_id. This keeps the two domains
cleanly separated and avoids id conflicts in the vec0 virtual table.

### 4. Reuse Embedder and build_embed_doc pattern

The `commitmux_embed::Embedder` and `commitmux_embed::EmbedConfig` are reused directly.
Memory documents use a simpler document format than commits (just the markdown content),
so a new `build_memory_embed_doc()` function is needed in the embed crate.

### 5. Store trait grows -- all mock/stub impls must be updated

Adding new methods to the `Store` trait requires updating:
- `crates/embed/src/lib.rs` NullStore (test mock)
- `crates/mcp/src/lib.rs` StubStore and StubStoreWithRepos (test mocks)

These cascade updates are assigned to specific agents to prevent merge conflicts.

### 6. Incremental ingestion via mtime tracking

The `memory_docs` table stores `file_mtime INTEGER` for each ingested file. On re-ingest,
the CLI checks the file's current mtime against the stored value and only re-embeds if changed.
This avoids expensive re-embedding on every sync.

---

## Dependency Graph

```
Wave 1 (parallel, no interdependency):
  ├── Agent 1A: Schema + Types + Store trait + Store impl
  │     crates/types/src/lib.rs        (new types, new Store trait methods)
  │     crates/store/src/schema.rs     (new DDL tables)
  │     crates/store/src/queries.rs    (new Store impl methods)
  │
  └── Agent 1B: Embed crate extensions
        crates/embed/src/lib.rs        (build_memory_embed_doc, embed_memory_pending)

Wave 2 (parallel, depends on Wave 1 merge):
  ├── Agent 2A: CLI command + mock cascade
  │     src/main.rs                    (IngestMemory command)
  │     crates/embed/src/lib.rs        (NullStore cascade -- add stub methods)
  │
  └── Agent 2B: MCP tool + mock cascade
        crates/mcp/src/lib.rs          (search_memory tool handler + StubStore cascade)
        crates/mcp/src/tools.rs        (SearchMemoryInput type)
```

Cascade candidates (files outside agent scope that reference changed interfaces):
- `crates/embed/src/lib.rs` NullStore: **assigned to Wave 2A**
- `crates/mcp/src/lib.rs` StubStore + StubStoreWithRepos: **assigned to Wave 2B**

---

## Interface Contracts

### New types (crates/types/src/lib.rs)

```rust
/// Source classification for memory documents.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MemorySourceType {
    SessionSummary,
    Task,
    Blocker,
    MemoryFile,
    Decision,
}

impl MemorySourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionSummary => "session_summary",
            Self::Task => "task",
            Self::Blocker => "blocker",
            Self::MemoryFile => "memory_file",
            Self::Decision => "decision",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "session_summary" => Self::SessionSummary,
            "task" => Self::Task,
            "blocker" => Self::Blocker,
            "decision" => Self::Decision,
            _ => Self::MemoryFile,
        }
    }
}

/// A memory document stored in the index.
#[derive(Debug, Clone)]
pub struct MemoryDoc {
    pub doc_id: i64,
    pub source: String,       // file path or identifier
    pub project: String,      // project name extracted from path
    pub source_type: MemorySourceType,
    pub content: String,
    pub file_mtime: i64,      // unix timestamp of file modification time
    pub created_at: i64,      // unix timestamp of when indexed
}

/// Input for inserting/updating a memory document.
#[derive(Debug, Clone)]
pub struct MemoryDocInput {
    pub source: String,
    pub project: String,
    pub source_type: MemorySourceType,
    pub content: String,
    pub file_mtime: i64,
}

/// Result type for memory search.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryMatch {
    pub doc_id: i64,
    pub source: String,
    pub project: String,
    pub source_type: String,
    pub content: String,
    pub score: f32,
}

/// Options for memory search.
#[derive(Debug, Clone, Default)]
pub struct MemorySearchOpts {
    pub project: Option<String>,
    pub source_type: Option<String>,
    pub limit: Option<usize>,    // default 10
}
```

### New Store trait methods (crates/types/src/lib.rs)

Add to `pub trait Store`:
```rust
    // Memory document support
    fn upsert_memory_doc(&self, input: &MemoryDocInput) -> Result<MemoryDoc>;
    fn get_memory_doc_by_source(&self, source: &str) -> Result<Option<MemoryDoc>>;
    fn get_memory_docs_without_embeddings(&self, limit: usize) -> Result<Vec<MemoryDoc>>;
    fn store_memory_embedding(&self, doc_id: i64, embedding: &[f32]) -> Result<()>;
    fn search_memory(&self, embedding: &[f32], opts: &MemorySearchOpts) -> Result<Vec<MemoryMatch>>;
```

### New schema DDL (crates/store/src/schema.rs)

Append to `SCHEMA_SQL`:
```sql
CREATE TABLE IF NOT EXISTS memory_docs (
    doc_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    source       TEXT NOT NULL UNIQUE,
    project      TEXT NOT NULL,
    source_type  TEXT NOT NULL DEFAULT 'memory_file',
    content      TEXT NOT NULL,
    file_mtime   INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_memory_docs_project ON memory_docs(project);

CREATE TABLE IF NOT EXISTS memory_embed_map (
    embed_id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id   INTEGER NOT NULL UNIQUE
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_embeddings USING vec0(
    embed_id   INTEGER PRIMARY KEY,
    embedding  FLOAT[768],
    +doc_id    INTEGER,
    +source    TEXT,
    +project   TEXT,
    +source_type TEXT
);
```

### New embed functions (crates/embed/src/lib.rs)

```rust
/// Builds the embedding document for a memory file.
/// Format: "# {project}\n\n{content truncated to 3000 chars}"
pub fn build_memory_embed_doc(doc: &MemoryDoc) -> String;

/// Embeds all memory docs without embeddings.
/// Reuses the same Embedder and batch pattern as embed_pending.
pub async fn embed_memory_pending(
    store: &dyn Store,
    embedder: &Embedder,
    batch_size: usize,
) -> anyhow::Result<EmbedSummary>;
```

### New MCP tool input (crates/mcp/src/tools.rs)

```rust
#[derive(Debug, Deserialize)]
pub struct SearchMemoryInput {
    pub query: String,
    pub project: Option<String>,
    pub source_type: Option<String>,  // "session_summary"|"task"|"blocker"|"memory_file"|"decision"
    pub limit: Option<usize>,
}
```

### New CLI command (src/main.rs)

```rust
#[command(about = "Ingest claudewatch memory files for semantic search")]
IngestMemory {
    #[arg(
        long = "claude-home",
        help = "Path to .claude directory (default: ~/.claude)"
    )]
    claude_home: Option<PathBuf>,
    #[arg(
        long,
        help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
    )]
    db: Option<PathBuf>,
},
```

---

## Disjoint File Ownership

| File | Wave 1A | Wave 1B | Wave 2A | Wave 2B |
|------|---------|---------|---------|---------|
| `crates/types/src/lib.rs` | OWN | | | |
| `crates/store/src/schema.rs` | OWN | | | |
| `crates/store/src/queries.rs` | OWN | | | |
| `crates/embed/src/lib.rs` | | OWN | cascade | |
| `src/main.rs` | | | OWN | |
| `crates/mcp/src/lib.rs` | | | | OWN |
| `crates/mcp/src/tools.rs` | | | | OWN |

Note: `crates/embed/src/lib.rs` is owned by 1B in Wave 1 and receives cascade updates from 2A
in Wave 2. These are in different waves so there is no conflict -- 2A runs after 1B merges.

---

## Scaffolds

No scaffold step is needed. Wave 1A adds all shared types and trait methods. Wave 1B depends
only on existing types (`MemoryDoc` from types, `Embedder`/`EmbedConfig`/`EmbedSummary` from
embed) and the Store trait. Since 1A and 1B are in the same wave:

- Agent 1B will have build blockers on the new Store trait methods (`get_memory_docs_without_embeddings`,
  `store_memory_embedding`) because 1A's changes won't be in 1B's worktree. This is expected and
  documented as `out_of_scope_build_blockers`. Resolution: post-Wave-1 merge.
- Agent 1B should use the existing `MemoryDoc` type definition from the interface contract above,
  noting that compilation will fail until merge.

---

## Agent Prompts

### Wave 1, Agent A: Schema + Types + Store Implementation

```
agent_name: memory-schema-types-store
wave: 1
depends_on: []
owned_files:
  - crates/types/src/lib.rs
  - crates/store/src/schema.rs
  - crates/store/src/queries.rs

instructions: |
  Add memory document support to the commitmux type system, schema, and store implementation.

  1. **crates/types/src/lib.rs**: Add these new types after the existing `SemanticSearchOpts`:
     - `MemorySourceType` enum (SessionSummary, Task, Blocker, MemoryFile, Decision) with
       `as_str()` and `from_str()` methods, deriving Debug, Clone, PartialEq, Serialize, Deserialize
     - `MemoryDoc` struct with fields: doc_id (i64), source (String), project (String),
       source_type (MemorySourceType), content (String), file_mtime (i64), created_at (i64)
     - `MemoryDocInput` struct with fields: source, project, source_type, content, file_mtime
     - `MemoryMatch` struct (Serialize, Deserialize) with fields: doc_id (i64), source (String),
       project (String), source_type (String), content (String), score (f32)
     - `MemorySearchOpts` struct (Default) with fields: project (Option<String>),
       source_type (Option<String>), limit (Option<usize>)
     - Add 5 new methods to `pub trait Store` (see interface contracts in IMPL doc):
       `upsert_memory_doc`, `get_memory_doc_by_source`, `get_memory_docs_without_embeddings`,
       `store_memory_embedding`, `search_memory`

  2. **crates/store/src/schema.rs**: Append to SCHEMA_SQL (before the closing `"#`):
     - `memory_docs` table (doc_id PK AUTOINCREMENT, source TEXT UNIQUE, project TEXT,
       source_type TEXT DEFAULT 'memory_file', content TEXT, file_mtime INTEGER DEFAULT 0,
       created_at INTEGER DEFAULT 0)
     - Index `idx_memory_docs_project` on memory_docs(project)
     - `memory_embed_map` table (embed_id PK AUTOINCREMENT, doc_id INTEGER UNIQUE)
     - `memory_embeddings` vec0 virtual table with: embed_id INTEGER PRIMARY KEY,
       embedding FLOAT[768], +doc_id INTEGER, +source TEXT, +project TEXT, +source_type TEXT

  3. **crates/store/src/queries.rs**: Implement the 5 new Store trait methods on SqliteStore:
     - `upsert_memory_doc`: INSERT OR REPLACE into memory_docs. Set created_at to current unix
       timestamp on insert. Return the MemoryDoc with the doc_id from last_insert_rowid or by
       querying back by source.
     - `get_memory_doc_by_source`: SELECT from memory_docs WHERE source = ?1.
     - `get_memory_docs_without_embeddings`: SELECT from memory_docs LEFT JOIN memory_embed_map
       WHERE memory_embed_map.doc_id IS NULL, LIMIT ?1.
     - `store_memory_embedding`: INSERT OR IGNORE into memory_embed_map to get embed_id, then
       INSERT OR REPLACE into memory_embeddings with the vector and auxiliary columns (+doc_id,
       +source, +project, +source_type). Follow the exact same pattern as `store_embedding`
       for commits.
     - `search_memory`: Query memory_embeddings with vec0 kNN syntax:
       `SELECT embed_id, distance, doc_id, source, project, source_type FROM memory_embeddings
       WHERE embedding MATCH ?1 AND k = ?2`. Join back to memory_docs to get content.
       Apply optional project and source_type filters. Return Vec<MemoryMatch> with
       score = distance.

  4. **Tests**: Add tests to crates/store/src/lib.rs (tests module):
     - test_upsert_memory_doc_roundtrip: insert, query back by source, verify fields
     - test_memory_doc_incremental: insert, then upsert with new mtime, verify updated
     - test_get_memory_docs_without_embeddings: insert 2 docs, embed 1, verify only 1 returned
     - test_store_memory_embedding_idempotent: store same embedding twice, no error

interface_contract: |
  New types: MemorySourceType, MemoryDoc, MemoryDocInput, MemoryMatch, MemorySearchOpts
  New Store trait methods: upsert_memory_doc, get_memory_doc_by_source,
    get_memory_docs_without_embeddings, store_memory_embedding, search_memory
  See IMPL doc "Interface Contracts" section for exact signatures.

verification: |
  cargo test -p commitmux-types
  cargo test -p commitmux-store
  cargo clippy -p commitmux-types -p commitmux-store -- -D warnings

out_of_scope_build_blockers: |
  Other crates that impl Store (NullStore in crates/embed, StubStore in crates/mcp) will fail
  to compile because they lack the 5 new trait methods. This is expected -- those cascade
  updates are handled by Wave 2 agents.

rollback: |
  Revert all changes to crates/types/src/lib.rs, crates/store/src/schema.rs,
  crates/store/src/queries.rs, and crates/store/src/lib.rs (test additions).
```

### Agent A - Completion Report

**Status: COMPLETE**
**Commit:** `ee23273` on branch `worktree-agent-a63a6142`

**What was implemented:**

1. **crates/types/src/lib.rs** -- Added `MemorySourceType` enum (with `as_str()` and `from_str()`), `MemoryDoc`, `MemoryDocInput`, `MemoryMatch`, `MemorySearchOpts` types. Extended `Store` trait with 5 new methods: `upsert_memory_doc`, `get_memory_doc_by_source`, `get_memory_docs_without_embeddings`, `store_memory_embedding`, `search_memory`.

2. **crates/store/src/schema.rs** -- Appended `memory_docs` table, `idx_memory_docs_project` index, `memory_embed_map` table, and `memory_embeddings` vec0 virtual table to `SCHEMA_SQL`.

3. **crates/store/src/queries.rs** -- Implemented all 5 Store trait methods on `SqliteStore`, following the existing `store_embedding`/`search_semantic` patterns (embed_map + vec0 delete-then-insert for idempotency, kNN with post-filter).

4. **crates/store/src/lib.rs** -- Added 4 tests: `test_upsert_memory_doc_roundtrip`, `test_memory_doc_incremental`, `test_get_memory_docs_without_embeddings`, `test_store_memory_embedding_idempotent`.

**Verification:**
- `cargo test -p commitmux-types` -- 5 passed
- `cargo test -p commitmux-store` -- 33 passed
- `cargo clippy -p commitmux-types -p commitmux-store -- -D warnings` -- 0 warnings

**Notes:**
- Added `#[allow(clippy::should_implement_trait)]` on `MemorySourceType::from_str()` since the method intentionally does not implement `std::str::FromStr` (it is infallible, defaulting to `MemoryFile`).
- `INSERT OR REPLACE` on `memory_docs` (keyed by UNIQUE `source`) causes AUTOINCREMENT `doc_id` to change on upsert. This is fine -- the embed pipeline re-queries doc_id after upsert.

### Wave 1, Agent B: Embed Crate Extensions

```
agent_name: memory-embed-functions
wave: 1
depends_on: []
owned_files:
  - crates/embed/src/lib.rs

instructions: |
  Add memory document embedding support to the embed crate. This reuses the existing Embedder
  and EmbedConfig but adds memory-specific document building and batch embedding functions.

  1. **build_memory_embed_doc**: Add a public function after `build_embed_doc`:
     ```rust
     pub fn build_memory_embed_doc(project: &str, content: &str) -> String {
         let mut doc = format!("# {}\n\n", project);
         // Truncate content to ~3000 chars to stay within embedding model context
         if content.len() > 3000 {
             doc.push_str(&content[..3000]);
         } else {
             doc.push_str(content);
         }
         doc
     }
     ```
     Note: Takes project and content as &str rather than MemoryDoc to avoid depending on
     types that won't compile in this worktree yet. The caller (Wave 2A) will destructure
     MemoryDoc and pass the fields.

  2. **embed_memory_pending**: Add a public async function after `embed_pending`:
     ```rust
     pub async fn embed_memory_pending(
         store: &dyn Store,
         embedder: &Embedder,
         batch_size: usize,
     ) -> anyhow::Result<EmbedSummary>
     ```
     Implementation pattern -- mirror `embed_pending` exactly:
     - Loop: fetch batch via `store.get_memory_docs_without_embeddings(batch_size)`
     - For each doc: call `build_memory_embed_doc(&doc.project, &doc.content)`, then
       `embedder.embed(&doc_text).await`
     - On success: `store.store_memory_embedding(doc.doc_id, &embedding)`
     - On connection error: return Err immediately (use `is_connection_error`)
     - On other error: increment failed, continue
     - Break when batch is empty

     IMPORTANT: This function calls Store trait methods that don't exist in this worktree
     (get_memory_docs_without_embeddings, store_memory_embedding). This is expected --
     document as out_of_scope_build_blockers.

  3. **Tests**: Add unit tests:
     - test_build_memory_embed_doc_basic: verify output starts with "# project\n\n"
     - test_build_memory_embed_doc_truncates: verify content >3000 chars is truncated to 3000
     - test_build_memory_embed_doc_short: verify short content is not truncated

interface_contract: |
  pub fn build_memory_embed_doc(project: &str, content: &str) -> String
  pub async fn embed_memory_pending(store: &dyn Store, embedder: &Embedder, batch_size: usize) -> anyhow::Result<EmbedSummary>

verification: |
  FAIL (expected). Build will fail because Store trait lacks the 5 new memory methods.
  Unit tests for build_memory_embed_doc should pass if isolated (they don't touch Store).
  Run: cargo test -p commitmux-embed -- build_memory_embed_doc

out_of_scope_build_blockers: |
  - Store trait in crates/types/src/lib.rs lacks: get_memory_docs_without_embeddings,
    store_memory_embedding (added by Wave 1A)
  - NullStore in this file lacks the 5 new trait methods (cascade handled by Wave 2A)
  Resolution: post-Wave-1 merge with Agent 1A.

rollback: |
  Revert changes to crates/embed/src/lib.rs.
```

### Wave 2, Agent A: CLI Command + Embed Mock Cascade

```
agent_name: memory-cli-ingest
wave: 2
depends_on: [memory-schema-types-store, memory-embed-functions]
owned_files:
  - src/main.rs

instructions: |
  Add the `ingest-memory` CLI command and update the NullStore mock in the embed crate.

  **IMPORTANT**: After Wave 1 merge, the embed crate's NullStore will be missing the 5 new
  Store trait methods. You must add stub implementations to NullStore before the CLI work,
  otherwise `cargo test` won't compile.

  UPDATE: Since `crates/embed/src/lib.rs` is owned by Wave 1B, the cascade fix for NullStore
  must happen here since 2A depends_on both 1A and 1B being merged. However, 2A does NOT own
  `crates/embed/src/lib.rs`. The Orchestrator should apply the NullStore cascade as a merge
  fixup after Wave 1, OR reassign this file. To keep ownership clean: the Orchestrator will
  add the 5 stub methods to NullStore during the Wave 1 merge step. Agent 2A only owns
  `src/main.rs`.

  1. **src/main.rs**: Add `IngestMemory` variant to the `Commands` enum:
     ```rust
     #[command(about = "Ingest claudewatch memory files for semantic search")]
     IngestMemory {
         #[arg(long = "claude-home", help = "Path to .claude directory (default: ~/.claude)")]
         claude_home: Option<PathBuf>,
         #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
         db: Option<PathBuf>,
     },
     ```

  2. **src/main.rs**: Add the handler in the main match block:
     ```rust
     Commands::IngestMemory { claude_home, db } => {
         let db_path = resolve_db_path(db);
         if !db_path.exists() {
             anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
         }
         let store = SqliteStore::open(&db_path)?;

         let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
         let claude_dir = claude_home.unwrap_or_else(|| PathBuf::from(&home).join(".claude"));

         if !claude_dir.exists() {
             anyhow::bail!("Claude directory not found at {}", claude_dir.display());
         }

         // Scan projects/*/memory/*.md
         let projects_dir = claude_dir.join("projects");
         if !projects_dir.exists() {
             println!("No projects directory found at {}", projects_dir.display());
             return Ok(());
         }

         let mut total_ingested = 0usize;
         let mut total_skipped = 0usize;

         for project_entry in std::fs::read_dir(&projects_dir)? {
             let project_entry = project_entry?;
             let memory_dir = project_entry.path().join("memory");
             if !memory_dir.is_dir() { continue; }

             // Extract project name from directory name
             let project_name = project_entry.file_name().to_string_lossy().to_string();

             for file_entry in std::fs::read_dir(&memory_dir)? {
                 let file_entry = file_entry?;
                 let path = file_entry.path();
                 if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }

                 let metadata = std::fs::metadata(&path)?;
                 let file_mtime = metadata.modified()
                     .unwrap_or(std::time::UNIX_EPOCH)
                     .duration_since(std::time::UNIX_EPOCH)
                     .unwrap_or_default()
                     .as_secs() as i64;

                 let source = path.to_string_lossy().to_string();

                 // Check if already indexed with same mtime
                 if let Ok(Some(existing)) = store.get_memory_doc_by_source(&source) {
                     if existing.file_mtime >= file_mtime {
                         total_skipped += 1;
                         continue;
                     }
                 }

                 let content = std::fs::read_to_string(&path)?;
                 let input = commitmux_types::MemoryDocInput {
                     source,
                     project: project_name.clone(),
                     source_type: commitmux_types::MemorySourceType::MemoryFile,
                     content,
                     file_mtime,
                 };
                 store.upsert_memory_doc(&input)?;
                 total_ingested += 1;
             }
         }

         println!("Ingested {} memory files ({} unchanged, skipped)", total_ingested, total_skipped);

         // Embed any docs without embeddings
         match commitmux_embed::EmbedConfig::from_store(&store) {
             Ok(config) => {
                 let embedder = commitmux_embed::Embedder::new(&config);
                 let rt = tokio::runtime::Builder::new_current_thread()
                     .enable_all()
                     .build()
                     .expect("tokio runtime");
                 match rt.block_on(commitmux_embed::embed_memory_pending(&store, &embedder, 50)) {
                     Ok(summary) => {
                         if summary.embedded > 0 || summary.failed > 0 {
                             println!("Embedded {} memory docs ({} failed)", summary.embedded, summary.failed);
                         }
                     }
                     Err(e) => eprintln!("Warning: embedding failed: {e}"),
                 }
             }
             Err(e) => eprintln!("Warning: embed config error: {e}"),
         }
     }
     ```

  3. **Tests**: Add to the existing test module in src/main.rs:
     - test_ingest_memory_command_parses: verify clap can parse `ingest-memory` with and
       without `--claude-home` flag

interface_contract: |
  CLI: `commitmux ingest-memory [--claude-home PATH] [--db PATH]`
  Scans ~/.claude/projects/*/memory/*.md (or custom --claude-home)
  Incremental: skips files whose mtime hasn't changed
  After ingestion, runs embed_memory_pending to generate embeddings

verification: |
  cargo test -p commitmux
  cargo clippy -- -D warnings
  cargo build

out_of_scope_build_blockers: |
  None after Wave 1 merge (all dependencies resolved).

rollback: |
  Revert changes to src/main.rs.
```

### Wave 2, Agent B: MCP Tool + Mock Cascade

```
agent_name: memory-mcp-tool
wave: 2
depends_on: [memory-schema-types-store, memory-embed-functions]
owned_files:
  - crates/mcp/src/lib.rs
  - crates/mcp/src/tools.rs

instructions: |
  Add the `commitmux_search_memory` MCP tool and update all mock Store impls in the MCP crate.

  1. **crates/mcp/src/tools.rs**: Add `SearchMemoryInput` struct:
     ```rust
     #[derive(Debug, Deserialize)]
     pub struct SearchMemoryInput {
         pub query: String,
         pub project: Option<String>,
         pub source_type: Option<String>,
         pub limit: Option<usize>,
     }
     ```

  2. **crates/mcp/src/lib.rs**: Update imports at top to include new types:
     - Add `SearchMemoryInput` to the tools import
     - Add `MemorySearchOpts` to the commitmux_types import

  3. **crates/mcp/src/lib.rs**: Add tool to `handle_tools_list`:
     Append a new tool entry to the tools array:
     ```json
     {
         "name": "commitmux_search_memory",
         "description": "Semantic search over indexed claudewatch memory files (session summaries, tasks, blockers, decisions). Use for finding prior context, decisions, and known issues across projects.",
         "inputSchema": {
             "type": "object",
             "properties": {
                 "query": { "type": "string", "description": "Natural language description of what you're looking for" },
                 "project": { "type": "string", "description": "Filter by project name (optional)" },
                 "source_type": { "type": "string", "description": "Filter by source type: session_summary, task, blocker, memory_file, decision (optional)" },
                 "limit": { "type": "integer", "description": "Max results (default 10)" }
             },
             "required": ["query"]
         }
     }
     ```

  4. **crates/mcp/src/lib.rs**: Add dispatch in `handle_tools_call`:
     Add to the match:
     ```rust
     "commitmux_search_memory" => self.call_search_memory(&arguments),
     ```

  5. **crates/mcp/src/lib.rs**: Add `call_search_memory` method to McpServer:
     Follow the same pattern as `call_search_semantic`:
     - Deserialize SearchMemoryInput
     - Validate: empty query returns error, limit=0 returns error
     - Build EmbedConfig from store, create Embedder
     - Embed query with one-shot tokio runtime (same pattern as call_search_semantic)
     - Build MemorySearchOpts from input fields
     - Call store.search_memory(&embedding, &opts)
     - Serialize results to JSON

  6. **crates/mcp/src/lib.rs**: Update StubStore and StubStoreWithRepos:
     Add the 5 new Store trait methods to both mock impls with stub implementations:
     - upsert_memory_doc: unimplemented!()
     - get_memory_doc_by_source: unimplemented!()
     - get_memory_docs_without_embeddings: Ok(vec![])
     - store_memory_embedding: Ok(())
     - search_memory: Ok(vec![])

  7. **Tests**: Add to the existing test module:
     - test_tools_list_includes_search_memory: verify commitmux_search_memory appears in tools/list
     - test_search_memory_rejects_empty_query: empty query returns isError true
     - test_search_memory_rejects_limit_zero: limit=0 returns isError true
     - Update test_tools_list_response: change expected tool count from 6 to 7

interface_contract: |
  MCP tool: commitmux_search_memory(query, project?, source_type?, limit?)
  Returns: JSON array of MemoryMatch objects

verification: |
  cargo test -p commitmux-mcp
  cargo clippy -p commitmux-mcp -- -D warnings

out_of_scope_build_blockers: |
  None after Wave 1 merge.

rollback: |
  Revert changes to crates/mcp/src/lib.rs and crates/mcp/src/tools.rs.
```

---

## Orchestrator Merge Notes

### Post-Wave-1 Merge Fixup

After merging Wave 1A and 1B, the NullStore in `crates/embed/src/lib.rs` will be missing the
5 new Store trait methods. The Orchestrator must add these stub implementations before running
verification:

```rust
// Add to NullStore impl in crates/embed/src/lib.rs
fn upsert_memory_doc(&self, _input: &commitmux_types::MemoryDocInput) -> Result<commitmux_types::MemoryDoc> {
    unimplemented!()
}
fn get_memory_doc_by_source(&self, _source: &str) -> Result<Option<commitmux_types::MemoryDoc>> {
    unimplemented!()
}
fn get_memory_docs_without_embeddings(&self, _limit: usize) -> Result<Vec<commitmux_types::MemoryDoc>> {
    Ok(vec![])
}
fn store_memory_embedding(&self, _doc_id: i64, _embedding: &[f32]) -> Result<()> {
    Ok(())
}
fn search_memory(&self, _embedding: &[f32], _opts: &commitmux_types::MemorySearchOpts) -> Result<Vec<commitmux_types::MemoryMatch>> {
    Ok(vec![])
}
```

### Agent B - Completion Report

**Status**: COMPLETE
**Commit**: d7faf37 (`feat(embed): add memory document embedding support`)
**Branch**: `worktree-agent-a377f5a5`
**File modified**: `crates/embed/src/lib.rs`

**What was implemented**:
1. `build_memory_embed_doc(project: &str, content: &str) -> String` — pure function that formats memory content as `# {project}\n\n{content}` with 3000-char truncation
2. `embed_memory_pending(store, embedder, batch_size) -> Result<EmbedSummary>` — async batch embedding function mirroring `embed_pending` exactly, using `get_memory_docs_without_embeddings` and `store_memory_embedding` Store trait methods
3. Three unit tests: `test_build_memory_embed_doc_basic`, `test_build_memory_embed_doc_truncates`, `test_build_memory_embed_doc_short`

**Build status**: `embed_memory_pending` does not compile (expected) — depends on Store trait methods from Wave 1A. All 3 `build_memory_embed_doc` tests pass when filtered (`cargo test -p commitmux-embed -- build_memory_embed_doc`; requires temporarily gating `embed_memory_pending` behind `#[cfg(feature = "memory")]` since Rust compiles the whole crate).

**Out-of-scope build blockers** (as documented):
- `Store::get_memory_docs_without_embeddings` — added by Wave 1A
- `Store::store_memory_embedding` — added by Wave 1A
- NullStore missing 5 new trait methods — cascade handled by Wave 2A / Orchestrator merge fixup

### Agent B (Wave 2) - Completion Report

**Status**: COMPLETE
**Commit**: `da348cc` (`feat(mcp): add commitmux_search_memory MCP tool`)
**Branch**: `main`
**Files modified**: `crates/mcp/src/lib.rs`, `crates/mcp/src/tools.rs`

**What was implemented**:

1. **crates/mcp/src/tools.rs** -- Added `SearchMemoryInput` struct with `query` (required), `project`, `source_type`, and `limit` fields.

2. **crates/mcp/src/lib.rs** -- Updated imports to include `SearchMemoryInput` and `MemorySearchOpts`.

3. **crates/mcp/src/lib.rs** -- Added `commitmux_search_memory` tool entry to `handle_tools_list` with full JSON Schema (query required, project/source_type/limit optional).

4. **crates/mcp/src/lib.rs** -- Added dispatch in `handle_tools_call` routing to `call_search_memory`.

5. **crates/mcp/src/lib.rs** -- Added `call_search_memory` method following `call_search_semantic` pattern: validates empty query and limit=0, builds EmbedConfig/Embedder from store, embeds query via one-shot tokio runtime, builds `MemorySearchOpts`, calls `store.search_memory`, serializes results.

6. **crates/mcp/src/lib.rs** -- Added 5 new Store trait method stubs to both `StubStore` and `StubStoreWithRepos` (`upsert_memory_doc`, `get_memory_doc_by_source`, `get_memory_docs_without_embeddings`, `store_memory_embedding`, `search_memory`).

7. **Tests** -- Added 3 new tests: `test_tools_list_includes_search_memory`, `test_search_memory_rejects_empty_query`, `test_search_memory_rejects_limit_zero`. Updated `test_tools_list_response` expected count from 6 to 7.

**Verification**:
- `cargo test -p commitmux-mcp` -- 18 passed (including 2 existing tool input deserialization tests)
- `cargo clippy -p commitmux-mcp -- -D warnings` -- 0 warnings

### Post-Wave-2 Verification

After merging all waves:
```bash
cargo test                          # all workspace tests pass
cargo clippy -- -D warnings         # zero warnings
cargo build                         # binary builds successfully
```
