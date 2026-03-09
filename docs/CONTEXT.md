# Project Context — commitmux

## features_completed

- slug: memory-freshness
  impl_doc: docs/IMPL/IMPL-memory-freshness.md
  waves: 2
  agents: 4
  date: 2026-03-09
  summary: >
    FTS fallback in commitmux_search_memory MCP tool (Ollama unavailable → search_memory_fts),
    commitmux reindex CLI (delete embeddings + re-embed per repo), commitmux install-memory-hook
    CLI (writes Stop hook to ~/.claude/settings.json), Store::delete_embeddings_for_repo trait
    method with SqliteStore impl (row-by-row vec0 deletes).

- slug: roadmap-p0-p4
  impl_doc: docs/IMPL/IMPL-roadmap.md
  waves: 2
  agents: 5
  date: 2026-03-09
  summary: >
    All P0–P4 post-MVP roadmap features. install-hook CLI, index-impl-docs CLI,
    auto-sync on MCP startup, commitmux_search_saw MCP tool, embedding dimension
    validation, FTS memory search fallback, patch_preview 2000 chars,
    get_patch prefix SHA matching, memory_docs_fts schema.

## decisions

- FTS memory search uses FTS5 content table backed by memory_docs (content='memory_docs',
  content_rowid='doc_id'). FTS index maintained manually in upsert_memory_doc via
  delete + insert pattern (same as commits_fts).

- get_patch now accepts prefix SHAs (LIKE sha || '%') matching get_commit behavior.
  Returns full SHA from DB row, not the prefix passed in.

- Embedding dimension validation is a soft guard at write time (config table key
  'embed.dimension'). No DDL migration to vec0 virtual table needed.

- auto-sync threshold on MCP startup: 3600 seconds (1 hour), hardcoded.

- install-hook writes post-commit hook; guards against overwrite without --force.
  Hook content: commitmux sync --repo "$(git rev-parse --show-toplevel)" 2>/dev/null || true

- commitmux_search_saw builds FTS5 query from feature + optional wave number.
  Uses existing store.search() — no new Store trait method required.

- install-memory-hook writes a Claude Code Stop hook entry to ~/.claude/settings.json.
  Duplicate guard: substring match on "commitmux ingest-memory" in existing Stop hook commands.

- reindex --reset-dim limitation: validate_or_store_dimension treats stored "" or "0" as 0,
  which fails 0 != actual_dim check. Full reset requires delete_config store method or sentinel
  path. Currently prints advisory; user can run `commitmux config set embed.dimension <N>`.

- FTS fallback in call_search_memory: any embed error → search_memory_fts. FTS results returned
  in identical JSON format to vector results (no wrapper object) for caller transparency.

## established_interfaces

- Store::search_memory_fts(query: &str, opts: &MemoryFtsSearchOpts) -> Result<Vec<MemoryMatch>>
- Store::delete_embeddings_for_repo(repo_id: i64) -> Result<()>
- MemoryFtsSearchOpts { project: Option<String>, source_type: Option<String>, limit: Option<usize> }
- MemorySourceType::ImplDoc (as_str: "impl_doc")
- validate_or_store_dimension(store: &dyn Store, embedding: &[f32]) -> anyhow::Result<()>
- CONFIG_KEY_EMBED_DIM: "embed.dimension"
