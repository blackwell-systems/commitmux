# IMPL: Feature Wave 1 — Correctness, Ingest Pipeline, MCP Surface

<!-- scout v0.2.0 -->

> **Index file.** Full agent prompts are in `docs/IMPL-feature-wave-1-agents/`.
> This file is what the orchestrator reads every turn.

---

## Suitability Assessment

**Verdict: SUITABLE**

The 11 features span four distinct layers (types, store, ingest, mcp/CLI).
Almost every feature can be assigned to an agent with non-overlapping file
ownership. The two features requiring a new `Store` trait method are gated
behind Wave 0 (Agent A extends the trait first). Interface contracts are fully
derivable. No feature requires investigation before implementation.

**Pre-implementation scan:**

```
Total items:          11 features
Already implemented:   0
Partially implemented: 1  (--exclude: CLI parsing + warn exists; DB storage missing)
To-do:                10

Agent adjustments:
- Agent E: "complete the implementation" for --exclude (warn removed, persistence added)
- All others proceed as planned

Estimated time saved: ~5 min (avoid re-implementing --exclude CLI flag)
```

**Estimated times:**

```
Scout phase:       ~15 min
Agent execution:   ~70 min  (Wave 0: 1×15 min; Wave 1: 4 parallel × 10–15 min)
Merge & verify:    ~10 min
Total SAW time:    ~95 min

Sequential baseline: ~110 min
Time savings: ~15 min wall-clock (Wave 1 parallelism)
Recommendation: Clear speedup.
```

---

## Known Issues

None. All 17 tests pass clean on `cargo test --workspace` as of 2026-02-28.

---

## Dependency Graph

```
commitmux-types  (crates/types/src/lib.rs)   ← Wave 0 Agent A owns
       |
       ├── commitmux-store  (crates/store/src/)    ← Wave 1 Agent B owns
       |
       ├── commitmux-ingest  (crates/ingest/src/)  ← Wave 1 Agent C owns walker
       |
       ├── commitmux-mcp  (crates/mcp/src/)        ← Wave 1 Agent D owns tool additions
       |
       └── commitmux bin  (src/main.rs)             ← Wave 1 Agent E owns
```

**Root (Wave 0):** `crates/types/src/lib.rs` — `Store` trait and domain types.
All five Wave 1 agents compile against the trait; it must be finalized first.

**Cascade candidates (unchanged files that reference changed interfaces):**

- `tests/integration.rs` — constructs `RepoInput` directly. Agent A updates
  this atomically (new struct fields). Verify at post-merge gate.
- `crates/store/src/lib.rs` (test helper `make_repo_input`) — Agent A updates
  atomically. Verify at post-merge gate.
- `crates/mcp/src/lib.rs` `StubStore` — Agent A adds stubs; Agent D adds tool
  logic. Different code regions; merge conflicts unlikely but check.
- `crates/ingest/src/lib.rs` `MockStore` — Agent A adds `commit_exists` as a
  real implementation (not `unimplemented!()`); Agent C's tests depend on this.

---

## Interface Contracts

### Delivered by Wave 0 Agent A

**New fields on `Repo` and `RepoInput`:**

```rust
pub struct Repo {
    // ... existing fields ...
    pub fork_of: Option<String>,
    pub author_filter: Option<String>,
    pub exclude_prefixes: Vec<String>,
}

pub struct RepoInput {
    // ... existing fields ...
    pub fork_of: Option<String>,
    pub author_filter: Option<String>,
    pub exclude_prefixes: Vec<String>,
}
```

**New types:**

```rust
#[derive(Debug, Clone, Default)]
pub struct RepoUpdate {
    pub fork_of: Option<Option<String>>,
    pub author_filter: Option<Option<String>>,
    pub exclude_prefixes: Option<Vec<String>>,
    pub default_branch: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoListEntry {
    pub name: String,
    pub commit_count: usize,
    pub last_synced_at: Option<i64>,
}
```

**Changed/new `Store` trait methods:**

```rust
// CHANGED: sha_prefix matched with LIKE prefix (≥4 chars recommended)
fn get_commit(&self, repo_name: &str, sha_prefix: &str) -> Result<Option<CommitDetail>>;

// NEW:
fn remove_repo(&self, name: &str) -> Result<()>;
fn commit_exists(&self, repo_id: i64, sha: &str) -> Result<bool>;
fn update_repo(&self, repo_id: i64, update: &RepoUpdate) -> Result<Repo>;
fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>>;
```

**`MockStore.commit_exists`** (in `crates/ingest/src/lib.rs`) — Agent A must
implement this as a real check (not `unimplemented!()`):

```rust
fn commit_exists(&self, _repo_id: i64, sha: &str) -> Result<bool> {
    Ok(self.commits.lock().unwrap().iter().any(|c| c.sha == sha))
}
```

### Delivered by Wave 1 agents (depend on Agent A)

- **Agent B** (`crates/store/`): implements all 5 new/changed `Store` methods
  in `SqliteStore`; adds three columns to `repos` table.
- **Agent C** (`crates/ingest/src/walker.rs`): no API changes; reads new
  `Repo` fields and calls `store.commit_exists`.
- **Agent D** (`crates/mcp/src/`): adds `commitmux_list_repos` tool (no
  required args); calls `store.list_repos_with_stats()`.
- **Agent E** (`src/main.rs`): adds `remove-repo`, `update-repo` subcommands;
  extends `add-repo` with `--fork-of`, `--author`, persisted `--exclude`.

---

## File Ownership

| File | Agent | Wave | Depends On |
|------|-------|------|------------|
| `crates/types/src/lib.rs` | A | 0 | — |
| `crates/ingest/src/lib.rs` | A | 0 | — (MockStore stubs + make_repo helper) |
| `crates/mcp/src/lib.rs` (StubStore only) | A | 0 | — |
| `crates/store/src/lib.rs` (make_repo_input helper) | A | 0 | — (atomic call-site) |
| `tests/integration.rs` (RepoInput literal only) | A | 0 | — (atomic call-site) |
| `crates/store/src/schema.rs` | B | 1 | Agent A |
| `crates/store/src/queries.rs` | B | 1 | Agent A |
| `crates/store/src/lib.rs` (tests) | B | 1 | Agent A |
| `crates/ingest/src/walker.rs` | C | 1 | Agent A |
| `crates/mcp/src/lib.rs` (tool dispatch + tests) | D | 1 | Agent A |
| `crates/mcp/src/tools.rs` | D | 1 | Agent A |
| `src/main.rs` | E | 1 | Agent A |

**Ownership split notes:**

- `crates/mcp/src/lib.rs` is split between A (Wave 0, StubStore stubs only)
  and D (Wave 1, tool logic + tests). Different code regions; merge by
  section, not by file. A's changes are inside `impl Store for StubStore { }`;
  D's changes are in `impl McpServer { }` and `mod tests { }`.

- `crates/store/src/lib.rs` is split between A (Wave 0, `make_repo_input`
  helper update only — atomic call-site) and B (Wave 1, new store tests).
  A's change is a single helper function body; B adds new test functions.

---

## Wave Structure

```
Wave 0:  [A]                    ← prerequisite: types + trait + mock stubs
          |  (A completes; cargo test --workspace must pass before Wave 1)
Wave 1: [B]  [C]  [D]  [E]     ← 4 fully independent parallel agents
          |  (all 4 complete; merge all; cargo test --workspace must pass)
```

Wave 0 gates all downstream. If Agent A's build breaks, Wave 1 cannot start.

---

## Agent Prompts

Full prompts are in per-agent files. Load only the file for the agent you
are launching.

| Agent | Wave | File |
|-------|------|------|
| A | 0 | [`docs/IMPL-feature-wave-1-agents/agent-a.md`](IMPL-feature-wave-1-agents/agent-a.md) |
| B | 1 | [`docs/IMPL-feature-wave-1-agents/agent-b.md`](IMPL-feature-wave-1-agents/agent-b.md) |
| C | 1 | [`docs/IMPL-feature-wave-1-agents/agent-c.md`](IMPL-feature-wave-1-agents/agent-c.md) |
| D | 1 | [`docs/IMPL-feature-wave-1-agents/agent-d.md`](IMPL-feature-wave-1-agents/agent-d.md) |
| E | 1 | [`docs/IMPL-feature-wave-1-agents/agent-e.md`](IMPL-feature-wave-1-agents/agent-e.md) |

---

## Wave Execution Loop

After each wave completes:

1. **Read completion reports** from `### Agent {letter} — Completion Report`
   sections at the bottom of this file. Check for interface contract deviations
   and out-of-scope files.

2. **Merge worktrees.** Wave 0:
   ```bash
   git merge wave0-agent-a --no-ff -m "merge wave0-agent-a"
   ```
   Wave 1 (merge all four):
   ```bash
   git merge wave1-agent-b --no-ff -m "merge wave1-agent-b"
   git merge wave1-agent-c --no-ff -m "merge wave1-agent-c"
   git merge wave1-agent-d --no-ff -m "merge wave1-agent-d"
   git merge wave1-agent-e --no-ff -m "merge wave1-agent-e"
   ```

3. **Run full verification gate:**
   ```bash
   cargo build --workspace
   cargo clippy --workspace -- -D warnings
   cargo test --workspace
   ```

4. **Check cascade candidates** (see Dependency Graph section above).

5. **Fix any merge conflicts or compilation errors** before proceeding.
   In particular:
   - If `MockStore.commit_exists` is `unimplemented!()` after merge, fix it
     before launching Agent C (or before the post-Wave-1 gate).
   - If `crates/mcp/src/lib.rs` has merge conflicts between A's StubStore
     stubs and D's tool additions, resolve by keeping both sections.

6. **Update status checkboxes** below.

7. **Commit the merged wave** and proceed to next wave.

---

## Status

- [ ] Wave 0 Agent A — Extend `Store` trait + `Repo`/`RepoInput`/`RepoUpdate`/`RepoListEntry` types; MockStore + StubStore stubs; atomic call-site updates
- [ ] Wave 1 Agent B — Schema migrations; `remove_repo`, `commit_exists`, `update_repo`, `list_repos_with_stats`; short-SHA `get_commit`
- [ ] Wave 1 Agent C — `sync_repo` walker: incremental skip, author filter, exclude persistence, fork-of merge-base exclusion
- [ ] Wave 1 Agent D — MCP tool `commitmux_list_repos`
- [ ] Wave 1 Agent E — CLI: `remove-repo`, `update-repo`, `--fork-of`, `--author`, `--exclude` persistence

---

<!-- Agent completion reports appended below after each wave -->
