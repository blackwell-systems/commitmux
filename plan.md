---
title: "commitmux"
version: "seed-0.2"
status: "idea"
last_updated: "2026-02-28"
---

# commitmux — Seed Document

## One-liner
**commitmux** is a local MCP server that gives coding agents structured, bounded, credential-free access to your git history across all your repos.

## What problem this solves
Coding agents need prior-work context: "how did I solve this before," "what changed in this area," "find the commit that introduced this pattern." The options today are bad:

- Give the agent `gh` CLI + a token → unbounded access, rate limits, credential exposure, GitHub-only, no diffs
- Give the agent nothing → it hallucinates or asks you to paste context manually
- Paste context yourself → interrupts the flow, doesn't scale

commitmux is a third option: a **read-only, local, structured retrieval surface** over your entire commit history. The agent calls it like a function. You control what's indexed.

## Non-goals
- Not a tool for humans to browse git history (CLI is admin/debug only).
- Not a monorepo or repo merger.
- Not a Git hosting platform.
- Not a general code search engine.
- Not "store full file contents for every commit."
- Not a vector DB product (embeddings are later, maybe never).
- Not dependent on GitHub or any remote.

## Target user
A developer with many repos who uses AI coding agents regularly and wants those agents to have reliable, bounded context about prior work — without handing them live credentials or letting them thrash an external API.

## The mental model
- Each repo stays independent. commitmux never touches the Git graph.
- commitmux builds a **read-optimized local index** over commit metadata and patches.
- An MCP server exposes that index as a narrow, read-only tool surface.
- The agent calls tools. commitmux returns structured results. No credentials, no network, no rate limits.

## MCP tool surface (the product)

```
commitmux.search(query, since?, repos?, paths?, limit?)
commitmux.touches(path_glob, since?, repos?)
commitmux.get_commit(repo, sha)
commitmux.get_patch(repo, sha, max_bytes?)
```

### Response shape (design from here, work backwards)
Each tool returns compact, structured JSON optimized for agent consumption:
- `search` → array of `{ repo, sha, subject, author, date, matched_paths[], patch_excerpt }`
- `touches` → array of `{ repo, sha, subject, date, path, status }`
- `get_commit` → `{ repo, sha, subject, body, author, date, changed_files[] }`
- `get_patch` → `{ repo, sha, patch_text }` (truncated at `max_bytes`)

No pagination in MVP. Use `limit` and `since` to keep responses bounded.

## Data model (minimum to serve MCP tools)

### repos
- repo_id (pk)
- name
- local_path
- remote_url (optional)
- default_branch (optional)

### commits
- repo_id, sha (pk)
- author_name, author_email
- author_time, commit_time
- subject, body
- parent_count

### commit_files
- repo_id, sha
- path
- status (A/M/D/R)
- old_path (nullable, for renames)

### commit_patches
- repo_id, sha
- patch_blob (zstd-compressed raw patch)
- patch_preview (first ~500 chars of patch text, uncompressed, for fast excerpt)

### ingest_state
- repo_id
- last_synced_at
- last_synced_sha
- last_error (nullable)

### Full-text index
- FTS5 table over: subject, body, patch_preview

## Ingestion strategy

For each repo:
- Enumerate commits on default branch only (MVP).
- For each commit, collect:
  - metadata via `git show -s --format=...`
  - changed paths + statuses via `git show --name-status`
  - patch via `git show --patch` (cap at 1MB per commit; skip binary diffs)
- Upsert keyed on `(repo_id, sha)` — safe to re-run.
- Skip: binary diffs, commits where patch exceeds cap, paths matching ignore rules.

### Ignore rules (MVP)
Config file per repo (or global default) to exclude path prefixes:
- `node_modules/`, `vendor/`, `dist/`, `*.lock`, generated file patterns.
- Without this, large repos will bloat the index and pollute search results.

### Freshness (day-one requirement)
- MCP server syncs any stale repos on startup (last sync > threshold, e.g. 1 hour).
- `commitmux sync` CLI command for manual or scripted refresh.
- A stale index is worse than no index — agents will trust wrong results.

## CLI (admin and infrastructure only)

```
commitmux init                  # create DB
commitmux add-repo <path>       # register a repo
commitmux sync [--repo ...]     # ingest/update commits
commitmux show <repo> <sha>     # inspect a commit (debug)
commitmux status                # show index health, last sync times
```

No human-optimized search UX. No stats command. No pretty output. The CLI is not the product.

## Security / privacy
- Local-first. No network calls for any core feature.
- MCP server is read-only by design — no tools that write, delete, or modify.
- Ignore rules prevent secrets and generated noise from entering the index.
- No credentials stored or required.

## "Prove usefulness" test
The MVP is successful if an agent, given only commitmux MCP access:
1. Finds a relevant prior commit by description ("how did I handle X before").
2. Retrieves the actual patch and uses it to inform a new change.
3. Does this without any manual context-pasting from the developer.

If this works reliably in one real agent session, it's worth continuing.

## First milestone checklist
- [ ] `commitmux init` (create DB + schema)
- [ ] `commitmux add-repo <path>` (register repo with ignore rules)
- [ ] `commitmux sync` (ingest commits, idempotent)
- [ ] FTS5 over subject/body/patch_preview
- [ ] MCP server with `search`, `touches`, `get_commit`, `get_patch`
- [ ] Auto-sync on MCP server start if index is stale
- [ ] Basic ignore rules (path prefix excludes)
- [ ] Binary diff and oversize patch handling
- [ ] One end-to-end test: agent calls `search`, gets useful context, uses it

## Open questions
- Which compression for patch_blob: zstd or zlib? (pick one, don't defer)
- Default staleness threshold for auto-sync: 1 hour? configurable?
- Should `search` FTS include patch_preview by default, or only on a flag?
- Which refs to ingest post-MVP: all branches, or default only forever?

---

## Roadmap (post-MVP)

*Added 2026-03-08 based on deep code review and real-world SAW protocol usage.*

### P0 — Freshness (the fundamental gap)

The tool is a **passive index** requiring manual `commitmux sync`. For agents asking
about in-flight development this is the wrong model. An agent asking "what did we just
implement?" needs data from right now, not from the last time a human remembered to sync.

**`commitmux install-hook`** — writes a `post-commit` git hook to `.git/hooks/` that
calls `commitmux sync --repo <path>` after every commit. Should be a first-class install
step, not left to the user to figure out. Three lines of shell but it changes the
freshness model from "batch" to "live."

**Auto-sync on MCP startup** was listed as a day-one requirement in the seed doc but
the current implementation does not do it. The MCP server opens the DB and immediately
serves — it does not check `last_synced_at` or trigger a sync pass. Either implement
the startup sync or document that the hook approach is the intended freshness mechanism.

---

### P1 — SAW Protocol Integration

commitmux is installed in a SAW-heavy workflow. SAW produces structured merge commit
messages (`Merge wave{N}-agent-{X}: description`) that are machine-parseable but treated
as plain text by the current FTS index.

**`commitmux_search_saw` MCP tool** — takes `feature` (string) and `wave` (int,
optional) params, constructs the right FTS5 query internally, and returns results
grouped by wave/agent. Agents should not need to know FTS5 syntax to find their own
wave history.

**IMPL doc indexing** — SAW IMPL docs live in `docs/IMPL/IMPL-*.md` in the working
tree, not as standalone memory files. They contain wave execution history, completion
reports, and interface contracts — exactly the kind of cross-session context agents
need. The existing `memory_docs` infrastructure can handle this; it just needs an
`--include-docs` flag on `ingest-memory` (or a new `commitmux index-impl-docs` command)
that reads file content from the working tree rather than the git history.

---

### P2 — Tool API Improvements

**`touches` glob fix** — the `path_glob` parameter does `LIKE %pattern%` internally.
`src/**/*.rs` will not match `.rs` files; it will match the literal string `src/**/*.rs`
which exists nowhere. Either implement real glob matching (`glob` crate), or rename the
parameter to `path_substring` and document the actual behavior. The current API is
silently wrong for any caller using glob syntax.

**SHA consistency between tools** — `commitmux_get_commit` accepts prefix SHAs
(`LIKE sha || '%'`), but `commitmux_get_patch` requires an exact SHA. An agent that
calls `get_commit("abc12")` and then `get_patch("abc12")` with the same input will fail.
`get_patch` should accept the same prefix matching as `get_commit`, or the `get_commit`
response should document that the returned `sha` field (full SHA) is what `get_patch`
requires.

**FTS covers only 500 chars of diff** — `patch_preview` (stored in `commits` for FTS)
is capped at 500 characters at ingest time. Long commit diffs — including SAW completion
reports embedded in commit bodies — are truncated. The full patch is stored compressed
in `commit_patches` but is not FTS-indexed. Consider raising the FTS preview to 2000
chars, or adding a separate FTS pass over full decompressed patches for commits where
the body is long.

---

### P3 — Memory Subsystem

**Automatic memory ingestion** — `commitmux ingest-memory` must be run manually.
For the claudewatch/MEMORY.md workflow, this means memory is only as fresh as the
last manual run. Options:
1. A `post-session` Claude Code hook that calls `commitmux ingest-memory` after each session
2. A file watcher daemon (`commitmux watch-memory`) using FSEvents (macOS) / inotify (Linux)
3. Document the hook approach explicitly so users know how to wire it

**FTS over memory docs** — `commitmux_search_memory` is vector-only. If Ollama is not
running, memory search fails entirely with no fallback. Adding FTS5 over `memory_docs.content`
(same pattern as `commits_fts`) would give keyword search as a fallback and make the tool
usable without Ollama.

---

### P4 — Embedding Model Flexibility

The schema hardcodes `FLOAT[768]` for both `commit_embeddings` and `memory_embeddings`.
This matches `nomic-embed-text` but breaks silently if the user switches to a model
with different dimensions — old and new vectors co-exist in the same table and ANN
results become nonsense. Options:
1. Store the embedding dimension in the `config` table and validate on write
2. On model change, require a `commitmux reindex --repo <name>` to rebuild the vector table
3. Namespace by model in the table (complex)

At minimum, `commitmux embed` should error if the embedding dimension does not match
the dimension stored for existing vectors, rather than silently mixing incompatible vectors.
