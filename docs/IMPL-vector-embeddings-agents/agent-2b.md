# Wave 2 Agent B: MCP — commitmux_search_semantic tool

You are Wave 2 Agent B. Your task is to add the `commitmux_search_semantic` MCP tool to
`crates/mcp/src/lib.rs` and `crates/mcp/src/tools.rs`, add tokio as a dependency to
`crates/mcp/Cargo.toml`, update `StubStore` with new Store trait method stubs, and add
`commitmux-embed` as a dependency.

Wave 1A and 1B are already merged. All new Store trait methods and `crates/embed` exist.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-b 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-b"
if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"; echo "Actual: $ACTUAL_DIR"; exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave2-agent-b" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: wave2-agent-b"; echo "Actual: $ACTUAL_BRANCH"; exit 1
fi

git worktree list | grep -q "wave2-agent-b" || { echo "ISOLATION FAILURE: Not in worktree list"; exit 1; }
echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `crates/mcp/src/lib.rs` — modify
- `crates/mcp/src/tools.rs` — modify
- `crates/mcp/Cargo.toml` — modify

Do NOT touch any other files.

## 2. Interfaces You Must Implement

### `SemanticSearchInput` in `crates/mcp/src/tools.rs`

```rust
#[derive(Debug, serde::Deserialize)]
pub struct SemanticSearchInput {
    pub query: String,
    pub repos: Option<Vec<String>>,
    pub since: Option<i64>,
    pub limit: Option<usize>,
}
```

### `call_search_semantic` method on `McpServer` in `crates/mcp/src/lib.rs`

```rust
fn call_search_semantic(&self, args: &serde_json::Value) -> anyhow::Result<serde_json::Value>;
```

## 3. Interfaces You May Call

```rust
// From crates/types (Wave 1A):
Store::get_config(key) -> Result<Option<String>>
Store::search_semantic(embedding, opts) -> Result<Vec<SearchResult>>
SemanticSearchOpts { repos, since, limit }

// From crates/embed (Wave 1B):
commitmux_embed::EmbedConfig::from_store(store) -> anyhow::Result<EmbedConfig>
commitmux_embed::Embedder::new(config) -> Embedder
commitmux_embed::Embedder::embed(text) -> async anyhow::Result<Vec<f32>>
```

## 4. What to Implement

Read `crates/mcp/src/lib.rs` and `crates/mcp/src/tools.rs` in full before making changes.
Read `docs/vector-embeddings.md` for context. Read the existing `call_search` implementation
as a pattern for `call_search_semantic`.

### 4a. Update `crates/mcp/Cargo.toml`

Add dependencies:
```toml
commitmux-embed = { path = "../embed" }
tokio = { version = "1", features = ["rt"] }
```

### 4b. Add `SemanticSearchInput` to `tools.rs`

Following the pattern of the existing `SearchInput` struct, add:
```rust
#[derive(Debug, serde::Deserialize)]
pub struct SemanticSearchInput {
    pub query: String,
    pub repos: Option<Vec<String>>,
    pub since: Option<i64>,
    pub limit: Option<usize>,
}
```

### 4c. Register the tool in `handle_tools_list`

In `crates/mcp/src/lib.rs`, in the `handle_tools_list` method, add an entry to the tools array:

```json
{
    "name": "commitmux_search_semantic",
    "description": "Semantic search over indexed commits using vector similarity. Use when keyword search is insufficient — e.g. 'find commits related to rate limiting' or 'work similar to this description'. Only returns results for repos with embeddings enabled.",
    "inputSchema": {
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "Natural language description of what you're looking for" },
            "repos": { "type": "array", "items": { "type": "string" }, "description": "Optional list of repo names to search within" },
            "since": { "type": "integer", "description": "Optional Unix timestamp lower bound" },
            "limit": { "type": "integer", "description": "Max results (default 10)" }
        },
        "required": ["query"]
    }
}
```

### 4d. Add dispatch in `handle_tools_call`

In the `match name { ... }` block:
```rust
"commitmux_search_semantic" => self.call_search_semantic(&arguments),
```

### 4e. Implement `call_search_semantic`

**IMPORTANT: The MCP server is synchronous by design (no tokio runtime).** The embed call is
async. Use a single-threaded tokio runtime for this one call only:

```rust
fn call_search_semantic(&self, args: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    use commitmux_types::SemanticSearchOpts;
    use tools::SemanticSearchInput;

    let input: SemanticSearchInput = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("Invalid arguments: {e}"))?;

    // Build embedder from store config
    let config = commitmux_embed::EmbedConfig::from_store(self.store.as_ref())
        .map_err(|e| anyhow::anyhow!("Embed config error: {e}"))?;
    let embedder = commitmux_embed::Embedder::new(&config);

    // Embed the query (async → sync via block_on)
    let embedding = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build tokio runtime: {e}"))?
        .block_on(embedder.embed(&input.query))
        .map_err(|e| anyhow::anyhow!("Failed to embed query: {e}"))?;

    // Search
    let opts = SemanticSearchOpts {
        repos: input.repos,
        since: input.since,
        limit: input.limit,
    };
    let results = self.store.search_semantic(&embedding, &opts)
        .map_err(|e| anyhow::anyhow!("Semantic search failed: {e}"))?;

    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&results)
                .unwrap_or_else(|_| "[]".into())
        }]
    }))
}
```

Errors from `call_search_semantic` propagate to `handle_tools_call`. Check how existing
tool errors are handled and follow the same pattern (wrap in MCP error response).

### 4f. Update `StubStore` and `StubStoreWithRepos`

In `crates/mcp/src/lib.rs`, the `#[cfg(test)]` block has `StubStore` and (possibly)
`StubStoreWithRepos`. Both implement the `Store` trait. Add stub implementations for all
new methods:

```rust
fn get_config(&self, _key: &str) -> commitmux_types::Result<Option<String>> { Ok(None) }
fn set_config(&self, _key: &str, _value: &str) -> commitmux_types::Result<()> { Ok(()) }
fn get_commits_without_embeddings(&self, _repo_id: i64, _limit: usize) -> commitmux_types::Result<Vec<commitmux_types::EmbedCommit>> { Ok(vec![]) }
#[allow(clippy::too_many_arguments)]
fn store_embedding(&self, _repo_id: i64, _sha: &str, _subject: &str, _author_name: &str, _repo_name: &str, _author_time: i64, _patch_preview: Option<&str>, _embedding: &[f32]) -> commitmux_types::Result<()> { Ok(()) }
fn search_semantic(&self, _embedding: &[f32], _opts: &commitmux_types::SemanticSearchOpts) -> commitmux_types::Result<Vec<commitmux_types::SearchResult>> { Ok(vec![]) }
```

Also update the `use commitmux_types::{...}` import at the top of the test module to include
`EmbedCommit` and `SemanticSearchOpts`.

## 5. Tests to Write

Add to `crates/mcp/src/lib.rs` `#[cfg(test)]`:

1. `test_tools_list_includes_semantic` — call `handle_tools_list` on a test server, parse the
   result, assert that a tool named `"commitmux_search_semantic"` appears in the tools array.

2. `test_search_semantic_missing_query` — call `handle_tools_call` with
   `{"name": "commitmux_search_semantic", "arguments": {}}` (no `query`), assert the response
   contains an `"error"` field (malformed input should return MCP error, not panic).

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-b
cargo build -p commitmux-mcp
cargo clippy -p commitmux-mcp -- -D warnings
cargo test -p commitmux-mcp
```

All existing MCP tests must pass. New tests must pass.

## 7. Constraints

- **Do NOT add an async runtime globally to the MCP crate.** The single `block_on` in
  `call_search_semantic` is the correct pattern. Do not add `#[tokio::main]` or restructure
  `run_stdio` to be async.
- Error path: if the embedding endpoint is unreachable, `call_search_semantic` returns an `Err`.
  The caller (`handle_tools_call`) converts this to a JSON-RPC error response. This is correct —
  the AI agent sees a tool error and can handle it.
- Follow the existing MCP response envelope format exactly:
  `{ "content": [{ "type": "text", "text": "..." }] }` for success.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-b
git add crates/mcp/src/lib.rs crates/mcp/src/tools.rs crates/mcp/Cargo.toml
git commit -m "wave2-agent-b: add commitmux_search_semantic MCP tool"
```

Append to `docs/IMPL-vector-embeddings.md` under `### Agent 2B — Completion Report`:

```yaml
### Agent 2B — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave2-agent-b
commit: {sha}
files_changed:
  - crates/mcp/src/lib.rs
  - crates/mcp/src/tools.rs
  - crates/mcp/Cargo.toml
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_tools_list_includes_semantic
  - test_search_semantic_missing_query
verification: PASS | FAIL ({command} — N/N tests)
```
