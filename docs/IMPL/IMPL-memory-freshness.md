# IMPL: memory-freshness

Three P3/P4 completion features: FTS fallback for `commitmux_search_memory`, `commitmux reindex`
CLI command, and `commitmux install-memory-hook` CLI command.

---

## Suitability Assessment

Verdict: SUITABLE WITH CAVEATS
test_command: `cargo test --workspace`
lint_command: `cargo clippy -- -D warnings`

The three features decompose cleanly into four agents across two waves. The main conflict is
`src/main.rs` ownership: Features 2 (reindex) and 3 (install-memory-hook) both add new `Commands`
variants and match arms to the same file. Resolution: Agent B takes Feature 3 (install-memory-hook)
in Wave 1 since it has no store-layer dependencies; Agent D takes Feature 2 (reindex) in Wave 2
after Agent A ships the new `Store::delete_embeddings_for_repo` trait method. The orchestrator
merges Wave 1 before launching Wave 2, so Agent D sees the post-B `src/main.rs` with no conflicts.

Caveat: Agent C (FTS fallback in `crates/mcp/src/lib.rs`) must also stub
`delete_embeddings_for_repo` into three test mock stores (StubStore in mcp, NullStore in embed,
MockStore in ingest). These mock additions are trivial but require Agent C to run after Agent A
completes (Wave 2). Agent C is NOT blocked on Agent B.

Pre-implementation scan:
- Feature 1 (FTS fallback): TO-DO. `call_search_memory` in `crates/mcp/src/lib.rs` currently
  calls only `self.store.search_memory()` with no fallback. `search_memory_fts` exists on the
  Store trait and is implemented in SqliteStore.
- Feature 2 (reindex): TO-DO. No `Reindex` variant in `Commands` enum. No
  `delete_embeddings_for_repo` method on Store trait.
- Feature 3 (install-memory-hook): TO-DO. No `InstallMemoryHook` variant in `Commands` enum.

Estimated times:
- Scout phase: ~15 min
- Agent execution: ~4 agents × ~12 min avg = ~48 min wall time with parallelism (Wave 1: ~12 min,
  Wave 2: ~12 min; sequential within each wave leg)
- Merge & verification: ~5 min
Total SAW time: ~32 min

Sequential baseline: ~4 × 12 = 48 min
Time savings: ~16 min (33% faster)

Recommendation: Clear speedup for Wave 1 parallelism (A+B run simultaneously). Proceed.

---

## Quality Gates

level: standard

gates:
  - type: build
    command: cargo build --workspace
    required: true
  - type: lint
    command: cargo clippy -- -D warnings
    required: false
  - type: test
    command: cargo test --workspace
    required: true

---

## Scaffolds

No scaffolds needed — agents have independent type ownership. The new Store trait method
`delete_embeddings_for_repo` is defined entirely within Agent A's file ownership
(`crates/types/src/lib.rs`). Downstream agents (C, D) call it; the signature is specified
in Interface Contracts below and is the binding contract.

---

## Pre-Mortem

**Overall risk:** low

| Scenario | Likelihood | Impact | Mitigation |
|----------|-----------|--------|------------|
| Agent D edits `src/main.rs` before Wave 1 merge creates conflicts with Agent B's additions | low | medium | Strict wave gating — Agent D must not start until Wave 1 is fully merged |
| `delete_embeddings_for_repo` SQL deletes from `commit_embed_map` but forgets to also delete from `commit_embeddings` vec0 table, leaving orphan rows | medium | medium | Agent A's prompt explicitly calls out both tables; verification test checks embedding count returns 0 post-delete |
| FTS fallback leaks Ollama error details into the fallback response note | low | low | Agent C formats the note string to say "FTS fallback mode" without echoing internal error |
| `install-memory-hook` writes duplicate entries if `commitmux ingest-memory` already appears under a different key path in `settings.json` | medium | low | Agent B guards on string-match of the command substring, not structural JSON equality |
| `settings.json` parse failure on malformed existing file crashes the hook installer | low | medium | Agent B wraps parse in a recoverable error with clear user message |
| `--all` flag for reindex resets `embed.dimension` config, breaking subsequent embedding if user passes `--all` by mistake | low | medium | Agent D requires explicit `--reset-dim` flag (separate from `--all`) to reset dimension; `--all` alone only deletes and re-embeds, not dimension reset |

---

## Known Issues

- None identified from the current codebase. `cargo test --workspace` passes cleanly on main.

---

## Dependency Graph

```yaml type=impl-dep-graph
Wave 1 (2 parallel agents — foundation):
    [A] crates/types/src/lib.rs
        crates/store/src/queries.rs
         Add Store::delete_embeddings_for_repo trait method and SqliteStore implementation.
         Deletes rows from commit_embed_map (and the corresponding commit_embeddings vec0 rows)
         for a given repo_id. No dependencies on other agents' work.
         ✓ root (no dependencies on other agents)

    [B] src/main.rs
         Add InstallMemoryHook command variant and handler. Reads/writes ~/.claude/settings.json
         with a Stop hook entry for `commitmux ingest-memory`. No store-layer dependencies.
         ✓ root (no dependencies on other agents)

Wave 2 (2 parallel agents — depend on Wave 1):
    [C] crates/mcp/src/lib.rs
        crates/embed/src/lib.rs  (NullStore stub only)
        crates/ingest/src/lib.rs  (MockStore stub only)
         Wire FTS fallback in call_search_memory; add delete_embeddings_for_repo stubs to
         test mock stores.
         depends on: [A] (Store trait must define delete_embeddings_for_repo before StubStore
                          can stub it; this avoids a compile error in mcp tests)

    [D] src/main.rs
         Add Reindex command variant and handler (calls store.delete_embeddings_for_repo,
         then embed_pending). Depends on Agent A's trait method and sees Agent B's changes
         via the Wave 1 merge.
         depends on: [A] [B] (merged src/main.rs from Wave 1; new Store method available)
```

Agent C owns `crates/embed/src/lib.rs` and `crates/ingest/src/lib.rs` for the narrow purpose
of adding a single stub method to their test-internal mock stores. Agent A owns those files'
production code; there is no conflict because Agent A's work is in the trait definition and
SqliteStore implementation, while Agent C's additions are inside `#[cfg(test)]` blocks.

However, since Agent A and Agent C both touch `crates/embed/src/lib.rs` and
`crates/ingest/src/lib.rs`, those files are assigned to Agent A in Wave 1 (production code)
and to Agent C in Wave 2 (test stubs). This is sequential, not concurrent — no conflict.

**Revised ownership clarification:** Agent A owns `crates/embed/src/lib.rs` and
`crates/ingest/src/lib.rs` in Wave 1. Agent C's Wave 2 work on those files is done after
the Wave 1 merge, so ownership is sequential per-file across waves (safe).

---

## Interface Contracts

### New Store trait method (Agent A defines; Agents C, D consume)

```rust
// In crates/types/src/lib.rs, Store trait:
fn delete_embeddings_for_repo(&self, repo_id: i64) -> Result<()>;
```

Implementation in `crates/store/src/queries.rs` (SqliteStore):
```rust
fn delete_embeddings_for_repo(&self, repo_id: i64) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    // Get all embed_ids for this repo from the map table
    // Delete from the vec0 virtual table first, then the map table
    // Pattern: SELECT embed_id FROM commit_embed_map WHERE repo_id = ?1
    //          DELETE FROM commit_embeddings WHERE embed_id IN (...)
    //          DELETE FROM commit_embed_map WHERE repo_id = ?1
}
```

Stub for test mock stores (Agent C adds to StubStore/NullStore/MockStore):
```rust
fn delete_embeddings_for_repo(&self, _repo_id: i64) -> Result<()> {
    Ok(())
}
```

### FTS fallback in call_search_memory (Agent C implements)

Behavior contract (not a function signature — this is the logic Agent C must wire):
- Try vector path first: embed query → `store.search_memory(&embedding, &opts)`
- On embed failure (Ollama down, connection error): fall back to `store.search_memory_fts(&query, &fts_opts)` where `fts_opts` mirrors `opts` fields
- Return FTS results serialized to JSON, with an additional top-level note field or a leading string in the response indicating fallback mode
- Recommended response format: wrap in a JSON object `{ "fallback": true, "reason": "Ollama unavailable: <short msg>", "results": [...] }` OR prepend a `// Note: FTS fallback mode` comment in the string — pick whichever is simpler given existing return type `Result<String, String>`
- On FTS failure after vector failure: return the FTS error (not the original vector error)
- Input validation (empty query, limit=0) still runs before the vector/FTS branching

FTS opts mapping from SearchMemoryInput:
```rust
let fts_opts = MemoryFtsSearchOpts {
    project: input.project.clone(),
    source_type: input.source_type.clone(),
    limit: input.limit,
};
```

### InstallMemoryHook (Agent B implements)

```rust
// In src/main.rs Commands enum:
#[command(about = "Install commitmux ingest-memory as a Claude Code Stop hook in ~/.claude/settings.json")]
InstallMemoryHook {
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
    db: Option<PathBuf>,
    #[arg(long = "claude-settings", help = "Path to Claude settings.json (default: ~/.claude/settings.json)")]
    claude_settings: Option<PathBuf>,
}
```

Hook entry format in settings.json (Claude Code Stop hook):
```json
{
  "hooks": {
    "Stop": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "commitmux ingest-memory"
          }
        ]
      }
    ]
  }
}
```

Duplicate guard: before inserting, scan existing `hooks.Stop[*].hooks[*].command` values for
any string containing `commitmux ingest-memory`. If found, print "Already installed." and exit 0.

### Reindex (Agent D implements)

```rust
// In src/main.rs Commands enum:
#[command(about = "Delete and rebuild embeddings for one or all repositories")]
Reindex {
    #[arg(long, help = "Name of repo to reindex (omit to reindex all)")]
    repo: Option<String>,
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
    db: Option<PathBuf>,
    #[arg(long = "reset-dim", help = "Reset stored embed.dimension after deleting embeddings (required when switching embedding models)")]
    reset_dim: bool,
}
```

Handler logic:
1. Open store, resolve repos (single by name, or all)
2. For each repo: call `store.delete_embeddings_for_repo(repo.repo_id)?`
3. If `--reset-dim`: call `store.set_config(CONFIG_KEY_EMBED_DIM, "")` — or delete the key
   (use `set_config` with empty string; the validate_or_store_dimension function will treat
   empty string as "not found" since `stored.parse::<usize>()` will fail and return `stored_dim=0`,
   which will never equal the actual dim — **safer**: delete the config row entirely via a new
   approach or just use set_config with a sentinel. Prefer: call `store.set_config("embed.dimension", "")` — then in validate_or_store_dimension the `stored.parse().unwrap_or(0)` path makes 0 != real_dim, still an error. **Correct approach**: add a new `delete_config` method OR just re-embed without resetting (the error will re-appear). **Simplest correct approach**: if `--reset-dim`, delete the stored dimension by looking for an alternative. Given the existing set_config UPSERT, the cleanest is: store.set_config(CONFIG_KEY_EMBED_DIM, &"0") then after first embed call `validate_or_store_dimension` will see 0 != real_dim... that still fails. **Resolution for Agent D**: if `--reset-dim` is passed, after deleting embeddings, call the Store trait's `set_config` to delete the key by setting it to empty string; then modify the reindex loop so it calls `embed_pending` which calls `validate_or_store_dimension` on the first embedding — but `validate_or_store_dimension` checks `None` (not found) to store. Empty string will be found (Some("")) and parse to 0. **Agent D should**: if `--reset-dim`, call `store.set_config(commitmux_embed::CONFIG_KEY_EMBED_DIM, "")` to zero it out, then `validate_or_store_dimension` will get Some("") → parse fails → stored_dim=0 → 0 != actual_dim → still errors. The cleanest fix is for Agent D to document this caveat and simply not implement `--reset-dim` automatically reindexing; instead print a follow-up instruction: "Run `commitmux config set embed.dimension <N>` after reindexing with the new model's dimension, or use `commitmux config set embed.dimension <N>` before reindexing."  **Final decision**: `--reset-dim` calls `store.set_config(CONFIG_KEY_EMBED_DIM, "RESET")` and Agent D also patches `validate_or_store_dimension` to treat "RESET" as None (clears and stores fresh). OR simplest: just always delete the config key when --reset-dim by using a raw SQL approach via a new `delete_config` method. See Known Issues/Agent D notes for the resolution chosen at implementation time.

4. Call `embed_pending` for each repo (using existing tokio::runtime block_on pattern from Sync handler)
5. Print per-repo summary

---

## File Ownership

```yaml type=impl-file-ownership
| File                              | Agent | Wave | Depends On    |
|-----------------------------------|-------|------|---------------|
| crates/types/src/lib.rs           | A     | 1    | —             |
| crates/store/src/queries.rs       | A     | 1    | —             |
| src/main.rs                       | B     | 1    | —             |
| crates/mcp/src/lib.rs             | C     | 2    | A             |
| crates/embed/src/lib.rs           | C     | 2    | A             |
| crates/ingest/src/lib.rs          | C     | 2    | A             |
| src/main.rs                       | D     | 2    | A, B (merged) |
```

Note: `src/main.rs` appears in both Wave 1 (Agent B) and Wave 2 (Agent D). This is safe because
waves are strictly sequential — Agent D works against the post-Wave-1-merge version of `src/main.rs`
that already contains Agent B's InstallMemoryHook additions.

---

## Wave Structure

```yaml type=impl-wave-structure
Wave 1: [A] [B]          <- 2 parallel agents (foundation)
           | (A+B complete, merge both)
Wave 2:   [C] [D]        <- 2 parallel agents (depend on Wave 1)
```

---

## Wave 1

Wave 1 delivers two independent foundations: the new Store trait method needed by reindex and
the install-memory-hook CLI command (which is pure file I/O, no store dependency). Both agents
work in completely disjoint files and can run simultaneously.

### Agent A - Store trait: delete_embeddings_for_repo

**Role:** Add `delete_embeddings_for_repo(repo_id: i64) -> Result<()>` to the `Store` trait and
implement it in SqliteStore.

**Context:** The `commitmux reindex` command (implemented by Agent D in Wave 2) needs to delete
all existing embeddings for a repo before re-embedding. The embedding data lives in two tables:
- `commit_embed_map` (the key/ID map, one row per commit with an embedding): `embed_id`, `repo_id`, `sha`
- `commit_embeddings` (the sqlite-vec virtual table): stores float vectors indexed by `embed_id`

The delete must go in reverse order: delete from `commit_embeddings` first (by embed_id), then
delete from `commit_embed_map`. The pattern for looking up embed_ids and deleting from the vec0
table already appears in `store_embedding` in `crates/store/src/queries.rs` (lines 880–897).

**Files to modify:**
- `/Users/dayna.blackwell/code/commitmux/crates/types/src/lib.rs` — add method to `Store` trait
- `/Users/dayna.blackwell/code/commitmux/crates/store/src/queries.rs` — add `impl Store for SqliteStore` implementation

**Do NOT touch:**
- `crates/embed/src/lib.rs` — Agent C handles the NullStore stub in Wave 2
- `crates/ingest/src/lib.rs` — Agent C handles the MockStore stub in Wave 2
- `crates/mcp/src/lib.rs` — Agent C handles the StubStore stub in Wave 2
- `src/main.rs` — Agent B and D own this file

**Interface to implement:**

```rust
// In Store trait (crates/types/src/lib.rs):
fn delete_embeddings_for_repo(&self, repo_id: i64) -> Result<()>;
```

Implementation logic for SqliteStore:
1. Lock the connection
2. Collect all `embed_id` values from `commit_embed_map WHERE repo_id = ?1` into a Vec<i64>
3. For each `embed_id`, execute `DELETE FROM commit_embeddings WHERE embed_id = ?1`
4. Execute `DELETE FROM commit_embed_map WHERE repo_id = ?1`
5. Return Ok(())

Important: The `commit_embeddings` table is a sqlite-vec `vec0` virtual table. These do not
support `DELETE ... WHERE embed_id IN (...)` with a subquery; delete each embed_id individually
in a loop, as the existing `store_embedding` code already does for single-row deletes.

**Test to add** in `crates/store/src/queries.rs` test module (near `test_count_embeddings_for_repo`):

```rust
#[test]
fn test_delete_embeddings_for_repo() {
    // Store a fake embedding, verify count=1, call delete, verify count=0
    // Use the existing make_store() helper and store_embedding pattern already
    // present in the test module
}
```

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build --workspace
cargo clippy -- -D warnings
cargo test -p commitmux-store -p commitmux-types
```

**Completion report format:**
```
status: complete
files_changed:
  - crates/types/src/lib.rs
  - crates/store/src/queries.rs
interface_deviations: []
notes: "Added delete_embeddings_for_repo. Deletes embed_ids individually via loop due to vec0 constraints."
```

---

### Agent B - CLI: commitmux install-memory-hook

**Role:** Add the `commitmux install-memory-hook` subcommand to `src/main.rs`. This command
writes a Claude Code Stop hook entry for `commitmux ingest-memory` into `~/.claude/settings.json`.

**Context:** Claude Code's `settings.json` supports a `hooks` map with lifecycle event keys
(`Stop`, `PostToolUse`, etc.). Each key maps to an array of hook group objects. The format is:
```json
{
  "hooks": {
    "Stop": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "..." }] }
    ]
  }
}
```
This command should add `commitmux ingest-memory` (with optional `--db <path>`) to the `Stop`
hooks array, guarding against duplicates.

**Files to modify:**
- `/Users/dayna.blackwell/code/commitmux/src/main.rs` — add `InstallMemoryHook` to `Commands` enum and add the match arm in `fn main()`

**Do NOT touch:** Any file in `crates/`.

**Implementation details:**

Add to the `Commands` enum:
```rust
#[command(about = "Install commitmux ingest-memory as a Claude Code Stop hook in ~/.claude/settings.json")]
InstallMemoryHook {
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
    db: Option<PathBuf>,
    #[arg(long = "claude-settings", help = "Path to Claude settings.json (default: ~/.claude/settings.json)")]
    claude_settings: Option<PathBuf>,
}
```

Handler logic in `fn main()`:
1. Resolve the settings file path: `claude_settings` flag or `~/.claude/settings.json`
2. Read the file if it exists; parse as `serde_json::Value` (use `json!({})` as fallback for
   missing file). On parse error: bail with a clear message including the file path.
3. Build the command string: `"commitmux ingest-memory"`. If `db` was passed, append
   ` --db <path>`: e.g. `format!("commitmux ingest-memory --db {}", db_path.display())`.
4. Duplicate check: traverse `value["hooks"]["Stop"]` as array; for each entry, check
   `entry["hooks"]` array for any `command` string containing `"commitmux ingest-memory"`.
   If found: print "commitmux ingest-memory is already registered as a Stop hook." and return Ok(()).
5. Insert: push a new hook group object into `value["hooks"]["Stop"]` (create the path if missing):
   ```json
   { "matcher": "", "hooks": [{ "type": "command", "command": "<cmd>" }] }
   ```
6. Write the updated JSON back to the file (pretty-printed with `serde_json::to_string_pretty`).
   Create parent directories if missing (`std::fs::create_dir_all`).
7. Print: `"Installed: commitmux ingest-memory will run after each Claude Code session.\nSettings: <path>"`

**serde_json is already a dependency** of the root crate (used in `src/main.rs` indirectly via
`commitmux_mcp`). Check `Cargo.toml` — if `serde_json` is not in `[dependencies]`, add it.
Looking at the current `Cargo.toml`, `serde_json = "1"` IS present.

**Test to add** (in `src/main.rs` test module, near `test_add_repo_persists_author_filter`):
- A unit test that calls the install-hook logic with a temp dir, verifies the JSON structure
  written, and verifies duplicate detection. Extract the handler logic into a small helper
  function `fn install_memory_hook(settings_path: &Path, db_display: Option<String>) -> Result<()>`
  for testability, and call it from the match arm.

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build --workspace
cargo clippy -- -D warnings
cargo test -p commitmux
```

**Completion report format:**
```
status: complete
files_changed:
  - src/main.rs
interface_deviations: []
notes: "Added InstallMemoryHook. Extracted install_memory_hook() helper for testability."
```

---

## Wave 2

Wave 2 runs after the Wave 1 merge. Agent C sees the new Store trait method (from A) and
Agent D sees both the new Store trait method (from A) and Agent B's InstallMemoryHook addition
to `src/main.rs`. Agents C and D work on disjoint files: C owns `crates/mcp/`, `crates/embed/`,
`crates/ingest/`; D owns `src/main.rs`.

### Agent C - MCP: commitmux_search_memory FTS fallback + mock stubs

**Role:** Wire the FTS fallback in `call_search_memory` in `crates/mcp/src/lib.rs`, and add
the `delete_embeddings_for_repo` stub to the three test mock stores (StubStore in mcp,
NullStore in embed, MockStore in ingest).

**Context:** After Wave 1 merges, `Store` trait has a new method `delete_embeddings_for_repo`.
Any struct implementing `Store` (including test mocks) will fail to compile without the new
method. Agent C must add the stub to all three test mock stores AND implement the FTS fallback.

**Files to modify:**
- `/Users/dayna.blackwell/code/commitmux/crates/mcp/src/lib.rs` — modify `call_search_memory`, add stub to `StubStore`
- `/Users/dayna.blackwell/code/commitmux/crates/embed/src/lib.rs` — add stub to `NullStore` (inside `#[cfg(test)]` block, around line 318)
- `/Users/dayna.blackwell/code/commitmux/crates/ingest/src/lib.rs` — add stub to `MockStore` (inside `#[cfg(test)]` block, around line 187)

**Do NOT touch:** `crates/types/src/lib.rs`, `crates/store/src/queries.rs`, `src/main.rs`

**Task 1: Add delete_embeddings_for_repo stubs**

In each of the three mock stores, add:
```rust
fn delete_embeddings_for_repo(&self, _repo_id: i64) -> Result<()> {
    Ok(())
}
```

Locations:
- `crates/mcp/src/lib.rs`: inside `impl Store for StubStore` (test module, around line 652 where `search_memory_fts` stub is)
- `crates/embed/src/lib.rs`: inside `impl Store for NullStore` (test module, after `search_memory_fts` stub around line 470)
- `crates/ingest/src/lib.rs`: inside `impl Store for MockStore` (test module, after `search_memory_fts` stub around line 187)

**Task 2: FTS fallback in call_search_memory**

Current code (around line 395–436 in `crates/mcp/src/lib.rs`):
```rust
fn call_search_memory(&self, arguments: &Value) -> Result<String, String> {
    // ... validation ...
    // Build embedder from store config
    let config = commitmux_embed::EmbedConfig::from_store(self.store.as_ref())
        .map_err(|e| format!("Embed config error: {e}"))?;
    let embedder = commitmux_embed::Embedder::new(&config);
    // Embed the query
    let embedding = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to build tokio runtime: {e}"))?
        .block_on(embedder.embed(&input.query))
        .map_err(|e| format!("Failed to embed query: {e}"))?;
    // Search
    let opts = MemorySearchOpts { ... };
    let results = self.store.search_memory(&embedding, &opts)
        .map_err(|e| format!("Memory search failed: {e}"))?;
    serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
}
```

Rewrite the embed+search section to:
```rust
// Try vector path
let vector_result = (|| -> Result<Vec<commitmux_types::MemoryMatch>, String> {
    let config = commitmux_embed::EmbedConfig::from_store(self.store.as_ref())
        .map_err(|e| format!("Embed config error: {e}"))?;
    let embedder = commitmux_embed::Embedder::new(&config);
    let embedding = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to build tokio runtime: {e}"))?
        .block_on(embedder.embed(&input.query))
        .map_err(|e| format!("{e}"))?;
    let opts = MemorySearchOpts {
        project: input.project.clone(),
        source_type: input.source_type.clone(),
        limit: input.limit,
    };
    self.store.search_memory(&embedding, &opts)
        .map_err(|e| format!("Memory search failed: {e}"))
})();

match vector_result {
    Ok(results) => serde_json::to_string_pretty(&results).map_err(|e| e.to_string()),
    Err(vector_err) => {
        // Fallback to FTS
        let fts_opts = commitmux_types::MemoryFtsSearchOpts {
            project: input.project.clone(),
            source_type: input.source_type.clone(),
            limit: input.limit,
        };
        let fts_results = self.store
            .search_memory_fts(&input.query, &fts_opts)
            .map_err(|e| format!("Memory search failed (vector unavailable, FTS fallback also failed): {e}"))?;
        // Wrap in a response that signals fallback mode
        let response = serde_json::json!({
            "fallback_mode": true,
            "reason": format!("Vector search unavailable ({}). Results are keyword-based (FTS).", vector_err),
            "results": fts_results
        });
        serde_json::to_string_pretty(&response).map_err(|e| e.to_string())
    }
}
```

**Test to add** in `crates/mcp/src/lib.rs` test module: A test using a modified StubStore
that makes `search_memory` return an error (simulating Ollama down) and verifies that
`call_search_memory` returns a JSON response with `"fallback_mode": true` and non-empty
`"results"` from the FTS path. Use the existing `make_server()` pattern but with a custom
stub store.

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build --workspace
cargo clippy -- -D warnings
cargo test -p commitmux-mcp -p commitmux-embed -p commitmux-ingest
```

**Completion report format:**
```
status: complete
files_changed:
  - crates/mcp/src/lib.rs
  - crates/embed/src/lib.rs
  - crates/ingest/src/lib.rs
interface_deviations: []
notes: "FTS fallback wraps results in {fallback_mode, reason, results} JSON. Three mock stubs added."
```

---

### Agent D - CLI: commitmux reindex

**Role:** Add the `commitmux reindex` subcommand to `src/main.rs`. This command deletes and
rebuilds embeddings for one or all repos using the `delete_embeddings_for_repo` Store method
(added by Agent A) and the existing `embed_pending` function from `commitmux_embed`.

**Context:** You are working against the post-Wave-1-merge `src/main.rs`, which already contains
Agent B's `InstallMemoryHook` command. The `Store` trait now has `delete_embeddings_for_repo`.
The `validate_or_store_dimension` function in `crates/embed/src/lib.rs` will return an error if
the stored dimension mismatches — the `--reset-dim` flag must clear the stored dimension.

**Important note on `--reset-dim` and dimension clearing:** The `set_config` method uses
`INSERT ... ON CONFLICT DO UPDATE`, so there is no `delete_config` method on the Store trait.
To reset the dimension, call:
```rust
store.set_config(commitmux_embed::CONFIG_KEY_EMBED_DIM, "0")?;
```
Then `validate_or_store_dimension` will see stored="0", parse to 0, compare to actual_dim (e.g.
768) → 0 != 768 → **still fails**. This is the design tension noted in the Interface Contracts
section. **Resolution for Agent D**: implement `--reset-dim` by calling
`store.set_config(commitmux_embed::CONFIG_KEY_EMBED_DIM, "")` and then trust that
`validate_or_store_dimension` treats `"".parse().unwrap_or(0)` → `stored_dim = 0`. Since 0 will
never equal a real embedding dim, validation will error again. The cleanest working approach:
**skip calling `validate_or_store_dimension` on the first batch when `reset_dim` is true**.
Since `embed_pending` in `crates/embed/src/lib.rs` calls `validate_or_store_dimension`
internally, Agent D should instead: if `--reset-dim`, delete the config key by calling
`store.set_config(CONFIG_KEY_EMBED_DIM, "")` and then patch the embed flow to treat empty string
as "not found". **Simplest correct approach that requires no changes to embed crate**: call
`store.set_config(commitmux_embed::CONFIG_KEY_EMBED_DIM, &actual_dim.to_string())` where
`actual_dim` is obtained by making one test embedding call before the batch loop, then setting
the config to the returned embedding's length. This requires one extra embedding call. If this
is too complex, Agent D should document the limitation and not implement `--reset-dim`, instead
printing: "To change embedding models, run: `commitmux config set embed.model <new-model>` then
`commitmux sync --embed-only`."

**Implementation decision**: Implement `--reset-dim` by:
1. Building embedder, embed a short probe string (e.g. `"probe"`) to discover actual dim
2. Call `store.set_config(CONFIG_KEY_EMBED_DIM, &probe_dim.to_string())` to overwrite
3. Then proceed with `embed_pending` loop (which will now pass dimension validation)

If Ollama is down during `--reset-dim`, fail with clear message.

**Files to modify:**
- `/Users/dayna.blackwell/code/commitmux/src/main.rs` — add `Reindex` to `Commands` enum and match arm

**Do NOT touch:** Any crate files.

**Add to Commands enum:**
```rust
#[command(about = "Delete and rebuild embeddings for one or all repositories")]
Reindex {
    #[arg(long, help = "Repository name to reindex (default: all repos with embed_enabled)")]
    repo: Option<String>,
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
    db: Option<PathBuf>,
    #[arg(long = "reset-dim", help = "Reset stored embedding dimension (use when switching models)")]
    reset_dim: bool,
}
```

**Handler logic:**
1. Open store
2. Resolve repos: if `--repo` provided, look up by name (error if not found); else list all repos
   filtered to `embed_enabled == true`
3. If no repos with embeddings enabled: print warning and return
4. `--reset-dim` handling (only if flag set):
   - Build embedder via `EmbedConfig::from_store`
   - Embed probe string `"probe"` via tokio block_on to discover actual dimension
   - Call `store.set_config(commitmux_embed::CONFIG_KEY_EMBED_DIM, &dim.to_string())?`
   - Print: `"Reset embedding dimension to {dim}."`
5. For each repo:
   - Print: `"Reindexing '{}'... deleting existing embeddings"`, repo.name
   - Call `store.delete_embeddings_for_repo(repo.repo_id)?`
   - Print: `"  Embeddings deleted. Re-embedding..."`
   - Use tokio block_on pattern (copy from Sync handler) to call `embed_pending(&store, &embedder, repo.repo_id, 50)`
   - Print per-repo summary: `"  Done: {} embedded, {} failed"`, esummary.embedded, esummary.failed

Use the exact same tokio runtime construction pattern as the `Sync` command handler:
```rust
let rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .expect("tokio runtime");
```

**Test to add** (in `src/main.rs` test module):
A test that creates a temp store, adds a repo, calls the reindex handler logic with no embeddings
present (empty base case — should complete without error). Full integration test with actual
embedding requires Ollama; keep the test to the "no embed_enabled repos" early-return path and
the "delete then count = 0" path.

**Verification gate:**
```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build --workspace
cargo clippy -- -D warnings
cargo test -p commitmux
```

**Completion report format:**
```
status: complete
files_changed:
  - src/main.rs
interface_deviations: []
notes: "Added Reindex command. --reset-dim probes Ollama for actual dim before resetting config."
```

---

## Wave Execution Loop

After Wave 1 completes, run the Orchestrator Post-Merge Checklist (Wave 1 section), then
launch Wave 2 agents. After Wave 2 completes, run the checklist again (Wave 2 section).

The merge procedure is in `saw-merge.md`. Key principle: do not launch Wave 2 until
`cargo build --workspace && cargo test --workspace` passes cleanly on the merged Wave 1 result.
Agent C in Wave 2 requires the new Store method to compile; a broken Wave 1 merge would
cascade into Agent C's worktree.

---

## Orchestrator Post-Merge Checklist

### After Wave 1 (Agents A + B):

- [ ] Read Agent A and Agent B completion reports — confirm all `status: complete`; if any `partial` or `blocked`, stop and resolve
- [ ] Conflict prediction — `src/main.rs` (Agent B) and `crates/types/src/lib.rs`, `crates/store/src/queries.rs` (Agent A) are disjoint; no expected conflicts
- [ ] Review `interface_deviations` — if Agent A changed the signature of `delete_embeddings_for_repo`, update Agent C and Agent D prompts before launching Wave 2
- [ ] Merge Agent A: `git merge --no-ff wave1-agent-A -m "Merge wave1-agent-A: Store::delete_embeddings_for_repo"`
- [ ] Merge Agent B: `git merge --no-ff wave1-agent-B -m "Merge wave1-agent-B: commitmux install-memory-hook"`
- [ ] Worktree cleanup: `git worktree remove <path>` + `git branch -d <branch>` for each
- [ ] Post-merge verification:
      - [ ] Linter auto-fix pass: `cargo fmt --all` (then commit any formatting changes)
      - [ ] `cargo build --workspace && cargo clippy -- -D warnings && cargo test --workspace`
- [ ] E20 stub scan: collect files from completion reports; run `bash "${CLAUDE_SKILL_DIR}/scripts/scan-stubs.sh" crates/types/src/lib.rs crates/store/src/queries.rs src/main.rs`; append output as `## Stub Report — Wave 1`
- [ ] E21 quality gates: run `cargo build --workspace` (required) and `cargo test --workspace` (required)
- [ ] Fix any cascade failures before launching Wave 2
- [ ] Tick Wave 1 status checkboxes
- [ ] Launch Wave 2

### After Wave 2 (Agents C + D):

- [ ] Read Agent C and Agent D completion reports — confirm all `status: complete`
- [ ] Conflict prediction — `crates/mcp/src/lib.rs`, `crates/embed/src/lib.rs`, `crates/ingest/src/lib.rs` (Agent C) and `src/main.rs` (Agent D) are disjoint; no expected conflicts
- [ ] Review `interface_deviations`
- [ ] Merge Agent C: `git merge --no-ff wave2-agent-C -m "Merge wave2-agent-C: commitmux_search_memory FTS fallback"`
- [ ] Merge Agent D: `git merge --no-ff wave2-agent-D -m "Merge wave2-agent-D: commitmux reindex command"`
- [ ] Worktree cleanup for each
- [ ] Post-merge verification:
      - [ ] Linter auto-fix pass: `cargo fmt --all`
      - [ ] `cargo build --workspace && cargo clippy -- -D warnings && cargo test --workspace`
- [ ] E20 stub scan: `bash "${CLAUDE_SKILL_DIR}/scripts/scan-stubs.sh" crates/mcp/src/lib.rs crates/embed/src/lib.rs crates/ingest/src/lib.rs src/main.rs`
- [ ] E21 quality gates: run all gates marked `required: true`
- [ ] Feature-specific steps:
      - [ ] Smoke-test `commitmux install-memory-hook --help` and `commitmux reindex --help` on the built binary
      - [ ] Verify `commitmux install-memory-hook` writes correct JSON to a temp settings path
      - [ ] Verify `commitmux reindex --help` shows `--reset-dim` flag
- [ ] Commit: `git commit -m "feat: FTS fallback, reindex command, install-memory-hook"`
- [ ] Launch next wave (or mark complete)

---

### Status

| Wave | Agent | Description | Status |
|------|-------|-------------|--------|
| 1 | A | Store::delete_embeddings_for_repo (types + store) | TO-DO |
| 1 | B | commitmux install-memory-hook CLI | TO-DO |
| 2 | C | commitmux_search_memory FTS fallback + mock stubs | TO-DO |
| 2 | D | commitmux reindex CLI | TO-DO |
| — | Orch | Post-merge integration + binary smoke test | TO-DO |
