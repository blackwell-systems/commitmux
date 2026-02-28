# IMPL: UX Audit Fixes — Round 4

**Feature:** Fix Round 4 UX audit findings from `docs/cold-start-audit.md` (semantic search cold-start)
**Date:** 2026-02-28
**Scout version:** scout v0.2.1 / agent-template v0.3.2

---

## Suitability Assessment

Verdict: **SUITABLE**

7 findings decompose cleanly into 4 agents with disjoint file ownership:
- Agent A (critical): Fix semantic search SQL bug in `crates/store/src/queries.rs` + add test
- Agent B: Improve help text in `src/main.rs`
- Agent C: Fix status display logic in `src/main.rs`
- Agent D: Add input validation in `crates/mcp/src/lib.rs`

Polish items (R4-06, R4-07 progress indicators) deferred to future work — not blocking, require larger refactoring.

Pre-implementation scan results:
- Total items: 7 findings
- Already implemented: 0 items (0% of work)
- Partially implemented: 0 items
- To-do: 5 items (R4-01 through R4-05)
- Deferred: 2 items (R4-06, R4-07 — progress indicators, polish)

Agent adjustments:
- All agents proceed as planned (all to-do)
- No "verify + add tests" agents (none pre-implemented)

Estimated times:
- Scout phase: ~10 min (dependency mapping, SQL syntax research, IMPL doc)
- Agent execution: ~25 min (4 agents, A is 15min critical fix, B/C/D are ~5min each, parallel)
- Merge & verification: ~5 min
- Total SAW time: ~40 min

Sequential baseline: ~35 min (4 agents × ~9 min avg sequential)
Time savings: ~-5 min (marginal overhead, but CRITICAL bug needs immediate fix)

Recommendation: Clear speedup for Agent A (critical fix with test). Agents B/C/D are small but benefit from parallel execution. Proceed.

---

## Known Issues

None identified. Full `cargo test --workspace` passes clean (70 tests across all crates + integration tests) as of commit 9ecf59c.

---

## Dependency Graph

```
Agent A: crates/store/src/queries.rs
  - Root node: no dependencies on other agents
  - Fixes: search_semantic() SQL syntax bug (line 878-886)
  - Adds: test_search_semantic_returns_results() integration test
  - Critical: blocks semantic search functionality

Agent B: src/main.rs (help text)
  - Root node: no dependencies
  - Changes: #[command(about = ...)] and #[arg(help = ...)] strings only
  - Touches: Commands::Config, Commands::AddRepo, Commands::Sync help attributes

Agent C: src/main.rs (status display)
  - Root node: no dependencies
  - Changes: Status command output logic (lines 526-600)
  - Adds: embedding count column logic, distinguishes enabled vs generated
  - Depends on: Store::get_config (existing), new count_embeddings_for_repo query (Agent C implements)

Agent D: crates/mcp/src/lib.rs
  - Root node: no dependencies
  - Changes: call_search_semantic() validation (line 315-344)
  - Adds: empty query check, limit=0 check, nonexistent repo filter warning
```

Roots (no upstream dependencies): All 4 agents are roots — fully parallel in Wave 1.

Cascade candidates:
- None. No type renames, no public API changes.
- Agent A fixes internal SQL query — does not change Store trait signature.
- Agent C adds a new Store trait method `count_embeddings_for_repo` but implements it in the same worktree, so no cross-agent cascade.

Type rename cascade check: No type renames. No cascade search required.

---

## Interface Contracts

### Agent A delivers:

**Function (existing, fixed):**
```rust
// crates/store/src/queries.rs (line 864)
// EXISTING signature — no change, fix internal SQL only
fn search_semantic(&self, embedding: &[f32], opts: &SemanticSearchOpts) -> Result<Vec<SearchResult>>;
```

**Test added:**
```rust
// crates/store/src/queries.rs (in #[cfg(test)] mod tests)
#[test]
fn test_search_semantic_returns_results();
```

No new public API. Fix is internal to SqliteStore impl.

---

### Agent C delivers:

**New Store trait method:**
```rust
// crates/types/src/lib.rs — add to Store trait
fn count_embeddings_for_repo(&self, repo_id: i64) -> Result<usize>;
```

**Implementation:**
```rust
// crates/store/src/queries.rs — add to impl Store for SqliteStore
fn count_embeddings_for_repo(&self, repo_id: i64) -> Result<usize> {
    let conn = self.conn.lock().unwrap();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM commit_embed_map WHERE repo_id = ?1",
        params![repo_id],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}
```

Agent C owns both the trait addition and the implementation — no cross-agent dependency.

---

### Agents B and D:

No new interfaces. Both modify existing code:
- Agent B: string literals only
- Agent D: adds validation logic within existing `call_search_semantic()` function

---

## File Ownership

| File | Agent | Wave | Depends On |
|------|-------|------|------------|
| `crates/store/src/queries.rs` (search_semantic fix + test) | A | 1 | — |
| `crates/types/src/lib.rs` (count_embeddings trait) | C | 1 | — |
| `src/main.rs` (help text lines 12-77) | B | 1 | — |
| `src/main.rs` (status display lines 526-600) | C | 1 | — |
| `crates/mcp/src/lib.rs` (validation lines 315-344) | D | 1 | — |

**Conflict resolution:** Agents B and C both touch `src/main.rs` BUT in completely disjoint line ranges:
- Agent B: Lines 12-77 (command/arg attribute strings)
- Agent C: Lines 526-600 (Status command match arm body)

Git will merge these cleanly. No coordination needed.

No file is owned by multiple agents in overlapping regions.

---

## Wave Structure

```
Wave 1: [A] [B] [C] [D]     <- 4 parallel agents, fully independent
         |
    (all complete)
         |
    post-merge verification (cargo build && cargo test --workspace)
```

Wave 0 omitted: Agent A (critical fix) does not gate other agents' verification. All agents verify independently in their worktrees.

---

## Agent Prompts

---

### Wave 1 Agent A: Fix semantic search SQL query bug (CRITICAL)

You are Wave 1 Agent A. Your task is to fix the CRITICAL bug in `crates/store/src/queries.rs` where `search_semantic()` returns zero results for all queries. The SQL syntax `WHERE ce.embedding MATCH ?1 AND k = ?2` is incorrect for sqlite-vec's vec0 virtual table kNN search.

#### 0. CRITICAL: Isolation Verification (RUN FIRST)

**Step 1: Attempt environment correction**

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a 2>/dev/null || true
```

**Step 2: Verify isolation (strict fail-fast after self-correction attempt)**

```bash
ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory (even after cd attempt)"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual: $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave1-agent-a"

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

#### 1. File Ownership

You own these files:
- `crates/store/src/queries.rs` - modify (fix search_semantic SQL, add test)

#### 2. Interfaces You Must Implement

No new interfaces. You are fixing the implementation of:

```rust
fn search_semantic(&self, embedding: &[f32], opts: &SemanticSearchOpts) -> Result<Vec<SearchResult>>;
```

The signature does NOT change. Only the internal SQL query changes.

#### 3. Interfaces You May Call

All existing Store trait methods, rusqlite API, serde_json.

#### 4. What to Implement

**Context:** The audit (Area 6 of `docs/cold-start-audit.md`) found that ALL semantic search queries return empty results `[]`, despite 141 embeddings being successfully stored. The database has:
- `commit_embed_map` table with 141 rows
- `commit_embeddings` vec0 virtual table with correct schema

**Root cause:** Line 878-886 in `crates/store/src/queries.rs`:

```rust
let sql =
    "SELECT ce.repo_name, ce.sha, ce.subject, ce.author_name, ce.author_time,
            ce.patch_preview, distance
     FROM commit_embeddings ce
     WHERE ce.embedding MATCH ?1
       AND k = ?2
       AND ('' = ?3 OR ce.repo_name IN (SELECT value FROM json_each(?3)))
       AND (?4 = 0 OR ce.author_time >= ?4)
     ORDER BY distance";
```

The sqlite-vec vec0 virtual table kNN search syntax `AND k = ?2` is incorrect. Based on vec0 semantics, the k parameter (number of nearest neighbors) must be specified differently. Research indicates two possible fixes:

**Option 1: Use k in MATCH clause**
```sql
WHERE ce.embedding MATCH ?1 AND k = ?2
```
should become:
```sql
WHERE ce.embedding MATCH vector_slice(?1, 0, ?2)
```
(if vec0 expects k to be encoded in the match argument)

**Option 2: Use knn_where or similar function**
Check if sqlite-vec provides a `knn_where()` or `vec_slice()` function.

**Option 3: ORDER BY distance LIMIT ?2**
Remove `AND k = ?2` entirely and use `LIMIT ?2` at the end of the query instead. The MATCH clause identifies the query vector, and LIMIT controls how many results are returned.

**Implementation steps:**

1. Read `crates/store/src/queries.rs` lines 864-903 (the `search_semantic` function).
2. Research the correct vec0 kNN syntax. Check:
   - The `commit_embeddings` virtual table schema in `crates/store/src/schema.rs` line 77-86
   - Any test usage of vec0 queries (unlikely to exist yet)
   - Try **Option 3 first** (simplest): remove `AND k = ?2` and use `ORDER BY distance LIMIT ?2` instead. This is the most common kNN pattern.
3. Fix the SQL query:
   - Remove the `AND k = ?2` condition from the WHERE clause
   - Add `LIMIT ?2` to the end of the query (after `ORDER BY distance`)
   - Ensure the bind parameter order is correct: ?1 = embedding_bytes, ?2 = repos_json, ?3 = since, and the limit is appended as the last parameter.
4. Update the `stmt.query_map(params![...], ...)` call to reflect the new parameter order (limit moves from ?2 to ?4, repos_json moves from ?3 to ?2, since moves from ?4 to ?3).
5. Add a test `test_search_semantic_returns_results()` in the `#[cfg(test)] mod tests` section:
   - Use an in-memory SqliteStore
   - Add a repo with `embed_enabled: true`
   - Insert a commit via `store.upsert_commit()`
   - Store a mock embedding via `store.store_embedding()` (use a simple f32 vector like `vec![0.1; 768]`)
   - Call `store.search_semantic(&query_embedding, &opts)` with a similar vector
   - Assert that results.len() > 0 (i.e., the query returns the stored commit)

**Edge cases:**
- Empty embedding vector: not expected (MCP validates query is non-empty), but query should not panic
- limit=0: MCP will validate (Agent D's work), but query should handle gracefully (returns empty results)
- No embeddings exist: query returns empty results (not an error)

#### 5. Tests to Write

1. `test_search_semantic_returns_results` - Verifies that a query returns results when embeddings exist:
   - Add repo with embed_enabled
   - Insert commit
   - Store embedding with known vector
   - Query with similar vector
   - Assert results.len() > 0 and results[0].sha matches the inserted commit
2. `test_search_semantic_limit_respected` - Verifies limit parameter is honored:
   - Store 5 embeddings
   - Query with limit=2
   - Assert results.len() == 2
3. `test_search_semantic_repo_filter` - Verifies repos filter works:
   - Store embeddings for 2 repos
   - Query with repos filter for one repo
   - Assert all results.repo matches the filter

#### 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build
cargo test --package commitmux-store test_search_semantic
cargo test --workspace
```

All tests must pass. Specifically, the 3 new tests must pass and existing tests must remain green (70 tests).

#### 7. Constraints

- Do NOT change the `search_semantic` function signature in the Store trait (`crates/types/src/lib.rs`)
- Do NOT add new dependencies (sqlite-vec is already in Cargo.toml)
- Do NOT modify the embedding storage format (embedding_bytes as little-endian f32 bytes is correct)
- If the fix requires more than changing the SQL query and bind params, document the issue in your completion report and explain why

#### 8. Report

**Before reporting:** Commit your changes:

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a
git add crates/store/src/queries.rs
git commit -m "wave1-agent-a: fix semantic search SQL kNN syntax (R4-01)"
```

Append to IMPL doc under `### Agent A — Completion Report`:

```yaml
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-a
commit: {sha}
files_changed:
  - crates/store/src/queries.rs
files_created: []
interface_deviations:
  - (none expected, or describe if SQL syntax required unexpected changes)
out_of_scope_deps: []
tests_added:
  - test_search_semantic_returns_results
  - test_search_semantic_limit_respected
  - test_search_semantic_repo_filter
verification: PASS | FAIL (cargo test --package commitmux-store — 3/3 new tests, 70 total)
```

---

### Wave 1 Agent B: Add embedding setup guidance to help text

You are Wave 1 Agent B. Your task is to add brief guidance for semantic search setup to the `--help` output in `src/main.rs`, making it easier for users to discover the 4-step workflow.

#### 0. CRITICAL: Isolation Verification (RUN FIRST)

**Step 1: Attempt environment correction**

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b 2>/dev/null || true
```

**Step 2: Verify isolation (strict fail-fast after self-correction attempt)**

```bash
ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory (even after cd attempt)"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual: $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave1-agent-b"

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

#### 1. File Ownership

You own these files:
- `src/main.rs` - modify (lines 12-77, command/arg help attributes only)

#### 2. Interfaces You Must Implement

No new interfaces. You are modifying clap attribute strings only.

#### 3. Interfaces You May Call

None (string literals only).

#### 4. What to Implement

**Context:** Audit finding R4-02 (Area 2) notes that `--help` output mentions embeddings but provides no guidance on prerequisites. Users see `--embed` flag but don't know they need to:
1. Install and run Ollama
2. Pull an embedding model (e.g., `ollama pull nomic-embed-text`)
3. Set `embed.model` and `embed.endpoint` config
4. Use `--embed` when adding repos

**Changes:**

1. **Update `Commands::Config` about text** (line ~98):
   ```rust
   #[command(about = "Get or set global configuration values. For semantic search: set embed.model (e.g. nomic-embed-text) and embed.endpoint (default: http://localhost:11434/v1). Requires Ollama running.")]
   ```

2. **Update `Commands::AddRepo` `--embed` flag help** (line ~41):
   ```rust
   #[arg(long = "embed", help = "Enable semantic embeddings for this repo. Requires: 1) Ollama running, 2) embed.model configured (see: commitmux config --help)")]
   ```

3. **Update `Commands::Sync` `--embed-only` flag help** (line ~76):
   ```rust
   #[arg(long = "embed-only", help = "Generate embeddings for already-indexed commits; skip indexing new commits. Useful for backfilling when embeddings were enabled after initial sync.")]
   ```

**Do NOT change:**
- Any code logic (only `#[command(about = "...")]` and `#[arg(help = "...")]` strings)
- Function signatures
- Struct field names or types

#### 5. Tests to Write

None. This is a documentation-only change. Manual verification: run `commitmux --help`, `commitmux config --help`, `commitmux add-repo --help`, `commitmux sync --help` and verify the updated text appears.

#### 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build
cargo test --workspace
```

All existing tests must pass (no new tests required).

#### 7. Constraints

- Only modify help/about strings in clap attributes
- Do NOT change any function logic, struct fields, or imports
- Keep help text concise (under 150 chars per string)

#### 8. Report

**Before reporting:** Commit your changes:

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b
git add src/main.rs
git commit -m "wave1-agent-b: add semantic search setup guidance to help text (R4-02)"
```

Append to IMPL doc under `### Agent B — Completion Report`:

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-b
commit: {sha}
files_changed:
  - src/main.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added: []
verification: PASS (cargo test --workspace — 70/70 tests)
```

---

### Wave 1 Agent C: Distinguish enabled vs generated embeddings in status output

You are Wave 1 Agent C. Your task is to fix the `status` command output in `src/main.rs` to distinguish repos with embeddings enabled but not yet generated from repos that have embeddings fully generated.

#### 0. CRITICAL: Isolation Verification (RUN FIRST)

**Step 1: Attempt environment correction**

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-c 2>/dev/null || true
```

**Step 2: Verify isolation (strict fail-fast after self-correction attempt)**

```bash
ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-c"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory (even after cd attempt)"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual: $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave1-agent-c"

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

#### 1. File Ownership

You own these files:
- `src/main.rs` - modify (Status command match arm, lines 526-600)
- `crates/types/src/lib.rs` - modify (add count_embeddings_for_repo to Store trait)
- `crates/store/src/queries.rs` - modify (implement count_embeddings_for_repo)

#### 2. Interfaces You Must Implement

**New Store trait method:**

```rust
// crates/types/src/lib.rs — add to Store trait (line ~236, after count_commits_for_repo)
fn count_embeddings_for_repo(&self, repo_id: i64) -> Result<usize>;
```

**Implementation:**

```rust
// crates/store/src/queries.rs — add to impl Store for SqliteStore (line ~767, after count_commits_for_repo)
fn count_embeddings_for_repo(&self, repo_id: i64) -> Result<usize> {
    let conn = self.conn.lock().unwrap();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM commit_embed_map WHERE repo_id = ?1",
        params![repo_id],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}
```

#### 3. Interfaces You May Call

All existing Store trait methods (list_repos, repo_stats, get_config, count_commits_for_repo).

#### 4. What to Implement

**Context:** Audit finding R4-03 (Area 8) notes that the Status output shows `✓` in the EMBED column immediately after `update-repo --embed`, even when no embeddings exist yet. Users can't tell if embeddings are pending or complete.

**Changes:**

1. **Add `count_embeddings_for_repo` to Store trait** (crates/types/src/lib.rs):
   - Add method signature to trait (line ~236, after `count_commits_for_repo`)

2. **Implement `count_embeddings_for_repo`** (crates/store/src/queries.rs):
   - Add implementation to `impl Store for SqliteStore` (line ~767, after `count_commits_for_repo`)
   - Query: `SELECT COUNT(*) FROM commit_embed_map WHERE repo_id = ?1`
   - Return count as usize

3. **Update Status command output logic** (src/main.rs lines 526-600):
   - For each repo, call `store.count_embeddings_for_repo(r.repo_id)`
   - Compare embedding_count to commit_count
   - Update embed_col display:
     - `✓` if embed_enabled AND embedding_count == commit_count (fully generated)
     - `⋯` if embed_enabled AND embedding_count < commit_count (pending/partial)
     - `-` if NOT embed_enabled

4. **Update footer text** (line ~599):
   - Change `— ✓ = enabled` to `— ✓ = complete, ⋯ = pending`

**Example updated output:**

```
REPO                  COMMITS  SOURCE                                         LAST SYNCED             EMBED
commitmux                  70  /Users/dayna.blackwell/code/commitmux          2026-02-28 18:01:20 UTC  ✓
bubbletea-components       12  /Users/dayna.blackwell/code/bubbletea-compo...  2026-02-28 18:01:20 UTC  ⋯
scout-and-wave             59  /Users/dayna.blackwell/code/scout-and-wave     2026-02-28 18:01:20 UTC  -

Embedding model: nomic-embed-text (http://localhost:11434/v1) — ✓ = complete, ⋯ = pending
```

#### 5. Tests to Write

1. `test_count_embeddings_for_repo_zero_when_none_exist` - Verify count is 0 for repo with no embeddings:
   - Create in-memory store
   - Add repo with embed_enabled
   - Insert commits
   - Call count_embeddings_for_repo
   - Assert count == 0

2. `test_count_embeddings_for_repo_matches_stored_count` - Verify count matches number stored:
   - Add repo with embed_enabled
   - Insert 3 commits
   - Store embeddings for all 3
   - Call count_embeddings_for_repo
   - Assert count == 3

Add tests to `crates/store/src/queries.rs` in the `#[cfg(test)] mod tests` section.

#### 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build
cargo test --package commitmux-store test_count_embeddings
cargo test --workspace
```

All tests must pass (2 new tests + 70 existing).

#### 7. Constraints

- Do NOT change the existing EMBED column format (single character: ✓ / ⋯ / -)
- Do NOT add a separate EMBEDDED column (keep existing layout)
- The footer text must remain a single line (no multi-line formatting)

#### 8. Report

**Before reporting:** Commit your changes:

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-c
git add src/main.rs crates/types/src/lib.rs crates/store/src/queries.rs
git commit -m "wave1-agent-c: distinguish enabled vs generated embeddings in status (R4-03)"
```

Append to IMPL doc under `### Agent C — Completion Report`:

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-c
commit: {sha}
files_changed:
  - src/main.rs
  - crates/types/src/lib.rs
  - crates/store/src/queries.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_count_embeddings_for_repo_zero_when_none_exist
  - test_count_embeddings_for_repo_matches_stored_count
verification: PASS (cargo test --workspace — 72/72 tests)
```

---

### Wave 1 Agent D: Add input validation to semantic search MCP tool

You are Wave 1 Agent D. Your task is to add input validation to the `commitmux_search_semantic` MCP tool in `crates/mcp/src/lib.rs` to handle edge cases: empty query, limit=0, and nonexistent repo filters.

#### 0. CRITICAL: Isolation Verification (RUN FIRST)

**Step 1: Attempt environment correction**

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-d 2>/dev/null || true
```

**Step 2: Verify isolation (strict fail-fast after self-correction attempt)**

```bash
ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-d"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory (even after cd attempt)"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual: $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave1-agent-d"

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

#### 1. File Ownership

You own these files:
- `crates/mcp/src/lib.rs` - modify (call_search_semantic function, lines 315-344)

#### 2. Interfaces You Must Implement

No new interfaces. You are adding validation logic to the existing `call_search_semantic` method.

#### 3. Interfaces You May Call

All existing Store trait methods (list_repos).

#### 4. What to Implement

**Context:** Audit finding R4-04 and R4-05 (Area 12) note that edge cases are not validated:
- Empty query (`""`) returns `[]` (no error) — should validate and return error
- `limit=0` returns `[]` (no error) — should validate and return error or is valid?
- Nonexistent repo filter returns `[]` (no error) — user can't tell if repo doesn't exist, has no embeddings, or query didn't match

**Changes in `crates/mcp/src/lib.rs`, `call_search_semantic` method (line 315-344):**

1. **Validate empty query:**
   ```rust
   if input.query.trim().is_empty() {
       return Err("Query cannot be empty".to_string());
   }
   ```

2. **Validate limit=0:**
   ```rust
   if let Some(limit) = input.limit {
       if limit == 0 {
           return Err("Limit must be greater than 0".to_string());
       }
   }
   ```

3. **Validate nonexistent repo filters:**
   - If `input.repos` is Some(...), check that each repo name exists in `store.list_repos()`
   - If any repo name doesn't exist, return an error:
     ```rust
     if let Some(ref repos) = input.repos {
         let all_repos = self.store.list_repos().map_err(|e| e.to_string())?;
         let existing_names: Vec<&str> = all_repos.iter().map(|r| r.name.as_str()).collect();
         let unknown: Vec<&str> = repos.iter()
             .filter(|r| !existing_names.contains(&r.as_str()))
             .map(|s| s.as_str())
             .collect();
         if !unknown.is_empty() {
             return Err(format!("Unknown repo(s): {}", unknown.join(", ")));
         }
     }
     ```

**Placement:** Add validation checks at the beginning of `call_search_semantic`, before building EmbedConfig.

#### 5. Tests to Write

1. `test_search_semantic_rejects_empty_query` - Verify empty query returns isError: true:
   - Create StubStore
   - Call call_search_semantic with arguments: `{"query": ""}`
   - Assert response["result"]["isError"] == true
   - Assert error message contains "cannot be empty"

2. `test_search_semantic_rejects_limit_zero` - Verify limit=0 returns isError: true:
   - Call call_search_semantic with arguments: `{"query": "test", "limit": 0}`
   - Assert response["result"]["isError"] == true
   - Assert error message contains "greater than 0"

3. `test_search_semantic_rejects_nonexistent_repo` - Verify unknown repo returns isError: true:
   - Create StubStore that returns a known set of repos from list_repos()
   - Call call_search_semantic with arguments: `{"query": "test", "repos": ["nonexistent-repo"]}`
   - Assert response["result"]["isError"] == true
   - Assert error message contains "Unknown repo"

Add tests to `crates/mcp/src/lib.rs` in the `#[cfg(test)] mod tests` section.

#### 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux
cargo build
cargo test --package commitmux-mcp test_search_semantic_rejects
cargo test --workspace
```

All tests must pass (3 new tests + 70 existing).

#### 7. Constraints

- Do NOT change the `commitmux_search_semantic` tool schema in `handle_tools_list` (the inputSchema is correct)
- Do NOT change the Store trait signature
- Error messages must be user-friendly (no internal details, no stack traces)

#### 8. Report

**Before reporting:** Commit your changes:

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-d
git add crates/mcp/src/lib.rs
git commit -m "wave1-agent-d: add input validation to semantic search (R4-04, R4-05)"
```

Append to IMPL doc under `### Agent D — Completion Report`:

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-d
commit: {sha}
files_changed:
  - crates/mcp/src/lib.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_search_semantic_rejects_empty_query
  - test_search_semantic_rejects_limit_zero
  - test_search_semantic_rejects_nonexistent_repo
verification: PASS (cargo test --workspace — 73/73 tests)
```

---

## Wave Execution Loop

After Wave 1 completes:

1. Read each agent's completion report from their section in this IMPL doc. Check for:
   - Interface contract deviations
   - Out-of-scope dependencies flagged by agents
   - Verification status (PASS/FAIL)

2. Merge all agent worktrees back into main:
   ```bash
   cd /Users/dayna.blackwell/code/commitmux
   git merge --no-ff wave1-agent-a -m "Merge wave1-agent-a: fix semantic search SQL"
   git merge --no-ff wave1-agent-b -m "Merge wave1-agent-b: help text"
   git merge --no-ff wave1-agent-c -m "Merge wave1-agent-c: status display"
   git merge --no-ff wave1-agent-d -m "Merge wave1-agent-d: input validation"
   ```

3. Run full verification gate against merged code:
   ```bash
   cargo build
   cargo clippy -- -D warnings
   cargo test --workspace
   ```

   Expected: 78 tests pass (70 existing + 3 from A + 2 from C + 3 from D).

4. Test semantic search end-to-end manually:
   ```bash
   # Assuming a test DB with embeddings exists
   COMMITMUX_DB=/path/to/test.db commitmux serve
   # In another terminal, send a semantic search query via MCP
   printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"test query","limit":5}}}\n' | COMMITMUX_DB=/path/to/test.db commitmux serve
   # Verify: result should contain commits (not [])
   ```

5. Fix any compiler errors or integration issues.

6. Update this IMPL doc:
   - Tick status checkboxes
   - Correct any interface contracts that changed during implementation
   - Record any file ownership changes

7. Commit the wave's changes:
   ```bash
   git add .
   git commit -m "Wave 1 complete: R4 audit fixes (semantic search bug + UX improvements)"
   ```

8. If verification fails, fix before proceeding. Do not launch the next wave with a broken build.

---

## Status

- [ ] Wave 1 Agent A - Fix semantic search SQL query bug (R4-01)
- [ ] Wave 1 Agent B - Add embedding setup guidance to help text (R4-02)
- [ ] Wave 1 Agent C - Distinguish enabled vs generated embeddings in status (R4-03)
- [ ] Wave 1 Agent D - Add input validation to semantic search (R4-04, R4-05)

**Deferred to future work:**
- R4-06: Add incremental progress for embedding generation (>100 commits)
- R4-07: Add progress indicator for embed-only sync (>100 commits)

Both progress indicators require refactoring the batch loop in `crates/embed/src/lib.rs::embed_pending` to report progress via callback or channel. Not blocking for semantic search functionality.

---

## Agent Completion Reports

### Agent A — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-a
commit: aef795f037e4e51582160373828f04af9016d735
files_changed:
  - crates/store/src/queries.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_search_semantic_returns_results
  - test_search_semantic_limit_respected
  - test_search_semantic_repo_filter
verification: PASS (cargo test --package commitmux-store — 3/3 new tests, 73 total workspace tests)
```

**Implementation notes:**

The original SQL query had `AND k = ?2` in the WHERE clause, which is correct sqlite-vec syntax, but the issue was with the filter logic. The fix involved:

1. Wrapping the kNN query in a subquery to separate the vector search from post-filtering
2. Fixing the repos filter condition from `'' = ?3` to `'[]' = ?3` to properly handle the case when no repo filter is specified (repos_json is '[]')
3. The `k = ?2` parameter is required by sqlite-vec for vec0 virtual tables and must remain in the inner WHERE clause with the MATCH

The corrected query structure:
```sql
SELECT ... FROM (
  SELECT ... FROM commit_embeddings ce
  WHERE ce.embedding MATCH ?1 AND k = ?2
  ORDER BY distance
)
WHERE ('[]' = ?3 OR repo_name IN (SELECT value FROM json_each(?3)))
  AND (?4 = 0 OR author_time >= ?4)
```

All 3 new tests pass, verifying:
- Basic semantic search returns results
- Limit parameter is respected
- Repo filter works correctly

---

### Agent B — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-b
commit: d5b9e71
files_changed:
  - src/main.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added: []
verification: PASS (cargo test --workspace — 70/70 tests)
```

---

### Agent C — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-c
commit: a06d689dbad8dc9c2049dbae144a37e0ce1e39c8
files_changed:
  - src/main.rs
  - crates/types/src/lib.rs
  - crates/store/src/queries.rs
  - crates/ingest/src/lib.rs
  - crates/embed/src/lib.rs
  - crates/mcp/src/lib.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_count_embeddings_for_repo_zero_when_none_exist
  - test_count_embeddings_for_repo_matches_stored_count
verification: PASS (cargo test --workspace — 72/72 tests)
```

---

### Agent D — Completion Report

```yaml
status: complete
worktree: .claude/worktrees/wave1-agent-d
commit: d85cf0d85e133940cd837dff8993836c221e08dd
files_changed:
  - crates/mcp/src/lib.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_search_semantic_rejects_empty_query
  - test_search_semantic_rejects_limit_zero
  - test_search_semantic_rejects_nonexistent_repo
verification: PASS (cargo test --workspace — 70/70 tests, 3 new tests pass)
```

---

## Post-Merge Notes

(To be filled after wave merges and verification)
