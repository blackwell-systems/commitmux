# Vector Embeddings: Semantic Search for commitmux

## The Idea

commitmux currently indexes git commits as structured data — author, SHA, timestamp,
files changed, patch diff, subject — and exposes them via SQLite FTS5 full-text search.
This handles precision queries well but cannot answer semantic ones.

Adding a vector embedding per commit would give commitmux two complementary retrieval
layers that together cover queries neither can answer alone.

## Two Layers, Two Query Types

| Query type | Best layer |
|---|---|
| "Show me commit `a3f9c2d`" | Structured (exact SHA lookup) |
| "What touched `auth/session.go`?" | Structured (file index) |
| "What work happened on auth last sprint?" | Structured (date + path filter) |
| "Find commits related to rate limiting" | Vector (semantic — no exact term match) |
| "What was the thinking behind the session refactor?" | Vector (commit message semantics) |
| "Find work similar to this diff" | Vector (embedding similarity on patch text) |

The structured index handles precision. The vector layer handles fuzzy and semantic
queries. Neither replaces the other — they stack.

## Why This Is Better Than General AI Memory

Most AI memory systems (vector stores, RAG pipelines) operate on unstructured text and
retrieve by semantic similarity alone. commitmux's hybrid approach is stronger for the
software engineering domain because:

1. **Ground truth, not compression.** Structured metadata (author, date, files, SHA) is
   lossless. Embeddings compress meaning but structured filters don't — you can narrow
   the vector search space with exact predicates before ranking.

2. **Filtered semantic search.** Instead of searching all embeddings globally, first
   narrow by `repo=payments-service AND date > 90 days ago`, then rank semantically
   within that subset. Precision + recall together.

3. **No hallucination risk on structured fields.** An AI agent asking for a commit by
   SHA gets the exact commit or nothing — there is no "approximately this SHA."

4. **Git history is already the best memory a software team has.** Every decision is
   recorded at the moment it was made, with authorship and timestamp. commitmux makes
   that memory queryable; embeddings make it semantically queryable.

## What to Embed

At ingest time, construct a text document per commit for embedding:

```
{subject}

{body}

Files changed: {files_changed joined by ", "}

{patch_text truncated to ~400 tokens}
```

Subject + body captures intent. Files changed adds structural signal. Patch text
(truncated) adds implementation detail for similarity matching against code concepts.
Token approximation: `len / 4` is sufficient — this is not a billed API call with
Ollama, and ±20% accuracy is fine for embedding input.

## Storage

Embeddings live in the same SQLite file as the commit index, using the
[`sqlite-vec`](https://github.com/asg017/sqlite-vec) extension for ANN search.
No separate infrastructure required.

Schema:

```sql
CREATE VIRTUAL TABLE commit_embeddings USING vec0(
    commit_id INTEGER PRIMARY KEY,
    embedding FLOAT[768]  -- dims match the configured model
);
```

The `commit_id` is a foreign key to `commits.id`. The virtual table is separate from
the `commits` table so it can be dropped and rebuilt independently (e.g. when switching
embedding models). ANN query with optional repo pre-filter:

```sql
SELECT c.sha, c.subject, c.repo_id, distance
FROM commit_embeddings
JOIN commits c ON c.id = commit_embeddings.commit_id
WHERE commit_embeddings.embedding MATCH ?
  AND k = 10
  AND c.repo_id IN (?, ?)   -- optional pre-filter
ORDER BY distance;
```

Pre-filtering by repo/date before the ANN step is key: it narrows the candidate set
structurally, so semantic ranking operates over a relevant subset rather than the
entire corpus.

## Incremental Cost

- **Backfill:** One embedding per commit at ingest. At ~500 tokens per commit,
  4,500 commits ≈ 2.25M tokens. Negligible with a local model (nomic-embed-text);
  meaningful but one-time with an API.
- **Incremental sync:** New commits embedded at sync time. No added latency for
  already-indexed commits.
- **Storage:** 768-dim float32 (nomic-embed-text) = 3KB per commit.
  4,500 commits ≈ 13MB. Negligible.

## Embedding Model Options

| Model | Dims | Where runs | Notes |
|---|---|---|---|
| `nomic-embed-text` | 768 | Local (Ollama) | Strong code + prose, free, offline |
| `all-MiniLM-L6-v2` | 384 | Local | Smaller, fast, good for short texts |
| `text-embedding-3-small` | 1536 | OpenAI API | High quality, per-token cost |
| `text-embedding-3-large` | 3072 | OpenAI API | Highest quality, higher cost |

Recommended default: `nomic-embed-text` via Ollama — no API key, runs offline,
strong on both code and natural language.

## Libraries

- **`async-openai`** — HTTP client for embedding model calls. Covers both the OpenAI
  API and Ollama (which mirrors the `/v1/embeddings` endpoint). Switching between local
  and API models is a config change, not a code change.
- **`sqlite-vec`** — SQLite extension for ANN search. Loaded at connection time;
  keeps everything in one file with no new infrastructure.

## Configuration

### Per-repo (stored in `repos` table)

```sql
ALTER TABLE repos ADD COLUMN embed_enabled INTEGER NOT NULL DEFAULT 0;
```

Set via CLI flags:

```
commitmux add-repo ~/code/myrepo --embed
commitmux update-repo myrepo --embed
commitmux update-repo myrepo --no-embed
```

### Global (stored in `config` table)

A `config` table holds workspace-level settings as key/value pairs:

```sql
CREATE TABLE IF NOT EXISTS config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

The embedding model and endpoint are global because all repos must use the same model
for cross-repo semantic search to work (vector dimensions must match).

Managed via:

```
commitmux config set embed.model nomic-embed-text
commitmux config set embed.endpoint http://localhost:11434
commitmux config get embed.model
```

Defaults (when not set in `config` table):
- `embed.model`: `nomic-embed-text`
- `embed.endpoint`: `http://localhost:11434`

## New Crate: `crates/embed`

A new workspace crate owns the embedding pipeline. It depends on `crates/types`
(for `CommitDetail` and the `Store` trait) but not on `crates/store` directly —
all store access goes through the trait.

### `Embedder`

```rust
pub struct Embedder {
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
    model: String,
}

impl Embedder {
    pub fn new(endpoint: &str, model: &str) -> Self;
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
```

`Embedder::new` sets `api_base` to the configured endpoint (Ollama or OpenAI)
and `api_key` to `"ollama"` (Ollama ignores the key; OpenAI reads it from env
if a real key is needed).

### `embed_pending`

```rust
pub struct EmbedSummary {
    pub embedded: usize,
    pub skipped: usize,   // already had embeddings
    pub failed: usize,
}

pub async fn embed_pending(
    store: &dyn Store,
    embedder: &Embedder,
    repo_id: i64,
    batch_size: usize,
) -> Result<EmbedSummary>;
```

Queries `commits LEFT JOIN commit_embeddings WHERE commit_embeddings.commit_id IS NULL`
for the given repo. Processes in batches. Writes each embedding back via
`store.store_embedding`. If the endpoint is unreachable, returns `Err` — caller
logs a warning and continues without failing the sync.

### Document construction

```rust
fn build_embed_doc(commit: &CommitDetail) -> String;
```

Constructs the text document for embedding: subject + body + files changed +
patch preview (truncated to ~400 tokens via `len / 4` approximation).

## New `Store` Trait Methods

Add to the `Store` trait in `crates/types/src/lib.rs`:

```rust
// Config table
fn get_config(&self, key: &str) -> Result<Option<String>>;
fn set_config(&self, key: &str, value: &str) -> Result<()>;

// Embedding backfill queue
fn get_commits_without_embeddings(
    &self,
    repo_id: i64,
    limit: usize,
) -> Result<Vec<CommitDetail>>;

// Write embedding result
fn store_embedding(&self, commit_id: i64, embedding: &[f32]) -> Result<()>;
```

SQL implementations live in `crates/store/src/queries.rs`.

## Sync Integration

`commitmux sync` calls `embed_pending` after ingest if `repo.embed_enabled` is set:

```
sync_repo(repo, store, config)
  → walk commits, write to store       ← existing
  → if repo.embed_enabled:
      read embed.model + embed.endpoint from store.get_config()
      embed_pending(store, embedder, repo.repo_id, batch=50)
```

If the embedding endpoint is unavailable, sync logs a warning and proceeds. The commit
index is unaffected. Embeddings can be backfilled later:

```
commitmux sync --embed-only [--repo <name>]
```

## CLI Design

```
# Enable embeddings when adding a repo
commitmux add-repo ~/code/myrepo --embed

# Enable on an existing repo
commitmux update-repo myrepo --embed
commitmux update-repo myrepo --no-embed

# Configure model and endpoint globally
commitmux config set embed.model nomic-embed-text
commitmux config set embed.endpoint http://localhost:11434
commitmux config get embed.model

# Backfill embeddings for already-indexed commits
commitmux sync --embed-only
commitmux sync --embed-only --repo myrepo
```

## `commitmux status` Output

When any repos have `embed_enabled`, the status table gains an `EMBED` column and
a footer line showing the configured model:

```
REPO                 COMMITS  SOURCE                                         LAST SYNCED            EMBED
commitmux                412  /Users/dayna/code/commitmux                    2026-02-28 15:34 UTC   ✓
shelfctl                 203  /Users/dayna/code/shelfctl                     2026-02-28 12:01 UTC   -

Embedding model: nomic-embed-text (http://localhost:11434)
```

The `EMBED` column and footer are omitted entirely when no repos have embeddings enabled
(preserves existing output for users who don't use the feature).

## New MCP Tool

Add `commitmux_search_semantic` alongside the existing `commitmux_search`:

```json
{
  "name": "commitmux_search_semantic",
  "description": "Semantic search over indexed commits using vector similarity. Use when keyword search is insufficient — e.g. 'find commits related to rate limiting' or 'work similar to this description'.",
  "parameters": {
    "query": "Natural language description of what you're looking for",
    "repos": "Optional list of repo names to search within",
    "since": "Optional Unix timestamp lower bound",
    "limit": "Max results (default 10)"
  }
}
```

Returns the same `SearchResult` shape as `commitmux_search`.

The existing `commitmux_search` (FTS5 keyword) and `commitmux_touches` (file path) tools
stay unchanged. `commitmux_search_semantic` is purely additive.

### MCP async consideration

Before implementing `commitmux_search_semantic`, check whether `crates/mcp/src/lib.rs`
already uses a tokio runtime. If yes, the `embed()` call is free. If no, wrap with
`tokio::runtime::Runtime::new().unwrap().block_on(...)` as a contained workaround —
do not refactor the entire MCP server to async as part of this feature.

## Hybrid Retrieval (Future)

A single `commitmux_search` call could transparently do hybrid retrieval — run both
keyword and semantic search, merge results by reciprocal rank fusion (RRF), return the
top N. This would be the most powerful default: keyword precision + semantic recall,
no separate tool for the agent to choose between.

## Wave Structure (Implementation)

Designed for SAW parallel execution:

- **Wave 0** — Schema migration: `embed_enabled` column on `repos`, `config` table,
  `commit_embeddings` virtual table. Single agent, gates all downstream verification.
- **Wave 1** — Two parallel agents:
  - (A) `crates/types/src/lib.rs` + `crates/store/src/queries.rs`: new Store trait
    methods (`get_config`, `set_config`, `get_commits_without_embeddings`,
    `store_embedding`) with SQL implementations.
  - (B) `crates/embed/` (new crate): `Embedder`, `embed_pending`, `build_embed_doc`,
    `EmbedSummary`. New `Cargo.toml` + `src/lib.rs`.
- **Wave 2** — Two parallel agents:
  - (A) `src/main.rs`: `--embed`/`--no-embed` flags, `config` subcommand,
    `--embed-only` sync flag, `status` EMBED column + footer.
  - (B) `crates/mcp/src/lib.rs`: `commitmux_search_semantic` tool.
