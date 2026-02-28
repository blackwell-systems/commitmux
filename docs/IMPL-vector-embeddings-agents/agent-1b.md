# Wave 1 Agent B: New `crates/embed` Crate

You are Wave 1 Agent B. Your task is to create the `crates/embed` workspace crate,
implementing `Embedder`, `embed_pending`, `build_embed_doc`, `EmbedSummary`, and `EmbedConfig`.

**Expected build blockers:** This crate depends on new Store trait methods (`get_commits_without_embeddings`,
`store_embedding`, `EmbedCommit`) delivered by Wave 1 Agent A. Those methods do not exist in
your worktree. Code against the interface contracts specified below. Note build failures as
`out_of_scope_build_blockers` and mark verification FAIL. This is expected and resolved at merge.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b"
if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"; echo "Actual: $ACTUAL_DIR"; exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-b" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: wave1-agent-b"; echo "Actual: $ACTUAL_BRANCH"; exit 1
fi

git worktree list | grep -q "wave1-agent-b" || { echo "ISOLATION FAILURE: Not in worktree list"; exit 1; }
echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `crates/embed/Cargo.toml` — create
- `crates/embed/src/lib.rs` — create
- `Cargo.toml` (workspace root) — modify (add `crates/embed` to workspace members)

Do NOT touch any other files.

## 2. Interfaces You Must Implement

```rust
// In crates/embed/src/lib.rs:

pub struct EmbedConfig {
    pub model: String,
    pub endpoint: String,
}

impl EmbedConfig {
    /// Reads embed.model and embed.endpoint from store config.
    /// Falls back to "nomic-embed-text" and "http://localhost:11434/v1".
    pub fn from_store(store: &dyn Store) -> anyhow::Result<Self>;
}

pub struct Embedder {
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
    pub model: String,
}

impl Embedder {
    /// Constructs an Embedder pointing at the given endpoint with the given model.
    pub fn new(config: &EmbedConfig) -> Self;

    /// Calls the embedding API and returns a float vector.
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>>;
}

pub struct EmbedSummary {
    pub embedded: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Builds the embedding document for a commit.
/// Format: "{subject}\n\n{body}\n\nFiles changed: {files}\n\n{patch_preview truncated}"
/// Pure function — no I/O.
pub fn build_embed_doc(commit: &EmbedCommit) -> String;

/// Embeds all commits without embeddings for a repo.
/// Fetches in batches of `batch_size`. On per-commit failures: increments `failed`, continues.
/// Returns Err only if the store query itself fails.
pub async fn embed_pending(
    store: &dyn Store,
    embedder: &Embedder,
    repo_id: i64,
    batch_size: usize,
) -> anyhow::Result<EmbedSummary>;
```

## 3. Interfaces You May Call

These are from Wave 1 Agent A's scope. They will not compile in your isolated worktree.
Code against them as specified; note the build failures as expected:

```rust
// From commitmux_types (new — Wave 1A delivers):
pub struct EmbedCommit {
    pub repo_id: i64,
    pub sha: String,
    pub subject: String,
    pub body: Option<String>,
    pub files_changed: Vec<String>,
    pub patch_preview: Option<String>,
}

// Store trait methods (new — Wave 1A delivers):
fn get_commits_without_embeddings(&self, repo_id: i64, limit: usize) -> Result<Vec<EmbedCommit>>;
fn store_embedding(&self, repo_id: i64, sha: &str, embedding: &[f32]) -> Result<()>;
fn get_config(&self, key: &str) -> Result<Option<String>>;
```

## 4. What to Implement

### 4a. Workspace Cargo.toml

In the root `Cargo.toml`, add `"crates/embed"` to the `members` array:
```toml
[workspace]
members = [".", "crates/types", "crates/store", "crates/ingest", "crates/mcp", "crates/embed"]
```

### 4b. `crates/embed/Cargo.toml`

```toml
[package]
name = "commitmux-embed"
version = "0.1.0"
edition = "2021"

[dependencies]
commitmux-types = { path = "../types" }
async-openai = "0.27"
anyhow = "1"
tokio = { version = "1", features = ["rt", "macros"] }
```

Note: `async-openai` version may differ — use the latest available on crates.io. Check
`cargo search async-openai` or use `"*"` and let cargo resolve. The API surface we use
(embeddings endpoint) is stable across versions.

### 4c. `EmbedConfig::from_store`

```rust
pub fn from_store(store: &dyn commitmux_types::Store) -> anyhow::Result<Self> {
    let model = store.get_config("embed.model")
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .unwrap_or_else(|| "nomic-embed-text".into());
    let endpoint = store.get_config("embed.endpoint")
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .unwrap_or_else(|| "http://localhost:11434/v1".into());
    Ok(Self { model, endpoint })
}
```

### 4d. `Embedder::new` and `Embedder::embed`

```rust
impl Embedder {
    pub fn new(config: &EmbedConfig) -> Self {
        let openai_config = async_openai::config::OpenAIConfig::new()
            .with_api_base(&config.endpoint)
            .with_api_key("ollama");  // ignored by Ollama; required by client
        Self {
            client: async_openai::Client::with_config(openai_config),
            model: config.model.clone(),
        }
    }

    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        use async_openai::types::CreateEmbeddingRequestArgs;
        let request = CreateEmbeddingRequestArgs::default()
            .model(&self.model)
            .input(text)
            .build()?;
        let response = self.client.embeddings().create(request).await?;
        let embedding = response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))?
            .embedding;
        Ok(embedding)
    }
}
```

Check `async-openai` docs for exact type names (`CreateEmbeddingRequestArgs` may differ by
version). The pattern is: build request with model + input, call `.create()`, extract `.data[0].embedding`.

### 4e. `build_embed_doc`

```rust
pub fn build_embed_doc(commit: &commitmux_types::EmbedCommit) -> String {
    let mut doc = commit.subject.clone();
    if let Some(ref body) = commit.body {
        if !body.trim().is_empty() {
            doc.push_str("\n\n");
            doc.push_str(body.trim());
        }
    }
    if !commit.files_changed.is_empty() {
        doc.push_str("\n\nFiles changed: ");
        doc.push_str(&commit.files_changed.join(", "));
    }
    if let Some(ref preview) = commit.patch_preview {
        if !preview.trim().is_empty() {
            // Truncate to ~400 tokens (approx 1600 chars)
            let truncated = if preview.len() > 1600 {
                &preview[..1600]
            } else {
                preview.as_str()
            };
            doc.push_str("\n\n");
            doc.push_str(truncated);
        }
    }
    doc
}
```

### 4f. `embed_pending`

```rust
pub async fn embed_pending(
    store: &dyn commitmux_types::Store,
    embedder: &Embedder,
    repo_id: i64,
    batch_size: usize,
) -> anyhow::Result<EmbedSummary> {
    let mut summary = EmbedSummary { embedded: 0, skipped: 0, failed: 0 };

    loop {
        let batch = store
            .get_commits_without_embeddings(repo_id, batch_size)
            .map_err(|e| anyhow::anyhow!("Failed to fetch pending commits: {e}"))?;

        if batch.is_empty() {
            break;
        }

        for commit in &batch {
            let doc = build_embed_doc(commit);
            match embedder.embed(&doc).await {
                Ok(embedding) => {
                    match store.store_embedding(
                        commit.repo_id,
                        &commit.sha,
                        &commit.subject,
                        &commit.author_name,
                        &commit.repo_name,
                        commit.author_time,
                        commit.patch_preview.as_deref(),
                        &embedding,
                    ) {
                        Ok(()) => summary.embedded += 1,
                        Err(e) => {
                            eprintln!("embed: failed to store embedding for {}: {e}", commit.sha);
                            summary.failed += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("embed: failed to embed {}: {e}", commit.sha);
                    summary.failed += 1;
                }
            }
        }
    }

    Ok(summary)
}
```

## 5. Tests to Write

Add tests to `crates/embed/src/lib.rs` under `#[cfg(test)]`:

1. `test_build_embed_doc_subject_only` — EmbedCommit with only subject (no body, no files, no patch); verify doc equals subject.
2. `test_build_embed_doc_full` — EmbedCommit with all fields populated; verify doc contains subject, body, "Files changed:", and patch content.
3. `test_build_embed_doc_truncates_patch` — EmbedCommit with a patch_preview of 2000 chars; verify resulting doc's patch portion is at most 1600 chars.
4. `test_embed_config_defaults` — construct `EmbedConfig::from_store` with a mock that returns `None` for all keys; verify defaults are `"nomic-embed-text"` and `"http://localhost:11434/v1"`.

Note: Do NOT write tests that actually call the Ollama/OpenAI API — tests must pass without a
running embedding server. Tests for `Embedder::embed` and `embed_pending` that require the API
are out of scope for this agent.

For `test_embed_config_defaults`, you need a minimal Store mock. Use a simple struct:

```rust
struct NullStore;
impl commitmux_types::Store for NullStore {
    fn get_config(&self, _key: &str) -> commitmux_types::Result<Option<String>> { Ok(None) }
    fn set_config(&self, _key: &str, _value: &str) -> commitmux_types::Result<()> { Ok(()) }
    fn get_commits_without_embeddings(&self, _repo_id: i64, _limit: usize) -> commitmux_types::Result<Vec<commitmux_types::EmbedCommit>> { Ok(vec![]) }
    fn store_embedding(&self, _repo_id: i64, _sha: &str, _subject: &str, _author_name: &str, _repo_name: &str, _author_time: i64, _patch_preview: Option<&str>, _embedding: &[f32]) -> commitmux_types::Result<()> { Ok(()) }
    fn search_semantic(&self, _embedding: &[f32], _opts: &commitmux_types::SemanticSearchOpts) -> commitmux_types::Result<Vec<commitmux_types::SearchResult>> { Ok(vec![]) }
    // ... all other methods: unimplemented!()
}
```

This will fail to compile because Wave 1A's new methods don't exist in your worktree. That's expected.
Note the build failure as `out_of_scope_build_blockers`.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b
cargo build -p commitmux-embed 2>&1 | head -30
```

**Expected result:** Build will FAIL due to missing `EmbedCommit`, `SemanticSearchOpts`, and
new Store trait methods from Wave 1A. This is expected. Document the exact error in your report.

Run tests only if build succeeds (it won't — that's fine):
```bash
cargo test -p commitmux-embed
```

Mark verification as: `FAIL (build blocked on Wave 1A: missing EmbedCommit, get_commits_without_embeddings, store_embedding in Store trait)`

## 7. Constraints

- `embed_pending` must be resilient: per-commit failures increment `failed` and continue — never
  abort the entire batch.
- The `build_embed_doc` truncation is at the character level, not token level. `len / 4` ≈ tokens
  is close enough. 1600 chars ≈ 400 tokens.
- The `embed()` function is `async`. The MCP server (Wave 2B) will call it via `block_on`.
  Do NOT make `embed_pending` return a `std::thread::JoinHandle` or spawn threads — keep it
  pure async.
- Do NOT add `commitmux-embed` as a dependency to any other crate — that is Wave 2A and 2B's job.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b
git add crates/embed/ Cargo.toml
git commit -m "wave1-agent-b: add crates/embed with Embedder, embed_pending, build_embed_doc"
```

Append to `docs/IMPL-vector-embeddings.md` under `### Agent 1B — Completion Report`:

```yaml
### Agent 1B — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-b
commit: {sha}
files_changed:
  - Cargo.toml
files_created:
  - crates/embed/Cargo.toml
  - crates/embed/src/lib.rs
interface_deviations: []
out_of_scope_build_blockers:
  - "EmbedCommit type not found — owned by Wave 1A (crates/types/src/lib.rs)"
  - "Store::get_commits_without_embeddings not found — owned by Wave 1A"
  - "Store::store_embedding not found — owned by Wave 1A"
tests_added:
  - test_build_embed_doc_subject_only
  - test_build_embed_doc_full
  - test_build_embed_doc_truncates_patch
  - test_embed_config_defaults
verification: FAIL (build blocked on Wave 1A: missing EmbedCommit and new Store methods)
```
