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
