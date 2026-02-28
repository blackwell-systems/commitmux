# Wave 0 Agent A: Types, trait extension, and mock stubs

You are Wave 0 Agent A. Your task is to extend the `Store` trait and domain
types in `commitmux-types`, and add stub implementations to the two in-test
mock stores so the workspace compiles cleanly after your changes.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-agent-a 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-agent-a"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual:   $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave0-agent-a"

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

If verification fails, write the error to the completion report and stop.

## 1. File Ownership

You own these files. Do not touch any other files except the atomically-required
call-site updates listed in section 4.

- `crates/types/src/lib.rs` — modify
- `crates/ingest/src/lib.rs` — modify (MockStore stubs only; also update `make_repo` helper)
- `crates/mcp/src/lib.rs` — modify (StubStore stubs only; do NOT touch McpServer or tests)

**Atomically required call sites (justified in section 4):**
- `crates/store/src/lib.rs` — update `make_repo_input()` test helper
- `tests/integration.rs` — update `RepoInput` struct literal

## 2. Interfaces You Must Implement

**New fields on `Repo` and `RepoInput` in `crates/types/src/lib.rs`:**

```rust
pub struct Repo {
    pub repo_id: i64,
    pub name: String,
    pub local_path: PathBuf,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
    // NEW:
    pub fork_of: Option<String>,
    pub author_filter: Option<String>,
    pub exclude_prefixes: Vec<String>,
}

pub struct RepoInput {
    pub name: String,
    pub local_path: PathBuf,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
    // NEW:
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

**Changed and new methods on the `Store` trait:**

```rust
pub trait Store: Send + Sync {
    // CHANGED — now prefix-matches sha_prefix:
    /// sha_prefix: exact SHA or a unique hex prefix (≥4 chars recommended)
    fn get_commit(&self, repo_name: &str, sha_prefix: &str) -> Result<Option<CommitDetail>>;

    // NEW:
    fn remove_repo(&self, name: &str) -> Result<()>;
    fn commit_exists(&self, repo_id: i64, sha: &str) -> Result<bool>;
    fn update_repo(&self, repo_id: i64, update: &RepoUpdate) -> Result<Repo>;
    fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>>;

    // All other existing methods unchanged (add_repo, list_repos, get_repo_by_name,
    // upsert_commit, upsert_commit_files, upsert_patch, get_ingest_state,
    // update_ingest_state, search, touches, get_patch, repo_stats)
}
```

## 3. Interfaces You May Call

Existing types in `crates/types/src/lib.rs` — read the full file before editing.

## 4. What to Implement

**In `crates/types/src/lib.rs`:**

1. Add `fork_of`, `author_filter`, `exclude_prefixes` to `Repo` and `RepoInput`.

2. Add `RepoUpdate` struct with `#[derive(Debug, Clone, Default)]` and
   `RepoListEntry` with `#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]`.

3. Add 4 new methods to `Store` trait. Update `get_commit` doc comment.

4. Update `test_smoke_construct_all_types` test to set the new fields on `Repo`
   and `RepoInput` (use `fork_of: None`, `author_filter: None`,
   `exclude_prefixes: vec![]`).

**In `crates/ingest/src/lib.rs`:**

5. In `MockStore`, add stubs:
   ```rust
   fn remove_repo(&self, _name: &str) -> Result<()> { unimplemented!() }
   fn commit_exists(&self, _repo_id: i64, sha: &str) -> Result<bool> {
       // Real impl for test support:
       Ok(self.commits.lock().unwrap().iter().any(|c| c.sha == sha))
   }
   fn update_repo(&self, _repo_id: i64, _update: &RepoUpdate) -> Result<Repo> { unimplemented!() }
   fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>> { unimplemented!() }
   ```
   Also update `get_commit` signature: `fn get_commit(&self, _repo_name: &str, _sha: &str) -> Result<Option<CommitDetail>> { unimplemented!() }` (rename param to `_sha` to match updated trait).

6. Update `make_repo()` helper:
   ```rust
   fn make_repo(path: &std::path::Path) -> Repo {
       Repo {
           repo_id: 1,
           name: "test-repo".into(),
           local_path: path.to_path_buf(),
           remote_url: None,
           default_branch: None,
           fork_of: None,
           author_filter: None,
           exclude_prefixes: vec![],
       }
   }
   ```

**In `crates/mcp/src/lib.rs`:**

7. In `StubStore`, add stubs:
   ```rust
   fn remove_repo(&self, _name: &str) -> Result<()> { unimplemented!() }
   fn commit_exists(&self, _repo_id: i64, _sha: &str) -> Result<bool> { unimplemented!() }
   fn update_repo(&self, _repo_id: i64, _update: &RepoUpdate) -> Result<Repo> { unimplemented!() }
   fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>> { unimplemented!() }
   ```
   Also update `get_commit` signature to match new trait (param rename only).

8. Update import list in `crates/mcp/src/lib.rs` (the `use commitmux_types::{...}`
   at the top of the `#[cfg(test)]` block) to include `RepoUpdate` and
   `RepoListEntry`.

**Atomic call-site updates (justified: struct fields added require all
 construction sites to be updated):**

9. In `crates/store/src/lib.rs`, update `make_repo_input()`:
   ```rust
   fn make_repo_input(name: &str) -> RepoInput {
       RepoInput {
           name: name.to_string(),
           local_path: PathBuf::from(format!("/tmp/{}", name)),
           remote_url: None,
           default_branch: Some("main".to_string()),
           fork_of: None,
           author_filter: None,
           exclude_prefixes: vec![],
       }
   }
   ```

10. In `tests/integration.rs`, update the `RepoInput` struct literal:
    ```rust
    let repo_input = RepoInput {
        name: "test-repo".to_string(),
        local_path: repo_dir.path().to_path_buf(),
        remote_url: None,
        default_branch: None,
        fork_of: None,
        author_filter: None,
        exclude_prefixes: vec![],
    };
    ```

## 5. Tests to Write

In `crates/types/src/lib.rs`:

1. `test_repo_new_fields_default` — construct `Repo` with new fields set to
   `None`/empty; verify field access compiles and returns correct values.
2. `test_repo_update_type` — construct `RepoUpdate::default()` and one with
   `fork_of: Some(Some("https://github.com/foo/bar".into()))`.
3. `test_repo_list_entry_serializes` — construct `RepoListEntry`, serialize to
   JSON, deserialize back, verify all fields match.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-agent-a
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

All 17 existing tests must continue to pass. 3 new tests added.

## 7. Constraints

- Do NOT implement any SQL. That is Agent B.
- Do NOT change `sync_repo` logic. That is Agent C.
- Do NOT add MCP tool dispatch. That is Agent D.
- Do NOT add CLI subcommands. That is Agent E.
- `commit_exists` in `MockStore` should be a real implementation (checking the
  commits Vec) so Agent C's tests can use it.
- `RepoUpdate` uses `Option<Option<T>>` to distinguish "no change" from "clear".
- `exclude_prefixes` on `Repo`/`RepoInput` is `Vec<String>` (not `Option`).

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave0-agent-a
git add crates/types/src/lib.rs \
        crates/ingest/src/lib.rs \
        crates/mcp/src/lib.rs \
        crates/store/src/lib.rs \
        tests/integration.rs
git commit -m "wave0-agent-a: extend Store trait, Repo types, mock stubs"
```

Append your completion report to
`/Users/dayna.blackwell/code/commitmux/docs/IMPL-feature-wave-1.md`
under `### Agent A — Completion Report`.

Include:
- What you implemented (function names, key decisions)
- Test results (pass/fail, count)
- Any deviations from the spec and why
- Any interface contract changes
- Out-of-scope files touched (list each with justification)
