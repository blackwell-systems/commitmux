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

{patch_text up to N tokens}
```

Subject + body captures intent. Files changed adds structural signal. Patch text
(truncated) adds implementation detail for similarity matching against code concepts.

Store the embedding vector alongside the commit row in SQLite, using the
[`sqlite-vec`](https://github.com/asg017/sqlite-vec) extension for ANN search,
or in a separate vector store (Qdrant, Chroma, pgvector) if scale demands it.

## Incremental Cost

- **Backfill:** One embedding per commit at ingest. At ~500 tokens per commit,
  4,500 commits ≈ 2.25M tokens. Negligible with a local model (nomic-embed-text,
  all-MiniLM); meaningful but one-time with an API.
- **Incremental sync:** New commits embedded at sync time alongside existing ingest.
  No added latency for already-indexed commits.
- **Storage:** 384-dim float32 (nomic-embed-text) = 1.5KB per commit.
  4,500 commits ≈ 6.8MB. Negligible.

## Embedding Model Options

| Model | Dims | Where runs | Notes |
|---|---|---|---|
| `nomic-embed-text` | 768 | Local (Ollama) | Strong code + prose, free, offline |
| `all-MiniLM-L6-v2` | 384 | Local | Smaller, fast, good for short texts |
| `text-embedding-3-small` | 1536 | OpenAI API | High quality, per-token cost |
| `text-embedding-3-large` | 3072 | OpenAI API | Highest quality, higher cost |

Recommended default: `nomic-embed-text` via Ollama — no API key, runs offline,
strong on both code and natural language.

## CLI Design

```
# Enable embeddings when adding a repo
commitmux add-repo ~/code/myrepo --embed

# Enable embeddings on an existing repo
commitmux update-repo myrepo --embed

# Configure embedding model globally
commitmux config set embed.model nomic-embed-text
commitmux config set embed.endpoint http://localhost:11434  # Ollama default
```

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

The existing `commitmux_search` (FTS5 keyword) and `commitmux_touches` (file path) tools
stay unchanged. `commitmux_search_semantic` is purely additive.

## Hybrid Retrieval (Future)

A single `commitmux_search` call could transparently do hybrid retrieval — run both
keyword and semantic search, merge results by reciprocal rank fusion (RRF), return the
top N. This would be the most powerful default: keyword precision + semantic recall,
no separate tool for the agent to choose between.

## Implementation Notes

- Embeddings are optional infrastructure — commitmux works fully without them. The
  `--embed` flag opts a repo into embedding at add/update time.
- `commitmux sync` generates embeddings for new commits if the repo has `--embed` set.
- If the embedding endpoint is unavailable at sync time, sync proceeds without
  embeddings and logs a warning. Embeddings can be backfilled later with
  `commitmux sync --embed-only`.
- `commitmux status` should show an `EMBED` column indicating whether embeddings
  are enabled per repo.
