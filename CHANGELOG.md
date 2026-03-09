# Changelog

All notable changes to commitmux will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`commitmux install-memory-hook`** — registers `commitmux ingest-memory` as a Claude Code `Stop` hook in `~/.claude/settings.json`. Memory files are automatically ingested and embedded after every Claude Code session, keeping semantic search up-to-date with no manual steps. Duplicate guard prevents double-registration. Writes the absolute binary path so the hook works in non-interactive shells where `~/.cargo/bin` may not be on `PATH`.

- **`commitmux reindex [--repo NAME]`** — deletes all embeddings for one or all repositories and re-embeds from scratch. Use when switching embedding models or after bulk history imports. `--reset-dim` flag prints an advisory to manually clear `embed.dimension` (full automated reset requires a future `delete_config` store method).

- **`Store::delete_embeddings_for_repo`** — new store trait method that clears all embedding data for a given repo (both `commit_embeddings` vec0 rows and `commit_embed_map` entries). Deletes from the sqlite-vec virtual table row-by-row per embed_id to satisfy vec0 constraints.

- **FTS fallback in `commitmux_search_memory`** — if Ollama is unreachable or returns an error during query embedding, the MCP tool transparently falls back to FTS5 keyword search and returns results in the same JSON format. Callers receive useful results regardless of whether the embedding service is running.

- **`commitmux install-hook <repo>`** — writes a `post-commit` git hook to `.git/hooks/post-commit` that calls `commitmux sync` after every commit, keeping the index fresh without manual intervention. `--force` flag to overwrite an existing hook.

- **`commitmux index-impl-docs <path>`** — indexes SAW protocol IMPL docs (`docs/IMPL/IMPL-*.md`) from a working tree into the memory search index. Enables agents to search prior planning documents by content. Uses `--project` to tag results; defaults to directory name.

- **Auto-sync on MCP server startup** — `commitmux serve` now checks each indexed repo's `last_synced_at` on startup and syncs any repo stale by more than 1 hour before entering the JSON-RPC loop. All output to stderr to avoid polluting MCP stdout.

- **`commitmux_search_saw` MCP tool** — searches commit history for SAW (Scout-and-Wave) protocol merge commits by feature name and optional wave number. Constructs the right FTS5 query internally; callers pass `feature` and optionally `wave` (integer). Returns results ranked by relevance.

- **FTS5 keyword search over memory docs** — `commitmux_search_memory` now has a keyword fallback (`commitmux_search_memory` still vector-first; new `search_memory_fts` internal method provides FTS5 over `memory_docs.content`). Works without Ollama. New `memory_docs_fts` virtual table maintained automatically on every `upsert_memory_doc`.

- **`ImplDoc` memory source type** — `MemorySourceType::ImplDoc` (`"impl_doc"`) for documents indexed via `index-impl-docs`, distinct from `MemoryFile` entries.

### Changed

- **`patch_preview` cap raised 500 → 2000 chars** — FTS5 search now indexes up to 2000 characters of each commit's diff preview, improving search recall for large commits and commits with bodies embedded in the diff (e.g. SAW completion reports).

- **`commitmux_touches` description clarified** — MCP tool schema now documents that `path_glob` uses substring matching (`LIKE %pattern%`), not shell glob syntax. Glob patterns like `src/**/*.rs` will not work; use `src/` or `.rs` instead.

### Fixed

- **`commitmux_get_patch` now accepts prefix SHAs** — previously required an exact full SHA, while `commitmux_get_commit` accepted prefix SHAs. Both tools now accept the same short SHA format. The returned `sha` field is always the full SHA from the database.

- **Embedding dimension mismatch now errors explicitly** — switching embedding models with an existing index previously silently mixed incompatible vectors, producing nonsense ANN results. `commitmux embed` now validates the embedding dimension against the value stored in the config table on first use and returns a clear error with remediation instructions if dimensions differ.

- **CI workflow** — `.github/workflows/ci.yml` runs on every push and PR to `main`. Three sequential jobs: `Lint & Format` (rustfmt check + clippy `-D warnings`), `Test` (`cargo test --workspace`), `Build` (cross-compile check against linux/darwin × amd64/arm64 via `cargo check`). Uses `dtolnay/rust-toolchain@stable` and `Swatinem/rust-cache` for fast incremental builds.

- **Memory search**: `commitmux_search_memory` MCP tool — semantic search over claudewatch memory files (session summaries, tasks, blockers, decisions). Enables AI agents to find prior context and solutions across all projects by meaning, not just keywords. Uses the same embedding infrastructure as commit search.
- `ingest-memory [--claude-home PATH]`: scans `~/.claude/projects/*/memory/*.md` and indexes memory documents for semantic search. Incremental: tracks file modification time and only re-embeds changed files. Automatically generates embeddings after ingestion.
- Memory document storage: new `memory_docs`, `memory_embed_map`, and `memory_embeddings` tables in SQLite store. Source types: `session_summary`, `task`, `blocker`, `memory_file`, `decision`.
- **Semantic search**: `commitmux_search_semantic` MCP tool — natural language search over commit history using vector embeddings. Finds commits by intent, not just keywords. Powered by any OpenAI-compatible embedding endpoint (Ollama by default).
- `add-repo --embed`: enable semantic embeddings when registering a repo.
- `update-repo --embed` / `update-repo --no-embed`: enable or disable embeddings on an existing repo.
- `sync --embed-only`: generate embeddings for already-indexed commits without re-ingesting. Useful when embeddings are enabled after initial sync.
- `config get <key>` / `config set <key> <value>`: read and write named configuration values. Currently supported keys: `embed.model` (default: `nomic-embed-text`) and `embed.endpoint` (default: `http://localhost:11434/v1`).
- `commitmux status` EMBED column: shows `✓` for repos with embeddings enabled, `-` for disabled. Footer shows the active embedding model and endpoint.
- `add-repo --url <git-url>` flag: register a remote repository by URL. commitmux auto-clones the repo to `~/.commitmux/clones/<name>/` on first sync.
- Automatic remote fetch before ingest when a repo was registered with `--url`. Running `commitmux sync` on a URL-based repo pulls the latest commits from the remote before walking history.
- SSH agent authentication support for clone and fetch operations against SSH remotes.
- MIT license.
- MCP integration reference (`docs/mcp.md`): full tool surface, host configuration examples for Claude Desktop, Cursor, and Zed, security model, freshness considerations, and raw protocol examples.
- README with quick-start guide, full CLI reference, MCP tools reference, configuration, and MCP host setup instructions.
- `update-repo` command: update stored metadata for a repository (name, author filter, exclude prefixes, fork-of URL, default branch).
- `add-repo --fork-of <url>`: only index commits not present in the upstream repo, using merge-base exclusion.
- `add-repo --author <email>`: only index commits by a specific author (email substring match). Also available on `update-repo`.
- `add-repo --exclude <prefix>`: exclude a path prefix from indexing (repeatable). Also available on `update-repo`.
- `commitmux_list_repos` MCP tool: returns all indexed repositories with commit counts and last-synced timestamps.
- Incremental sync: subsequent `sync` runs skip commits already present in the index, reporting `already indexed` counts separately from newly indexed counts.
- `--version` flag: `commitmux --version` now returns the build version from Cargo.toml.
- Descriptions for all subcommands and flags in `--help` output.
- SOURCE column in `commitmux status`: shows remote URL or local path for each repo so custom `--name` entries can be traced back to their source.
- Active filter display in `commitmux status`: repos with `--author` or `--exclude` filters show an indented `filters:` line.
- Empty-state message in `commitmux status`: when no repos are indexed, prints a hint to run `commitmux add-repo`.
- MCP onboarding tip after a successful sync: `Tip: run 'commitmux serve' to expose this index via MCP to AI agents.`
- `remove-repo` now reports the number of commits deleted from the index.
- Git repository validation on `add-repo <path>`: fails immediately with a clear error if the path is not a git repository, rather than deferring the error to sync time.
- URL scheme validation on `add-repo --url`: rejects unrecognized schemes before attempting a clone.

### Changed

- `commitmux sync` output now distinguishes between commits skipped because they were already indexed (`already indexed`) and commits skipped by an author filter (`filtered by author`). Previously both were reported as `skipped`.
- `commitmux sync` exits with code 1 if any repository fails to sync, enabling scripts and CI to detect partial failures.
- `commitmux show` not-found error now includes the repo name and SHA searched: `Commit 'abc123' not found in repo 'myrepo'`.
- `commitmux status` timestamps now include a UTC label (e.g. `2026-02-28 15:34:55 UTC`).
- `commitmux show` date field is now an ISO 8601 UTC string (e.g. `"2026-02-28T15:34:55Z"`) instead of a raw Unix timestamp integer.
- Duplicate `add-repo` name error now shows a clean message (`A repo named '...' already exists`) instead of exposing the raw SQLite UNIQUE constraint error.

### Fixed

- Memory search kNN query now routes through `memory_embed_map` table instead of selecting auxiliary columns directly from the vec0 virtual table. sqlite-vec doesn't support selecting auxiliary columns (`+doc_id`, `+source`) in kNN subquery output.
- MCP test mock for `search` was missing the `score` field added to `SearchResult` for semantic search results. Added `score: None` to the mock struct literal.
- `embed_pending` now fail-fasts on Ollama connection errors instead of printing one error per commit and exiting 0. A single actionable message is shown with the configured endpoint and instructions to run `ollama serve`.
- `config set` now validates keys against a known allowlist and rejects empty values, rather than silently accepting invalid configuration.
- `--embed` and `--no-embed` are now mutually exclusive at the CLI level (clap `conflicts_with`); previously passing both flags silently resolved to the last one.
- `commitmux serve` now prints a startup confirmation to stderr so it is clear the server is running.
- `commitmux show` not-found error now includes `Error:` prefix and the repo name and SHA searched.
- PATH guidance added to README and quick start: `~/.cargo/bin` must be on `PATH` for the binary to be found in non-interactive shells (e.g. agent host environments).
- `add-repo` command was missing the `--db` flag, preventing database path override.
- Panic on patch preview retrieval when a multi-byte UTF-8 character fell on a truncation boundary.
- SSH agent authentication failures when cloning or fetching from SSH remotes.
- License copyright attribution.

[Unreleased]: https://github.com/blackwell-systems/commitmux/commits/master
