# Wave 1 Agent D: MCP tool — commitmux_list_repos

You are Wave 1 Agent D. Your task is to add the `commitmux_list_repos` tool to
the MCP server.

**Prerequisite:** Wave 0 Agent A must complete before you start. Read Agent A's
completion report in `docs/IMPL-feature-wave-1.md` before proceeding.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-d 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-d"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual:   $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-d" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: wave1-agent-d"
  echo "Actual:   $ACTUAL_BRANCH"
  exit 1
fi

git worktree list | grep -q "wave1-agent-d" || {
  echo "ISOLATION FAILURE: Worktree not in git worktree list"
  exit 1
}

echo "Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `crates/mcp/src/lib.rs` — modify (tool dispatch, tool schema, new tests ONLY)
- `crates/mcp/src/tools.rs` — modify

**Do NOT modify `StubStore`.** Agent A already added stubs to it. Only add new
test-private store implementations in the test module.

## 2. Interfaces You Must Implement

```rust
// crates/mcp/src/tools.rs — new input type:
#[derive(Debug, Deserialize, Default)]
pub struct ListReposInput {}

// crates/mcp/src/lib.rs — new dispatch method on McpServer:
fn call_list_repos(&self, _arguments: &Value) -> Result<String, String>;
```

## 3. Interfaces You May Call

From Agent A's additions to `commitmux-types`:
```rust
store.list_repos_with_stats() -> Result<Vec<RepoListEntry>>

pub struct RepoListEntry {
    pub name: String,
    pub commit_count: usize,
    pub last_synced_at: Option<i64>,
}
```

## 4. What to Implement

Read `crates/mcp/src/lib.rs` and `crates/mcp/src/tools.rs` in full before
editing.

### `crates/mcp/src/tools.rs`

Add `ListReposInput` after the existing input types:

```rust
/// Input type for the `commitmux_list_repos` tool (no required fields).
#[derive(Debug, Deserialize, Default)]
pub struct ListReposInput {}
```

### `crates/mcp/src/lib.rs`

**Step 1: Import `ListReposInput`** in the `use tools::{...}` import at the
top of the file:

```rust
use tools::{GetCommitInput, GetPatchInput, ListReposInput, SearchInput, TouchesInput};
```

**Step 2: Add to `handle_tools_list`** — append the new tool to the JSON
`"tools"` array (inside the existing `json!({...})` in `handle_tools_list`):

```json
{
    "name": "commitmux_list_repos",
    "description": "Returns the list of indexed repos with name, commit count, and last synced timestamp (Unix seconds)",
    "inputSchema": {
        "type": "object",
        "properties": {}
    }
}
```

**Step 3: Add to `handle_tools_call`** match block:

```rust
"commitmux_list_repos" => self.call_list_repos(&arguments),
```

**Step 4: Implement `call_list_repos`**:

```rust
fn call_list_repos(&self, _arguments: &Value) -> Result<String, String> {
    self.store
        .list_repos_with_stats()
        .map_err(|e| e.to_string())
        .and_then(|entries| {
            serde_json::to_string(&entries).map_err(|e| e.to_string())
        })
}
```

**Step 5: Update `test_tools_list_response`** — change:
```rust
assert_eq!(tool_names.len(), 4, "must have exactly 4 tools");
```
to:
```rust
assert_eq!(tool_names.len(), 5, "must have exactly 5 tools");
```

**Step 6: Add `RepoListEntry` to imports** in the test `use` block:

```rust
use commitmux_types::{
    ...,
    RepoListEntry,
    // ...
};
```

## 5. Tests to Write

In `crates/mcp/src/lib.rs` (add to `#[cfg(test)]` module):

**New stub store for list_repos tests** (do NOT modify `StubStore`):

```rust
struct StubStoreWithRepos;

impl Store for StubStoreWithRepos {
    // Implement all Store methods as unimplemented!() except:
    fn list_repos_with_stats(&self) -> StoreResult<Vec<RepoListEntry>> {
        Ok(vec![
            RepoListEntry {
                name: "repo-alpha".into(),
                commit_count: 42,
                last_synced_at: Some(1700000000),
            },
            RepoListEntry {
                name: "repo-beta".into(),
                commit_count: 7,
                last_synced_at: None,
            },
        ])
    }
    // All other methods: unimplemented!()
    fn add_repo(&self, _: &RepoInput) -> StoreResult<Repo> { unimplemented!() }
    fn list_repos(&self) -> StoreResult<Vec<Repo>> { unimplemented!() }
    fn get_repo_by_name(&self, _: &str) -> StoreResult<Option<Repo>> { unimplemented!() }
    fn remove_repo(&self, _: &str) -> StoreResult<()> { unimplemented!() }
    fn commit_exists(&self, _: i64, _: &str) -> StoreResult<bool> { unimplemented!() }
    fn update_repo(&self, _: i64, _: &RepoUpdate) -> StoreResult<Repo> { unimplemented!() }
    fn upsert_commit(&self, _: &Commit) -> StoreResult<()> { unimplemented!() }
    fn upsert_commit_files(&self, _: &[CommitFile]) -> StoreResult<()> { unimplemented!() }
    fn upsert_patch(&self, _: &CommitPatch) -> StoreResult<()> { unimplemented!() }
    fn get_ingest_state(&self, _: i64) -> StoreResult<Option<IngestState>> { unimplemented!() }
    fn update_ingest_state(&self, _: &IngestState) -> StoreResult<()> { unimplemented!() }
    fn search(&self, _: &str, _: &SearchOpts) -> StoreResult<Vec<SearchResult>> { unimplemented!() }
    fn touches(&self, _: &str, _: &TouchOpts) -> StoreResult<Vec<TouchResult>> { unimplemented!() }
    fn get_commit(&self, _: &str, _: &str) -> StoreResult<Option<CommitDetail>> { unimplemented!() }
    fn get_patch(&self, _: &str, _: &str, _: Option<usize>) -> StoreResult<Option<PatchResult>> { unimplemented!() }
    fn repo_stats(&self, _: i64) -> StoreResult<RepoStats> { unimplemented!() }
}
```

**Tests:**

1. `test_tools_list_includes_list_repos` — call `tools/list`, parse response,
   verify `"commitmux_list_repos"` is in the tool names array.

2. `test_tools_call_list_repos` — create `McpServer::new(Arc::new(StubStoreWithRepos))`.
   Call `tools/call` with `name: "commitmux_list_repos"` and `arguments: {}`.
   Verify `isError` is `false`. Parse the text field as a JSON array. Verify
   the array has length 2. Verify the first entry has `name: "repo-alpha"` and
   `commit_count: 42`.

3. Update existing `test_tools_list_response` to assert 5 tools (already in
   section 4 step 5 above).

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-d
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test -p commitmux-mcp
```

All MCP tests must pass (7 existing + 2 new = 9 total; the updated
`test_tools_list_response` is one of the 7 existing).

## 7. Constraints

- `commitmux_list_repos` accepts empty arguments `{}` (no required fields).
- Returns a JSON array directly (not wrapped in an object).
- Do not modify the existing 4 tools' schemas, dispatch, or behavior.
- Do not modify `StubStore` — create a separate `StubStoreWithRepos` for your tests.
- The `ListReposInput` type is defined for completeness but not strictly needed
  (no parsing required since there are no fields). Define it anyway for
  consistency with the other input types.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-d
git add crates/mcp/src/lib.rs crates/mcp/src/tools.rs
git commit -m "wave1-agent-d: add commitmux_list_repos MCP tool"
```

Append your completion report to
`/Users/dayna.blackwell/code/commitmux/docs/IMPL-feature-wave-1.md`
under `### Agent D — Completion Report`.

Include:
- What you implemented
- Test results (pass/fail, count)
- Deviations from spec
- Interface contract changes
- Out-of-scope dependencies
