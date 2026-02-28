# commitmux

[![Blackwell Systems™](https://raw.githubusercontent.com/blackwell-systems/blackwell-docs-theme/main/badge-trademark.svg)](https://github.com/blackwell-systems)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![MCP Compatible](https://img.shields.io/badge/MCP-compatible-blue.svg)](https://modelcontextprotocol.io)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Keyword and semantic search over your git history, exposed as MCP tools for coding agents. Cross-repo, local-first, no credentials, no rate limits.

Agents can search by keyword or describe what they're looking for in natural language. You control what's indexed. Nothing leaves your machine.

## Why commitmux

Agents need prior-work context: how a problem was solved before, what changed in an area, which commit introduced a pattern. The current options are bad:

- **Give the agent `gh` + a token** — unbounded access, rate limits, credential exposure, GitHub-only, no diffs.
- **Give the agent nothing** — it hallucinates or you paste context manually.
- **Paste context yourself** — interrupts flow, doesn't scale.

The cross-repo problem makes this worse: when you maintain 20+ repos and need to know what changed in the auth layer last quarter, `git log` requires you to check each repo manually. commitmux answers cross-repo questions in a single query.

commitmux is a third option. It builds a read-optimized local index over your commit history and exposes it as a narrow, read-only MCP tool surface. Two search modes work together:

- **Full-text search** (FTS5) — fast keyword search over commit subjects, bodies, and patch previews.
- **Semantic search** (vector embeddings) — natural language queries like "find commits related to rate limiting" or "work similar to this description". Powered by any OpenAI-compatible embedding endpoint; works out of the box with [Ollama](https://ollama.com) running locally.

The index lives in a single SQLite file on your machine. The MCP server runs as a subprocess of your agent host. Nothing leaves your machine.

## How it works

commitmux walks your git history using libgit2 (no `git` binary required), stores commits in SQLite with FTS5 full-text indexing over subjects, bodies, and patch previews, and compresses raw diffs with zstd. Semantic search stores float32 embeddings alongside commit metadata — cosine similarity is computed in-process with no external vector database. The MCP server speaks JSON-RPC 2.0 over stdio; your agent host runs it as a subprocess and the five read-only tools become available to the agent.

## Quick start

### Basic setup

```sh
# 1. Build and install
cargo install --path .
```

After installing, ensure `~/.cargo/bin` is on your PATH:

```sh
# Add to your shell profile (~/.zshrc, ~/.bashrc, etc.)
source "$HOME/.cargo/env"
```

Or add `export PATH="$HOME/.cargo/bin:$PATH"` to your shell profile directly.

```sh
# 2. Create the database
commitmux init

# 3. Register repos — local paths or remote URLs
commitmux add-repo ~/code/myproject
commitmux add-repo ~/code/anotherproject --name another

# Remote repos are auto-cloned to ~/.commitmux/clones/<name>/
commitmux add-repo --url git@github.com:org/repo.git

# 4. Ingest commits (fetches from remote first for URL-based repos)
commitmux sync
```

After `sync`, the keyword search index is ready. Configure your agent host to run `commitmux serve` (see [MCP host setup](#mcp-host-setup)) and the MCP tools become available to the agent.

### Enable semantic search (optional)

Semantic search lets agents query by natural language instead of keywords. Requires any OpenAI-compatible embeddings endpoint — works out of the box with [Ollama](https://ollama.com) running locally.

```sh
ollama pull nomic-embed-text
commitmux config set embed.model nomic-embed-text
commitmux add-repo ~/code/myproject --embed   # or: commitmux update-repo myproject --embed
commitmux sync --embed-only                   # backfill embeddings for existing commits
```

To use a hosted provider instead:

```sh
commitmux config set embed.endpoint https://api.openai.com/v1
commitmux config set embed.model text-embedding-3-small
```

### Start the MCP server

```sh
# stdio transport — run by your agent host, not manually in a terminal
commitmux serve
```

## In action

Once the MCP server is running, an agent can query your git history directly. Two examples:

**"Have we implemented rate limiting before?"**

The agent calls `commitmux_search` with `query: "rate limiting"`. commitmux returns matching commits with patch excerpts. The agent calls `commitmux_get_patch` on the most relevant SHA to read the full diff. It builds on the prior implementation instead of starting from scratch.

**"What changed in the auth layer across all repos last month?"**

The agent calls `commitmux_touches` with `path_glob: "auth/"` and a `since` timestamp. commitmux returns commits from every indexed repo — `api-server`, `auth-service`, `web-frontend` — in a single response. No switching between repos, no pasting `git log` output, no token exposure.

**"Find commits related to backpressure and retry logic"** (semantic search)

The agent calls `commitmux_search_semantic` with that natural language query. commitmux embeds the query and returns commits by vector similarity — surfacing relevant work even when the exact words don't appear in the commit message.

## CLI reference

All subcommands accept `--db <path>` to override the database location. See [Configuration](#configuration) for path resolution order.

### `init`

Create the database and schema. Idempotent — safe to run again.

```sh
commitmux init
commitmux init --db /data/commitmux.sqlite3
```

### `add-repo`

Register a git repository. Accepts either a local path or a remote URL via `--url`. The repo name defaults to the directory name (local path) or the repository base name (URL).

```sh
commitmux add-repo <path> [--name <name>] [--exclude <prefix>]...
commitmux add-repo --url <git-url> [--name <name>] [--exclude <prefix>]...
```

```sh
# Use directory name as repo name
commitmux add-repo ~/code/myproject

# Override the name
commitmux add-repo ~/code/myproject --name myproject

# Exclude additional path prefixes on top of the defaults
commitmux add-repo ~/code/myproject --exclude generated/ --exclude proto/

# Add a remote repo (auto-clones to ~/.commitmux/clones/<name>/ on first sync)
commitmux add-repo --url git@github.com:org/repo.git

# Add a remote repo over HTTPS
commitmux add-repo --url https://github.com/org/repo.git --name repo
```

Pass `--embed` to enable semantic embeddings for the repo. Embeddings are generated during `sync` using the configured model.

```sh
# Enable embeddings on registration
commitmux add-repo ~/code/myproject --embed

# Add a remote repo with embeddings
commitmux add-repo --url git@github.com:org/repo.git --embed
```

The `--exclude` flag appends to the default ignore list. Default ignored prefixes: `node_modules/`, `vendor/`, `dist/`, `.git/`.

SSH remotes use the SSH agent for authentication. Ensure your SSH agent is running and has the relevant key loaded (`ssh-add`) before running `sync` against an SSH URL.

### `update-repo`

Update configuration for an already-registered repository. Use this to enable or disable embeddings on a repo that was added before semantic search was configured.

```sh
commitmux update-repo <name> [--embed] [--no-embed]
```

```sh
# Enable embeddings on an existing repo
commitmux update-repo myproject --embed

# Disable embeddings
commitmux update-repo myproject --no-embed
```

After enabling embeddings, run `commitmux sync --embed-only` to backfill existing commits.

### `sync`

Ingest commits from all registered repos, or a single repo. Safe to re-run — upserts on `(repo, sha)`.

```sh
commitmux sync
commitmux sync --repo myproject
commitmux sync --embed-only   # generate embeddings only; skip re-ingesting commits
```

Ingestion walks the default branch only. Commits are skipped if the patch exceeds 1 MB or contains only binary diffs. Run `sync` again at any time to pick up new commits.

For repos registered with `--url`, `sync` automatically fetches from the remote before walking history. No additional flags are needed — a plain `commitmux sync` keeps URL-based repos up to date.

After ingestion, embeddings are automatically generated for any repo with `--embed` enabled. Use `--embed-only` to backfill embeddings without re-walking history (e.g. after enabling embeddings on a repo that was already synced).

### `show`

Print a single commit as JSON. Useful for debugging or verifying ingest.

```sh
commitmux show <repo> <sha>
commitmux show myproject a3f9c12
```

Output matches the `commitmux_get_commit` MCP tool response exactly.

### `status`

Print a table of all registered repos with commit counts and last sync times.

```sh
commitmux status
```

```
REPO                  COMMITS  SOURCE                                             LAST SYNCED             EMBED
myproject                2341  /Users/you/code/myproject                         2026-02-28 14:03:17 UTC  ✓
another                   892  https://github.com/org/another.git                2026-02-28 14:03:51 UTC  -

Embedding model: nomic-embed-text (http://localhost:11434/v1) — ✓ = enabled
```

### `config`

Read and write named configuration values. Used primarily to configure the embedding model and endpoint.

```sh
commitmux config get <key>
commitmux config set <key> <value>
```

Supported keys:

| Key | Default | Description |
|-----|---------|-------------|
| `embed.model` | `nomic-embed-text` | Embedding model name passed to the API |
| `embed.endpoint` | `http://localhost:11434/v1` | OpenAI-compatible embeddings endpoint |

```sh
# Use a different Ollama model
commitmux config set embed.model mxbai-embed-large

# Point at a remote OpenAI-compatible endpoint
commitmux config set embed.endpoint https://api.openai.com/v1
commitmux config set embed.model text-embedding-3-small
```

Configuration is stored in the database. Values persist across commands.

### `serve`

Start the MCP server on stdio. This is the command your agent host runs — not meant to be invoked directly in a terminal.

```sh
commitmux serve
commitmux serve --db /data/commitmux.sqlite3
```

The server reads newline-delimited JSON-RPC from stdin and writes responses to stdout. It runs until stdin is closed.

## MCP tools reference

The server exposes five tools. All tools are read-only.

### `commitmux_search_semantic`

Natural language semantic search over commit history using vector similarity. Use when keyword search is insufficient — e.g. "find commits related to error handling" or "work similar to this description". Only returns results for repos with embeddings enabled.

Requires any OpenAI-compatible embeddings endpoint. Works out of the box with [Ollama](https://ollama.com) running locally; supports OpenAI and any compatible provider. Configure with `commitmux config set embed.endpoint <url>` and `commitmux config set embed.model <model>`.

**Input schema:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | Natural language description of what you're looking for |
| `since` | integer | no | Unix timestamp lower bound on author date |
| `repos` | string[] | no | Restrict to these repo names |
| `limit` | integer | no | Max results. Default: 10 |

**Example call:**

```json
{
  "name": "commitmux_search_semantic",
  "arguments": {
    "query": "rate limiting and backpressure",
    "repos": ["api-server"],
    "limit": 5
  }
}
```

**Example output:**

```json
[
  {
    "repo": "api-server",
    "sha": "a3f9c12b4e77d",
    "subject": "Add token bucket rate limiter to middleware stack",
    "author": "Dayna Blackwell",
    "date": 1740700997,
    "score": 0.91,
    "patch_excerpt": "diff --git a/src/middleware/rate_limit.rs ..."
  }
]
```

Results include a `score` field (0–1) indicating similarity to the query. Higher is more similar.

### `commitmux_search`

Full-text search over commit subjects, bodies, and patch previews (first 500 characters of each diff). Uses SQLite FTS5.

**Input schema:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | FTS5 query string |
| `since` | integer | no | Unix timestamp lower bound on author date |
| `repos` | string[] | no | Restrict to these repo names |
| `paths` | string[] | no | Restrict to commits touching paths containing these substrings |
| `limit` | integer | no | Max results. Default: 20 |

**Example call:**

```json
{
  "name": "commitmux_search",
  "arguments": {
    "query": "rate limiting middleware",
    "repos": ["api-server"],
    "limit": 5
  }
}
```

**Example output:**

```json
[
  {
    "repo": "api-server",
    "sha": "a3f9c12b4e77d",
    "subject": "Add token bucket rate limiter to middleware stack",
    "author": "Dayna Blackwell",
    "date": 1740700997,
    "matched_paths": ["src/middleware/rate_limit.rs", "src/middleware/mod.rs"],
    "patch_excerpt": "diff --git a/src/middleware/rate_limit.rs b/src/middleware/rate_limit.rs\nnew file mode 100644\n+use std::sync::Arc;\n+use tokio::sync::Semaphore;"
  }
]
```

### `commitmux_touches`

Find commits that touched a specific file or path pattern. Uses substring matching on stored paths.

**Input schema:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `path_glob` | string | yes | Substring to match against file paths |
| `since` | integer | no | Unix timestamp lower bound on author date |
| `repos` | string[] | no | Restrict to these repo names |
| `limit` | integer | no | Max results. Default: 50 |

**Example call:**

```json
{
  "name": "commitmux_touches",
  "arguments": {
    "path_glob": "src/auth/",
    "since": 1735689600
  }
}
```

**Example output:**

```json
[
  {
    "repo": "api-server",
    "sha": "b8c21d3f9a",
    "subject": "Migrate auth tokens to short-lived JWTs",
    "date": 1740611200,
    "path": "src/auth/tokens.rs",
    "status": "M"
  },
  {
    "repo": "api-server",
    "sha": "c4e87f2110",
    "subject": "Add refresh token rotation",
    "date": 1739900000,
    "path": "src/auth/refresh.rs",
    "status": "A"
  }
]
```

File status values: `A` (added), `M` (modified), `D` (deleted), `R` (renamed), `C` (copied).

### `commitmux_get_commit`

Retrieve full metadata for a specific commit, including the list of changed files.

**Input schema:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `repo` | string | yes | Repo name as registered with `add-repo` |
| `sha` | string | yes | Commit SHA (full or partial) |

**Example call:**

```json
{
  "name": "commitmux_get_commit",
  "arguments": {
    "repo": "api-server",
    "sha": "a3f9c12b4e77d"
  }
}
```

**Example output:**

```json
{
  "repo": "api-server",
  "sha": "a3f9c12b4e77d831290ab45c6de1f8e3",
  "subject": "Add token bucket rate limiter to middleware stack",
  "body": "Fixes #482. Uses a per-IP token bucket with a 100 req/min default.\nBucket capacity and refill rate are configurable via environment variables.",
  "author": "Dayna Blackwell",
  "date": 1740700997,
  "changed_files": [
    { "path": "src/middleware/rate_limit.rs", "status": "A", "old_path": null },
    { "path": "src/middleware/mod.rs", "status": "M", "old_path": null },
    { "path": "tests/middleware_test.rs", "status": "M", "old_path": null }
  ]
}
```

### `commitmux_get_patch`

Retrieve the raw unified diff for a commit. Patches are stored zstd-compressed and decompressed on retrieval. Use `max_bytes` to limit response size when dealing with large commits.

**Input schema:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `repo` | string | yes | Repo name |
| `sha` | string | yes | Commit SHA |
| `max_bytes` | integer | no | Truncate patch text to this many bytes |

**Example call:**

```json
{
  "name": "commitmux_get_patch",
  "arguments": {
    "repo": "api-server",
    "sha": "a3f9c12b4e77d",
    "max_bytes": 8000
  }
}
```

**Example output:**

```json
{
  "repo": "api-server",
  "sha": "a3f9c12b4e77d831290ab45c6de1f8e3",
  "patch_text": "diff --git a/src/middleware/rate_limit.rs b/src/middleware/rate_limit.rs\nnew file mode 100644\nindex 0000000..f3a2c81\n--- /dev/null\n+++ b/src/middleware/rate_limit.rs\n@@ -0,0 +1,47 @@\n+use std::sync::Arc;\n+..."
}
```

Commits with patches larger than 1 MB at ingest time have their patch skipped. Binary-only diffs are also skipped. `commitmux_get_commit` will still return metadata and file list for those commits.

## Configuration

The database path is resolved in this order:

1. `--db <path>` flag (takes precedence over everything)
2. `COMMITMUX_DB` environment variable
3. `~/.commitmux/db.sqlite3` (default)

```sh
# Flag
commitmux sync --db /data/mydb.sqlite3

# Environment variable
export COMMITMUX_DB=/data/mydb.sqlite3
commitmux sync

# Default — no configuration needed
commitmux sync
```

## MCP host setup

### Claude Desktop

Add commitmux to `claude_desktop_config.json`. The file is typically at `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS.

```json
{
  "mcpServers": {
    "commitmux": {
      "command": "commitmux",
      "args": ["serve"]
    }
  }
}
```

If the `commitmux` binary is not on Claude Desktop's PATH, use the full path:

```json
{
  "mcpServers": {
    "commitmux": {
      "command": "/Users/you/.cargo/bin/commitmux",
      "args": ["serve"]
    }
  }
}
```

To use a non-default database:

```json
{
  "mcpServers": {
    "commitmux": {
      "command": "commitmux",
      "args": ["serve", "--db", "/data/commitmux.sqlite3"]
    }
  }
}
```

### Other MCP hosts

Any MCP host that supports stdio transport can run commitmux. The server command is `commitmux serve`. It speaks MCP protocol version `2024-11-05` over stdin/stdout as newline-delimited JSON-RPC 2.0.

Example for a generic host configuration:

```json
{
  "command": "commitmux",
  "args": ["serve"],
  "transport": "stdio"
}
```

See [docs/mcp.md](docs/mcp.md) for the full MCP integration reference, including security model, freshness considerations, and raw protocol examples.

## Implementation notes

- Uses [git2](https://github.com/rust-lang/git2-rs) (libgit2 bindings) for commit ingestion. No `git` binary required.
- Patches stored as zstd-compressed blobs (level 3). FTS5 index covers subject, body, and the first 500 characters of each patch.
- SQLite WAL mode enabled. The database is safe for reads during a concurrent sync.
- The MCP server is synchronous (no async runtime). Each request is handled inline on the main thread.
- Embeddings are stored as raw float32 blobs in SQLite alongside commit metadata. Similarity search uses cosine distance computed in-process — no separate vector database required.
- Embedding API calls use [async-openai](https://github.com/64bit/async-openai) against any OpenAI-compatible `/v1/embeddings` endpoint. Works with Ollama, OpenAI, and compatible providers.
