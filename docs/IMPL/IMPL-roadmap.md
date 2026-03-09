# IMPL: commitmux Post-MVP Roadmap (P0–P4)

## Suitability Assessment

Verdict: SUITABLE
test_command: `cargo test --workspace`
lint_command: `cargo clippy -- -D warnings`

The 10 roadmap items decompose cleanly across 7 agents with disjoint file
ownership. The codebase is a Rust workspace with five crates (`types`, `store`,
`ingest`, `mcp`, `embed`) plus `src/main.rs`. Cross-agent interfaces are
well-defined by the existing `Store` trait in `crates/types/src/lib.rs` —
new store methods follow the established pattern and can be specified as
binding contracts before any agent starts.

Pre-implementation scan results:
- Total items: 10 roadmap features
- Already implemented: 0 (all are clearly absent from current code)
- Partially implemented: 1 item — `touches` glob bug is documented, the
  fix is a clean edit to `queries.rs` (no investigation required)
- To-do: 9 items

Key ordering constraint: Agent D (FTS memory search) adds `search_memory_fts`
to the `Store` trait and `SqliteStore`. Agent E (FTS patch preview cap raise)
modifies `schema.rs` and `queries.rs`. These are independent of each other and
of P1 agents. The `commitmux_search_saw` tool (Agent C) depends only on the
existing `search()` store method — no new store methods required.

Estimated times:
- Scout phase: ~20 min (deep codebase read + interface contracts)
- Agent execution: ~60 min (7 agents × ~10–12 min avg, two parallel waves)
- Merge & verification: ~10 min
Total SAW time: ~90 min

Sequential baseline: ~130 min (7 × 15 min avg + overhead)
Time savings: ~40 min (~30% faster)

Recommendation: Clear speedup. Agents are largely independent with 2–5 files
each, and `cargo test --workspace` is a multi-second build. Proceed.

---

## Quality Gates

level: standard

gates:
  - type: build
    command: `cargo build --workspace`
    required: true
  - type: lint
    command: `cargo clippy -- -D warnings`
    required: true
  - type: test
    command: `cargo test --workspace`
    required: true

---

## Scaffolds

No scaffolds needed — agents have independent type ownership. The `Store`
trait extension methods are defined by Agent A in `crates/types/src/lib.rs`
(Wave 1), and all downstream agents in Wave 2 depend on Agent A completing
before they begin.

---

## Pre-Mortem

**Overall risk:** medium

**Failure modes:**

| Scenario | Likelihood | Impact | Mitigation |
|----------|-----------|--------|------------|
| Agent A adds `search_memory_fts` to `Store` trait; Agent D (same wave) also touches `crates/types/src/lib.rs` | low | high | Agents A and D are assigned to the same file — re-check: Agent A owns `crates/types/src/lib.rs` exclusively; Agent D reads it but does NOT modify it (D adds only to `queries.rs`). Conflict resolved by design. |
| `sqlite-vec` vec0 dimension is hardcoded at `FLOAT[768]`; Agent G's dimension validation requires schema migration, which cannot alter a `vec0` virtual table | medium | high | Agent G writes a soft check at write time (read stored dim from `config` table, compare to actual embedding size, return error). No DDL migration to vec0 is needed. |
| FTS memory search (Agent D) collides with Agent E's `queries.rs` edits | low | medium | Agents D and E both touch `queries.rs`. Assigned to same agent (D owns both FTS memory and the patch preview cap raise) to eliminate conflict. |
| `commitmux install-hook` writes to `.git/hooks/` which is inside the user's repo, not the commitmux source tree — integration test is tricky | medium | low | Agent B uses `tempfile` + `git2::Repository::init` same pattern as `crates/ingest/src/lib.rs` tests. |
| `SearchSawInput` deserialization — `wave` field is `Option<u32>` but FTS5 query building requires special escaping of SAW merge commit message format | low | low | Agent C's FTS query building is pure string formatting. Escape with FTS5 phrase quotes. |
| Agent F (IMPL doc indexing) uses `std::fs::read_dir` glob scan — docs path is not validated to be inside a working tree | low | low | Agent F validates path exists before scanning; clear error message if not found. |

---

## Known Issues

- The embed crate's `NullStore` in tests has a comment noting that "Wave 1A's
  new methods don't exist in this worktree yet" — this was from a prior SAW
  wave and is resolved in the current codebase. No known test failures in
  `cargo test --workspace` at present.

---

## Dependency Graph

```yaml type=impl-dep-graph
Wave 1 (4 parallel agents — foundation):
    [A] crates/types/src/lib.rs
         Adds `search_memory_fts` Store method + `MemoryFtsSearchOpts` type
         ✓ root

    [B] src/main.rs
         Adds `install-hook` CLI command, auto-sync on MCP startup,
         `index-impl-docs` CLI command (P0 + P1 IMPL doc indexing)
         ✓ root

    [C] crates/mcp/src/tools.rs, crates/mcp/src/lib.rs
         Adds `commitmux_search_saw` MCP tool (P1)
         ✓ root (search_saw builds its own FTS query using existing store.search())

    [G] crates/embed/src/lib.rs
         Adds dimension validation guard (P4)
         ✓ root

Wave 2 (2 parallel agents — depend on Wave 1):
    [D] crates/store/src/queries.rs, crates/store/src/schema.rs
         Implements `search_memory_fts` Store method (P3 FTS fallback),
         raises patch_preview cap 500→2000 chars (P2 FTS patch preview),
         fixes `touches` LIKE→real path match or renames param (P2 glob fix),
         adds prefix matching to `get_patch` (P2 SHA consistency)
         depends on: [A] (Store trait method must exist to implement it)

```

---

## Interface Contracts

All contracts below are Rust signatures that agents must implement exactly.
Agents implementing these must not change the signatures; they may add
`#[allow(...)]` attributes as needed.

### 1. `Store` trait new method — `search_memory_fts` (Agent A defines, Agent D implements)

```rust
/// FTS5 keyword search over memory_docs.content.
/// Returns results ranked by FTS5 bm25 score, most relevant first.
/// Falls back gracefully when memory_docs table is empty.
/// Returns at most `opts.limit.unwrap_or(10)` results.
fn search_memory_fts(
    &self,
    query: &str,
    opts: &MemoryFtsSearchOpts,
) -> Result<Vec<MemoryMatch>>;
```

### 2. `MemoryFtsSearchOpts` type (Agent A defines in `crates/types/src/lib.rs`)

```rust
/// Options for FTS keyword search over memory docs.
#[derive(Debug, Clone, Default)]
pub struct MemoryFtsSearchOpts {
    pub project: Option<String>,
    pub source_type: Option<String>,
    pub limit: Option<usize>,
}
```

### 3. `SearchSawInput` type (Agent C defines in `crates/mcp/src/tools.rs`)

```rust
/// Input type for the `commitmux_search_saw` tool.
#[derive(Debug, serde::Deserialize)]
pub struct SearchSawInput {
    pub feature: String,
    pub wave: Option<u32>,
    pub limit: Option<usize>,
}
```

### 4. MCP tool registration: `commitmux_search_saw` (Agent C, `crates/mcp/src/lib.rs`)

Tool name: `"commitmux_search_saw"`
Dispatches to: `self.call_search_saw(&arguments)`
Method `call_search_saw` signature:
```rust
fn call_search_saw(&self, arguments: &serde_json::Value) -> Result<String, String>;
```

### 5. CLI subcommand: `install-hook` (Agent B, `src/main.rs`)

```rust
// New Commands variant:
#[command(about = "Install a post-commit git hook that calls `commitmux sync` after every commit")]
InstallHook {
    #[arg(help = "Path to the git repository")]
    repo: PathBuf,
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3)")]
    db: Option<PathBuf>,
}
```

Hook content written to `.git/hooks/post-commit`:
```sh
#!/bin/sh
commitmux sync --repo "$(git rev-parse --show-toplevel)" 2>/dev/null || true
```
Must `chmod +x` the file after writing.

### 6. CLI subcommand: `index-impl-docs` (Agent B, `src/main.rs`)

```rust
#[command(about = "Index IMPL docs from docs/IMPL/IMPL-*.md files in a working tree")]
IndexImplDocs {
    #[arg(help = "Path to the working tree root (directory containing docs/IMPL/)")]
    path: PathBuf,
    #[arg(long, help = "Project name for memory doc tagging (default: directory name)")]
    project: Option<String>,
    #[arg(long, help = "Path to database file")]
    db: Option<PathBuf>,
}
```

Uses existing `store.upsert_memory_doc()` with `source_type: MemorySourceType::ImplDoc`
(new variant — see contract 7).

### 7. `MemorySourceType` new variant (Agent A defines, `crates/types/src/lib.rs`)

```rust
pub enum MemorySourceType {
    SessionSummary,
    Task,
    Blocker,
    MemoryFile,
    Decision,
    ImplDoc,   // NEW — for IMPL doc indexing
}
```
Add `"impl_doc"` to `as_str()` and `from_str()` match arms.

### 8. Auto-sync on MCP startup (Agent B, `src/main.rs` — `Commands::Serve` arm)

Before calling `commitmux_mcp::run_mcp_server(store)`, check `ingest_state`:
- Call `store.list_repos()` to get all repos
- For each repo, call `store.get_ingest_state(repo.repo_id)`
- If `last_synced_at` is more than 3600 seconds ago (or `None`), call ingester
- Print sync summary to stderr (not stdout — MCP server uses stdout for JSON-RPC)
- Only sync; do NOT block on embedding (too slow for startup)

### 9. Dimension validation in embed (Agent G, `crates/embed/src/lib.rs`)

New function:
```rust
/// Validates that `embedding.len()` matches the dimension stored at
/// `embed.dimension` in the config table. If no dimension is stored yet,
/// stores it. If dimension mismatches, returns Err with a human-readable
/// message including the expected and actual sizes.
pub fn validate_or_store_dimension(
    store: &dyn Store,
    embedding: &[f32],
) -> anyhow::Result<()>;
```

Called from `embed_pending()` before `store.store_embedding()` on the first
commit of each batch. On mismatch: return `Err` immediately (do not embed).

---

## File Ownership

```yaml type=impl-file-ownership
| File                              | Agent | Wave | Depends On |
|-----------------------------------|-------|------|------------|
| crates/types/src/lib.rs           | A     | 1    | —          |
| src/main.rs                       | B     | 1    | —          |
| crates/mcp/src/tools.rs           | C     | 1    | —          |
| crates/mcp/src/lib.rs             | C     | 1    | —          |
| crates/embed/src/lib.rs           | G     | 1    | —          |
| crates/store/src/queries.rs       | D     | 2    | A          |
| crates/store/src/schema.rs        | D     | 2    | A          |
```

Files NOT changing (cascade candidates — monitor at post-merge):
- `crates/ingest/src/lib.rs` — `MockStore` must gain stub for `search_memory_fts`
  after Agent A adds it to the `Store` trait. Orchestrator adds stub post-merge
  or Agent A adds it proactively as an `unimplemented!()` stub.
- `crates/embed/src/lib.rs` — `NullStore` in tests must gain stub for
  `search_memory_fts`. Same treatment.
- `crates/store/src/lib.rs` — does not need modification; `SqliteStore`'s
  `Store` impl lives in `queries.rs`.

---

## Wave Structure

```yaml type=impl-wave-structure
Wave 1: [A] [B] [C] [G]     <- 4 parallel agents (foundation)
              | (A complete)
Wave 2:    [D]               <- 1 agent (trait impl, depends on A)
```

---

## Wave 1

Wave 1 delivers: new Store trait method (`search_memory_fts` + new types),
all CLI additions (`install-hook`, auto-sync, `index-impl-docs`), the new
`commitmux_search_saw` MCP tool, and the embedding dimension validator.

These four agents are fully independent — no agent reads another's output.
The only post-Wave-1 orchestrator action before launching Wave 2 is verifying
that `cargo build --workspace` passes and that `crates/ingest/src/lib.rs`
compiles (Agent A's new trait method requires MockStore stubs; fix if needed).

### Agent A - Store Trait Extension (new types + `search_memory_fts` method)

**Project root:** `/Users/dayna.blackwell/code/commitmux`

**Context:**
You are implementing two additions to the `Store` trait and one new enum
variant to support the P3 FTS memory search feature and P1 IMPL doc indexing.
The `Store` trait is the central abstraction for all database operations. Every
new database capability starts here as a trait method.

**Your file:** `crates/types/src/lib.rs`

**What to implement:**

1. Add `ImplDoc` variant to `MemorySourceType`:
   - Add `ImplDoc` to the enum body
   - Add `"impl_doc" => Self::ImplDoc` to `from_str()`
   - Add `Self::ImplDoc => "impl_doc"` to `as_str()`

2. Add `MemoryFtsSearchOpts` struct (after `MemorySearchOpts`):
   ```rust
   #[derive(Debug, Clone, Default)]
   pub struct MemoryFtsSearchOpts {
       pub project: Option<String>,
       pub source_type: Option<String>,
       pub limit: Option<usize>,
   }
   ```

3. Add `search_memory_fts` to the `Store` trait (after `search_memory`):
   ```rust
   /// FTS5 keyword search over memory_docs.content.
   /// Returns results ranked by bm25 relevance, most relevant first.
   /// `opts.limit` defaults to 10. Returns Ok(vec![]) if no FTS table exists.
   fn search_memory_fts(
       &self,
       query: &str,
       opts: &MemoryFtsSearchOpts,
   ) -> Result<Vec<MemoryMatch>>;
   ```

**Cascade fix (do this in your agent):** After adding `search_memory_fts` to
the trait, the `NullStore` in `crates/embed/src/lib.rs` and the `MockStore` in
`crates/ingest/src/lib.rs` will fail to compile. Add stub implementations to
both files:
- `crates/embed/src/lib.rs` — find `impl Store for NullStore`, add:
  ```rust
  fn search_memory_fts(&self, _query: &str, _opts: &commitmux_types::MemoryFtsSearchOpts) -> Result<Vec<commitmux_types::MemoryMatch>> {
      Ok(vec![])
  }
  ```
- `crates/ingest/src/lib.rs` — find `impl Store for MockStore`, add same stub.

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build --workspace
cargo clippy -- -D warnings
cargo test --workspace -p commitmux-types
```

**Do NOT implement** the SQLite query for `search_memory_fts` — that is Agent D's job.

**Completion report format:**
```
status: complete
files_changed:
  - crates/types/src/lib.rs
  - crates/embed/src/lib.rs
  - crates/ingest/src/lib.rs
interface_deviations: none
notes: Added MemoryFtsSearchOpts, ImplDoc variant, search_memory_fts trait method, cascade stubs.
```

---

### Agent B - CLI Additions (install-hook, auto-sync, index-impl-docs)

**Project root:** `/Users/dayna.blackwell/code/commitmux`

**Context:**
You are implementing three P0/P1 CLI features in `src/main.rs`. This file
already has many subcommands (`Init`, `AddRepo`, `Sync`, `Serve`, `IngestMemory`,
etc.) following a consistent pattern. Follow that pattern exactly.

**Your file:** `src/main.rs`

**What to implement:**

**Feature 1: `commitmux install-hook` (P0 Freshness)**

Add to the `Commands` enum:
```rust
#[command(about = "Install a post-commit git hook that calls 'commitmux sync' after every commit")]
InstallHook {
    #[arg(help = "Path to the git repository root")]
    repo: PathBuf,
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
    db: Option<PathBuf>,
},
```

Handler logic in `match cli.command`:
- Resolve `repo` to a canonical path
- Verify it is a git repo (`.git/hooks/` directory exists)
- Write `.git/hooks/post-commit` with this exact content:
  ```sh
  #!/bin/sh
  commitmux sync --repo "$(git rev-parse --show-toplevel)" 2>/dev/null || true
  ```
- `chmod +x` the file using `std::fs::set_permissions` with mode `0o755`
- If `.git/hooks/post-commit` already exists, print a warning and ask user
  to confirm with `--force` flag; add `#[arg(long)] force: bool` to the variant
- Print: `Installed post-commit hook at <path>/.git/hooks/post-commit`

**Feature 2: Auto-sync on MCP startup (P0 Freshness)**

In the `Commands::Serve` arm, BEFORE calling `commitmux_mcp::run_mcp_server`,
add a startup sync pass:
- The sync threshold is 1 hour (3600 seconds), hardcoded
- Call `store.list_repos()` — get all repos
- For each repo: call `store.get_ingest_state(repo.repo_id)`
  - If `last_synced_at` is `None` OR `(now - last_synced_at) > 3600`:
    - `eprintln!("commitmux: syncing '{}' (stale)...", repo.name)`
    - Run `Git2Ingester::new().sync_repo(&repo, &store, &IgnoreConfig::default())`
    - On error: `eprintln!("commitmux: sync error for '{}': {e}", repo.name)` — do NOT abort
    - On success: print brief summary to stderr
- Use `std::time::SystemTime::now()` for current timestamp (same pattern as
  existing `Commands::Sync` arm in the file)
- Must add `use commitmux_ingest::Git2Ingester;` and `use commitmux_types::IgnoreConfig;`
  at the top (check if already imported)
- Total startup sync should not block MCP startup for long repos; it runs
  synchronously before entering the JSON-RPC loop. This is acceptable for now.

**Feature 3: `commitmux index-impl-docs` (P1 IMPL doc indexing)**

Add to `Commands` enum:
```rust
#[command(about = "Index IMPL docs from docs/IMPL/IMPL-*.md files in a working tree")]
IndexImplDocs {
    #[arg(help = "Path to working tree root (must contain docs/IMPL/)")]
    path: PathBuf,
    #[arg(long, help = "Project name for tagging (default: directory name)")]
    project: Option<String>,
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3)")]
    db: Option<PathBuf>,
},
```

Handler logic:
- Resolve `path` to canonical form
- Derive project name from `path.file_name()` if `--project` not provided
- Look for `path/docs/IMPL/IMPL-*.md` using `std::fs::read_dir`
- For each `.md` file found:
  - Read file contents (`std::fs::read_to_string`)
  - Get file mtime from metadata (`metadata.modified()` → unix seconds)
  - Call `store.upsert_memory_doc(&MemoryDocInput { source: file_path_str, project, source_type: MemorySourceType::ImplDoc, content, file_mtime })`
  - Print `Indexed: <filename>` or `Skipped (unchanged): <filename>` based on whether content was updated
- At end: print `Indexed N IMPL docs from <path>/docs/IMPL/`
- Use `MemorySourceType::ImplDoc` variant (defined by Agent A)

**Note on imports:** Add to the top of `src/main.rs` as needed:
```rust
use commitmux_types::MemoryDocInput;  // likely already imported
use commitmux_types::MemorySourceType; // likely already imported
```

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build --workspace
cargo clippy -- -D warnings
cargo test --workspace -p commitmux
```

**Completion report format:**
```
status: complete
files_changed:
  - src/main.rs
interface_deviations: none
notes: Added install-hook, index-impl-docs commands, auto-sync in Serve arm.
```

---

### Agent C - MCP Tool: `commitmux_search_saw`

**Project root:** `/Users/dayna.blackwell/code/commitmux`

**Context:**
You are adding the `commitmux_search_saw` MCP tool to the existing MCP server.
The server is in `crates/mcp/`. It uses a manual JSON-RPC dispatch pattern —
read `crates/mcp/src/lib.rs` and `crates/mcp/src/tools.rs` carefully before
starting. Every MCP tool follows the same three-step pattern:
1. Add input struct to `tools.rs`
2. Add tool schema to `handle_tools_list()` in `lib.rs`
3. Add dispatch case and handler method to `handle_tools_call()` / `lib.rs`

**Your files:** `crates/mcp/src/tools.rs`, `crates/mcp/src/lib.rs`

**What to implement:**

**Step 1 — `tools.rs`: Add `SearchSawInput`**

```rust
/// Input type for the `commitmux_search_saw` tool.
#[derive(Debug, serde::Deserialize)]
pub struct SearchSawInput {
    pub feature: String,
    pub wave: Option<u32>,
    pub limit: Option<usize>,
}
```

**Step 2 — `lib.rs`: Add `SearchSawInput` to imports**

In the `use tools::{...}` block at the top, add `SearchSawInput`.

**Step 3 — `lib.rs`: Add to `handle_tools_list()`**

Add after the `commitmux_search_memory` entry in the `"tools"` array:
```json
{
    "name": "commitmux_search_saw",
    "description": "Search SAW (Scout-and-Wave) protocol history. Finds merge commits from wave-based development by feature name and optional wave number. Returns commits grouped by wave and agent.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "feature": {
                "type": "string",
                "description": "Feature slug or description (e.g. 'memory-search' or 'roadmap')"
            },
            "wave": {
                "type": "integer",
                "description": "Optional wave number filter (e.g. 1 for Wave 1 commits only)"
            },
            "limit": {
                "type": "integer",
                "description": "Max results (default 20)"
            }
        },
        "required": ["feature"]
    }
}
```

**Step 4 — `lib.rs`: Add dispatch in `handle_tools_call()`**

In the `match name` block, add:
```rust
"commitmux_search_saw" => self.call_search_saw(&arguments),
```

**Step 5 — `lib.rs`: Implement `call_search_saw`**

Add private method to `McpServer`:
```rust
fn call_search_saw(&self, arguments: &Value) -> Result<String, String> {
    use tools::SearchSawInput;
    let input: SearchSawInput = serde_json::from_value(arguments.clone())
        .map_err(|e| format!("Invalid arguments for commitmux_search_saw: {e}"))?;

    // Build FTS5 query. SAW merge commits have subject lines like:
    // "Merge wave1-agent-A: description" or "feat: add memory search"
    // We search the feature name plus optionally constrain to a wave number.
    let fts_query = if let Some(wave) = input.wave {
        // Quote the feature term to treat it as a phrase
        format!("\"{}\" wave{}", input.feature, wave)
    } else {
        input.feature.clone()
    };

    let opts = commitmux_types::SearchOpts {
        since: None,
        repos: None,
        paths: None,
        limit: input.limit.or(Some(20)),
    };

    self.store
        .search(&fts_query, &opts)
        .map_err(|e| e.to_string())
        .and_then(|results| serde_json::to_string(&results).map_err(|e| e.to_string()))
}
```

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build -p commitmux-mcp
cargo clippy -p commitmux-mcp -- -D warnings
cargo test -p commitmux-mcp
```

**Completion report format:**
```
status: complete
files_changed:
  - crates/mcp/src/tools.rs
  - crates/mcp/src/lib.rs
interface_deviations: none
notes: Added commitmux_search_saw tool using existing store.search() FTS.
```

---

### Agent G - Embedding Dimension Validation (P4)

**Project root:** `/Users/dayna.blackwell/code/commitmux`

**Context:**
You are adding a dimension validation guard to the `commitmux-embed` crate.
The schema hardcodes `FLOAT[768]` for embedding columns. If a user switches
to a model with a different dimension, old and new vectors co-exist silently,
producing nonsense ANN results. Your job is to detect this at write time and
error clearly.

**Your file:** `crates/embed/src/lib.rs`

**What to implement:**

1. Add constant:
   ```rust
   const CONFIG_KEY_EMBED_DIM: &str = "embed.dimension";
   ```

2. Add function `validate_or_store_dimension`:
   ```rust
   /// Validates embedding dimension consistency.
   ///
   /// On first call (no stored dimension): stores `embedding.len()` in config
   /// under `embed.dimension` and returns Ok.
   ///
   /// On subsequent calls: reads stored dimension, compares to `embedding.len()`.
   /// If mismatch: returns Err with an actionable message.
   /// If match: returns Ok.
   pub fn validate_or_store_dimension(
       store: &dyn commitmux_types::Store,
       embedding: &[f32],
   ) -> anyhow::Result<()> {
       let dim = embedding.len();
       match store.get_config(CONFIG_KEY_EMBED_DIM)? {
           None => {
               // First time: record the dimension
               store.set_config(CONFIG_KEY_EMBED_DIM, &dim.to_string())?;
               Ok(())
           }
           Some(stored) => {
               let stored_dim: usize = stored.parse().unwrap_or(0);
               if stored_dim != dim {
                   anyhow::bail!(
                       "Embedding dimension mismatch: index was built with {}-dimensional vectors, \
                        but current model produces {} dimensions. \
                        Run 'commitmux reindex --repo <name>' to rebuild embeddings, \
                        or switch back to the original model ({}-dim).",
                       stored_dim, dim, stored_dim
                   )
               }
               Ok(())
           }
       }
   }
   ```

3. Call `validate_or_store_dimension` in `embed_pending()` before the first
   `store.store_embedding()` call. Do this by checking on the first commit
   in the batch:
   - Before the `for commit in &batch` loop, check `if !batch.is_empty()`:
     ```rust
     // Validate dimension consistency on first commit in batch.
     if let Some(first) = batch.first() {
         let doc = build_embed_doc(first);
         // We need to embed to get the dimension; do a pre-check embed first:
         // Actually, validate after getting the first embedding:
     }
     ```
   - Simpler approach: after `Ok(embedding) =>` branch in the loop, on the
     first iteration, call `validate_or_store_dimension(store, &embedding)?;`
     Use a `let mut first_checked = false;` flag before the loop.

4. Add unit tests:
   ```rust
   #[test]
   fn test_validate_dimension_first_call_stores() { ... }
   #[test]
   fn test_validate_dimension_mismatch_errors() { ... }
   #[test]
   fn test_validate_dimension_match_ok() { ... }
   ```
   Use the existing `NullStore` mock — you will need to make it track
   config key/value. Add a `config: Mutex<HashMap<String, String>>` field
   to `NullStore` and implement `get_config`/`set_config` to read/write it.

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build -p commitmux-embed
cargo clippy -p commitmux-embed -- -D warnings
cargo test -p commitmux-embed
```

**Completion report format:**
```
status: complete
files_changed:
  - crates/embed/src/lib.rs
interface_deviations: none
notes: Added validate_or_store_dimension; called from embed_pending first iteration.
```

---

## Wave 2

Wave 2 depends on Agent A completing (the `Store` trait must have
`search_memory_fts` before Agent D can implement it in `SqliteStore`).

Wave 2 contains one agent (D) covering four related fixes in `queries.rs`
and `schema.rs`. These are grouped together because: (1) all are in the same
two files, (2) they are logically independent from each other but share file
ownership, (3) the FTS schema change and the query implementations must land
together.

### Agent D - Store Query Fixes + FTS Memory Search (P2 + P3)

**Project root:** `/Users/dayna.blackwell/code/commitmux`

**Context:**
You are implementing four related improvements in the `commitmux-store` crate's
query layer. All changes land in `crates/store/src/queries.rs` and one schema
change in `crates/store/src/schema.rs`. The `Store` trait method you are
implementing (`search_memory_fts`) was defined by Wave 1 Agent A.

**Your files:** `crates/store/src/queries.rs`, `crates/store/src/schema.rs`

**Fix 1: FTS5 over `memory_docs.content` — P3 FTS Fallback**

In `schema.rs`, add a new virtual FTS5 table (after the `memory_embeddings` table):
```sql
CREATE VIRTUAL TABLE IF NOT EXISTS memory_docs_fts
    USING fts5(content, content='memory_docs', content_rowid='doc_id');
```

Add this to the `SCHEMA_SQL` constant string (inside the `r#"..."#` block).

In `queries.rs`, implement `search_memory_fts` for `SqliteStore`:

```rust
fn search_memory_fts(
    &self,
    query: &str,
    opts: &MemoryFtsSearchOpts,
) -> Result<Vec<MemoryMatch>> {
    let conn = self.conn.lock().unwrap();
    let limit = opts.limit.unwrap_or(10) as i64;

    let mut extra_conditions = String::new();
    let mut bind_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 2usize; // ?1 = FTS query

    if let Some(ref project) = opts.project {
        extra_conditions.push_str(&format!(" AND md.project = ?{}", param_idx));
        bind_vals.push(Box::new(project.clone()));
        param_idx += 1;
    }
    if let Some(ref source_type) = opts.source_type {
        extra_conditions.push_str(&format!(" AND md.source_type = ?{}", param_idx));
        bind_vals.push(Box::new(source_type.clone()));
        param_idx += 1;
    }

    let sql = format!(
        "SELECT md.doc_id, md.source, md.project, md.source_type, md.content,
                bm25(memory_docs_fts) AS score
         FROM memory_docs_fts
         JOIN memory_docs md ON md.doc_id = memory_docs_fts.rowid
         WHERE memory_docs_fts MATCH ?1{}
         ORDER BY score
         LIMIT ?{}",
        extra_conditions, param_idx
    );

    bind_vals.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;

    let all_params: Vec<&dyn rusqlite::types::ToSql> =
        std::iter::once(&query as &dyn rusqlite::types::ToSql)
            .chain(bind_vals.iter().map(|b| b.as_ref()))
            .collect();

    let rows: rusqlite::Result<Vec<MemoryMatch>> = stmt
        .query_map(all_params.as_slice(), |row| {
            Ok(MemoryMatch {
                doc_id: row.get(0)?,
                source: row.get(1)?,
                project: row.get(2)?,
                source_type: row.get(3)?,
                content: row.get(4)?,
                score: row.get::<_, f64>(5)? as f32,
            })
        })?
        .collect();
    Ok(rows?)
}
```

Also add `memory_docs_fts` maintenance: when `upsert_memory_doc` is called,
the FTS table must be updated. Find `upsert_memory_doc` in `queries.rs` and
add FTS insert/update after the main upsert (follow the same `DELETE + INSERT`
pattern used for `commits_fts`).

**Fix 2: Raise `patch_preview` cap from 500 to 2000 chars — P2 FTS Patch Preview**

In `queries.rs`, find `upsert_patch`:
```rust
// Current line (approximately):
let preview: String = patch.patch_preview.chars().take(500).collect();
// Change to:
let preview: String = patch.patch_preview.chars().take(2000).collect();
```

Also update the schema comment if present. The `commits` table column is `TEXT`
(no length constraint in SQLite) so no DDL change is needed.

**Fix 3: `touches` — rename `path_glob` to `path_substring` (P2 glob fix)**

The `touches` function currently does:
```rust
let like_pat = format!("%{}%", path_glob);
```
This is documented behavior, but the parameter name `path_glob` is misleading
because glob syntax (`**/*.rs`) does not work — only substring matching works.

In `queries.rs`: the `touches` implementation uses `path_glob` as a local
variable name — no change needed here (Store trait uses `path_glob: &str`
parameter name, which is a label, not the SQL logic).

Instead, update the `Store` trait doc comment for `touches` and the MCP tool
description. The actual fix requested in plan.md is:
> "either implement real glob matching (glob crate), or rename the parameter
> to `path_substring` and document the actual behavior"

**Decision:** Rename the MCP-facing parameter description only (not the Rust
trait parameter, which would be a breaking change). In `queries.rs`, add a
doc comment to the `Store::touches` implementation:
```rust
// NOTE: `path_glob` is matched as a substring (LIKE %pattern%), not as a
// shell glob. Patterns like `src/**/*.rs` will not work as glob — pass
// `src/` or `.rs` instead for substring matching.
```

For a true fix: update the MCP tool schema description in `crates/mcp/src/lib.rs`
(owned by Agent C — coordinate with orchestrator if Agent C's branch has already
merged, otherwise add the description update to this agent).

Since Agent C owns `lib.rs`, you should NOT modify it. Instead, add the doc
comment in `queries.rs` only, and note the MCP description update in your
completion report as a post-merge step.

**Fix 4: `get_patch` prefix matching — P2 SHA consistency**

In `queries.rs`, find the `get_patch` implementation. Currently:
```rust
"SELECT cp.patch_blob
 FROM commit_patches cp
 JOIN repos r ON r.repo_id = cp.repo_id
 WHERE r.name = ?1 AND cp.sha = ?2",
```

Change the WHERE clause to use prefix matching (same as `get_commit`):
```rust
"SELECT cp.patch_blob
 FROM commit_patches cp
 JOIN repos r ON r.repo_id = cp.repo_id
 WHERE r.name = ?1 AND cp.sha LIKE ?2 || '%'",
```

Also update the `PatchResult` returned to use the full SHA from the DB rather
than the prefix passed in:
```rust
// Add sha to SELECT:
"SELECT cp.patch_blob, cp.sha
 FROM commit_patches cp
 JOIN repos r ON r.repo_id = cp.repo_id
 WHERE r.name = ?1 AND cp.sha LIKE ?2 || '%'",
// Update the row extraction to get sha from column 1:
let compressed: Vec<u8> = row.get(0)?;
let full_sha: String = row.get(1)?;
// Return full_sha in PatchResult.sha instead of the input `sha` prefix
```

Add a test in `crates/store/src/lib.rs`'s `#[cfg(test)]` section:
```rust
#[test]
fn test_get_patch_prefix_sha() {
    // verify that get_patch("repo", "1234") returns the same result as
    // get_patch("repo", "1234abcd") when only one patch matches
}
```

**FTS maintenance for memory_docs — important:**

When `upsert_memory_doc` runs, it must keep `memory_docs_fts` in sync.
Find the `upsert_memory_doc` implementation and add after the main SQL upsert:
```rust
// Fetch the doc_id and content for FTS update
let (doc_id, content): (i64, String) = conn.query_row(
    "SELECT doc_id, content FROM memory_docs WHERE source = ?1",
    params![input.source],
    |row| Ok((row.get(0)?, row.get(1)?)),
)?;
// Delete old FTS entry (if any)
let _ = conn.execute(
    "INSERT INTO memory_docs_fts(memory_docs_fts, rowid, content) VALUES('delete', ?1, ?2)",
    params![doc_id, content],
);
// Insert new FTS entry
conn.execute(
    "INSERT INTO memory_docs_fts(rowid, content) VALUES(?1, ?2)",
    params![doc_id, input.content],
)?;
```

**Imports to add at top of `queries.rs`:**
```rust
use commitmux_types::MemoryFtsSearchOpts; // new type from Agent A
```

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build --workspace
cargo clippy -- -D warnings
cargo test --workspace -p commitmux-store
```

**Completion report format:**
```
status: complete
files_changed:
  - crates/store/src/queries.rs
  - crates/store/src/schema.rs
interface_deviations: none
out_of_scope_deps:
  - crates/mcp/src/lib.rs: update commitmux_touches description to say
    "path_substring" and note that glob patterns do not work. Apply post-merge.
notes: >
  Implemented search_memory_fts with FTS5 over memory_docs.content.
  Raised patch_preview cap to 2000.
  Added prefix matching to get_patch.
  Added memory_docs_fts FTS sync in upsert_memory_doc.
  MCP touches description update deferred to orchestrator (lib.rs owned by Agent C).
```

---

## Wave Execution Loop

After Wave 1 completes, work through the Orchestrator Post-Merge Checklist
below. The key gate before launching Wave 2 is that `cargo build --workspace`
passes — Agent A's new `search_memory_fts` method must compile across all
crates (types, store, ingest, embed, mcp) before Agent D can implement it.

If Agent A's cascade stubs are incomplete, apply missing stubs manually before
merging Wave 1 agents and before launching Wave 2.

## Orchestrator Post-Merge Checklist

After wave 1 completes:

- [ ] Read all agent completion reports — confirm all `status: complete`; if any
      `partial` or `blocked`, stop and resolve before merging
- [ ] Conflict prediction — cross-reference `files_changed` lists; Agent A owns
      `crates/types/src/lib.rs`, B owns `src/main.rs`, C owns MCP files, G owns
      embed — no overlap expected
- [ ] Review `interface_deviations` — update Agent D prompt if Agent A deviated
      from the `search_memory_fts` signature contract
- [ ] Merge each agent: `git merge --no-ff <branch> -m "Merge wave1-agent-{ID}: <desc>"`
- [ ] Worktree cleanup: `git worktree remove <path>` + `git branch -d <branch>` for each
- [ ] Post-merge verification:
      - [ ] Linter auto-fix pass: `cargo fmt` (optional; check if CI requires it)
      - [ ] `cargo build --workspace && cargo clippy -- -D warnings && cargo test --workspace`
- [ ] E20 stub scan: collect `files_changed`+`files_created` from all completion reports; run stub scan; append output to IMPL doc as `## Stub Report — Wave 1`
- [ ] E21 quality gates: run `cargo build --workspace` (required) and `cargo test --workspace` (required)
- [ ] Fix cascade failures — `crates/ingest/src/lib.rs` MockStore stubs must exist; Agent A should have added them. If missing, add `search_memory_fts` stub manually.
- [ ] Tick status checkboxes for Wave 1 agents
- [ ] Update interface contracts for any deviations logged
- [ ] Feature-specific post-Wave-1 steps:
      - [ ] Verify `commitmux install-hook --help` shows new subcommand
      - [ ] Verify `commitmux index-impl-docs --help` shows new subcommand
      - [ ] Verify `commitmux_search_saw` appears in MCP `tools/list` response
- [ ] Commit: `git commit -m "Merge wave 1: install-hook, auto-sync, search-saw, FTS types, embed dim validation"`
- [ ] Launch Wave 2 (Agent D)

After wave 2 completes:

- [ ] Read Agent D completion report — confirm `status: complete`
- [ ] No conflict prediction needed (single agent wave)
- [ ] Merge: `git merge --no-ff <branch> -m "Merge wave2-agent-D: FTS memory search, SHA prefix fix, patch preview 2000"`
- [ ] Worktree cleanup
- [ ] Post-merge verification:
      - [ ] `cargo build --workspace && cargo clippy -- -D warnings && cargo test --workspace`
- [ ] E20 stub scan on Agent D files
- [ ] Feature-specific post-Wave-2 steps:
      - [ ] Apply MCP `commitmux_touches` description update from Agent D's `out_of_scope_deps` note
        (edit `crates/mcp/src/lib.rs`, change `path_glob` description to note substring behavior)
      - [ ] Verify `search_memory_fts` works: `echo '...' | commitmux serve` (manual test)
      - [ ] Verify `get_patch` accepts prefix SHA (manual test via MCP or `commitmux show`)
- [ ] Commit: `git commit -m "Merge wave 2: FTS memory search, patch preview 2000, SHA prefix, touches doc fix"`

### Status

| Wave | Agent | Description | Status |
|------|-------|-------------|--------|
| 1    | A     | Store trait: `search_memory_fts`, `MemoryFtsSearchOpts`, `ImplDoc` variant + cascade stubs | TO-DO |
| 1    | B     | CLI: `install-hook`, `index-impl-docs`, auto-sync in `serve` | TO-DO |
| 1    | C     | MCP: `commitmux_search_saw` tool | TO-DO |
| 1    | G     | Embed: dimension validation guard + tests | TO-DO |
| 2    | D     | Store: FTS memory search impl, patch_preview→2000, `get_patch` prefix SHA, `memory_docs_fts` schema | TO-DO |
| —    | Orch  | MCP touches description fix (out_of_scope from D), post-merge install + integration test | TO-DO |

---

### Agent A - Completion Report

```yaml type=impl-completion-report
status: complete
worktree: .claude/worktrees/wave1-agent-A
branch: wave1-agent-A
commit: 0105f27a871188f6fd7cbe851419aff00ca40b6c
files_changed:
  - crates/types/src/lib.rs
  - crates/embed/src/lib.rs
  - crates/ingest/src/lib.rs
  - crates/store/src/queries.rs
  - crates/mcp/src/lib.rs
files_created: []
interface_deviations: []
out_of_scope_deps:
  - crates/store/src/queries.rs (owned by Agent D) required a cascade stub for SqliteStore to satisfy cargo build --workspace; added minimal Ok(vec![]) stub only, no FTS logic
  - crates/mcp/src/lib.rs (owned by Agent C) required cascade stubs for StubStore and StubStoreWithRepos; added minimal Ok(vec![]) stubs only
tests_added: []
verification: PASS
```

All changes are minimal stubs returning `Ok(vec![])`. The `search_memory_fts` trait method was added to `Store`, `MemoryFtsSearchOpts` struct added after `MemorySearchOpts`, and `ImplDoc` variant added to `MemorySourceType` with `"impl_doc"` in both `as_str()` and `from_str()`. Four additional files outside strict ownership had to receive cascade stubs to satisfy `cargo build --workspace` — these are noted in `out_of_scope_deps`. Agent D should replace the `queries.rs` stub with the real FTS implementation; Agent C may wish to review/own the `mcp/src/lib.rs` stubs.

### Agent C - Completion Report

```yaml type=impl-completion-report
status: complete
worktree: .claude/worktrees/wave1-agent-C
branch: wave1-agent-C
commit: 584a5305cdd3aba0fdd09fbeb969b1feaae84a84
files_changed:
  - crates/mcp/src/tools.rs
  - crates/mcp/src/lib.rs
interface_deviations: []
out_of_scope_deps: []
tests_added: []
verification: PASS
```

Added `SearchSawInput` struct to `tools.rs`, imported it in `lib.rs`, added the tool schema to `handle_tools_list`, dispatch arm in `handle_tools_call`, and the `call_search_saw` implementation. The implementation reuses `self.store.search()` with a constructed FTS5 query — no new Store trait methods required. All 18 existing tests pass; clippy and build both clean.

### Agent G - Completion Report

```yaml type=impl-completion-report
status: complete
worktree: .claude/worktrees/wave1-agent-G
branch: wave1-agent-G
commit: 3f12ef17e72978c4d035180cf9677c3344049060
files_changed:
  - crates/embed/src/lib.rs
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_validate_dimension_first_call_stores
  - test_validate_dimension_match_ok
  - test_validate_dimension_mismatch_errors
verification: PASS
```

`get_config`/`set_config` already existed on the Store trait (no out_of_scope_dep needed). The existing `NullStore` in the test module was upgraded from a no-op unit struct to a stateful struct with a `Mutex<HashMap>` to enable dimension storage across calls within a single test. The `validate_or_store_dimension` function and `CONFIG_KEY_EMBED_DIM` constant were added before `embed_pending`. The guard in `embed_pending` uses a `first_checked` bool to call validation only on the first successful embedding per batch loop (not per batch). All 12 tests pass; clippy and build clean.

### Agent B - Completion Report

```yaml type=impl-completion-report
status: complete
worktree: .claude/worktrees/wave1-agent-B
branch: wave1-agent-B
commit: 6be614d
files_changed:
  - src/main.rs
interface_deviations:
  - IndexImplDocs uses MemorySourceType::MemoryFile instead of MemorySourceType::ImplDoc
    (ImplDoc variant does not exist yet; added by Agent A post-merge)
out_of_scope_deps: []
tests_added: []
verification: PASS
```

All three features implemented in `src/main.rs`:

1. `install-hook`: Writes `#!/bin/sh\ncommitmux sync --repo "$(git rev-parse --show-toplevel)" 2>/dev/null || true` to `.git/hooks/post-commit`, chmod 0o755. Guards against overwrite without `--force`. Validates `.git` directory exists.

2. `index-impl-docs`: Scans `<path>/docs/IMPL/*.md`, upserts each as a `MemoryDocInput` with mtime-based skip logic (same pattern as `IngestMemory`). Uses `MemorySourceType::MemoryFile` as fallback — **change to `MemorySourceType::ImplDoc` after Agent A's changes are merged into main**.

3. Auto-sync in `Serve`: Before `run_mcp_server`, calls `store.list_repos()` and for each repo checks `get_ingest_state`. Syncs if `last_synced_at` is absent (no IngestState row) or `(now - last_synced_at) > 3600`. All output via `eprintln!` to avoid polluting MCP stdout. Errors on individual repos are non-fatal.

Build, clippy (`-D warnings`), and all 89 workspace tests pass.

## Stub Report — Wave 1

65 `unimplemented!()` hits across 3 files — all pre-existing test mock stubs (NullStore, MockStore, StubStore). None introduced by Wave 1 agents (agents used `Ok(vec![])` for cascade stubs). All 89 workspace tests pass; no stubs triggered. No action required.

Files: `crates/embed/src/lib.rs` (21), `crates/ingest/src/lib.rs` (13), `crates/mcp/src/lib.rs` (31).
