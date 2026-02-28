# commitmux MCP integration

This document covers the MCP server in depth: the problem it solves, the full tool surface, host configuration, the security model, and freshness considerations.

For a quick-start, see the [README](../README.md).

## The problem

Coding agents need access to prior work. The two naive approaches both fail:

**Giving the agent `gh` CLI or a GitHub token** has several problems:
- Exposes credentials to the agent process.
- GitHub rate limits apply — a session doing many lookups will hit them.
- Only works for repos hosted on GitHub.
- Raw GitHub API responses for commits omit diffs or require a separate call.
- The agent can do more than you intended (search across orgs, read private repos, etc.).

**Pasting context manually** breaks flow and doesn't scale across sessions.

commitmux provides a third option: a local index that the agent queries like a database. The index is built from your local clone, not from a remote. The MCP server exposes a small, fixed tool surface. The agent cannot do anything outside that surface.

## Architecture

```
  agent host (Claude Desktop, etc.)
       |
       | spawns subprocess (stdio)
       v
  commitmux serve
       |
       | reads from SQLite
       v
  ~/.commitmux/db.sqlite3
       |
       | populated by
       v
  commitmux sync  (run separately, uses libgit2)
       |
       v
  local git repos on disk
```

- The agent never touches the git repos directly.
- The MCP server never touches the network.
- `commitmux sync` is the only component that reads git history. It runs as a separate CLI command or can be scheduled.

## Protocol

- Transport: stdio (stdin/stdout)
- Framing: newline-delimited JSON-RPC 2.0
- MCP protocol version: `2024-11-05`
- Capabilities advertised: `{ "tools": {} }`

The server handles three JSON-RPC methods:

| Method | Description |
|--------|-------------|
| `initialize` | Handshake. Returns server info and protocol version. |
| `tools/list` | Returns the list of available tools with input schemas. |
| `tools/call` | Dispatches a tool call by name. |

Notifications (messages without an `id` field) are silently ignored per the JSON-RPC 2.0 spec.

### Initialize handshake

Request:
```json
{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}
```

Response:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": { "tools": {} },
    "serverInfo": { "name": "commitmux", "version": "0.1.0" }
  }
}
```

### Tool call envelope

All tool calls use `tools/call` with a `name` and `arguments` field:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "commitmux_search",
    "arguments": { "query": "rate limiter", "limit": 5 }
  }
}
```

Successful response:
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "content": [{ "type": "text", "text": "[{\"repo\":\"api-server\", ...}]" }],
    "isError": false
  }
}
```

Error response (tool-level error, not transport-level):
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "content": [{ "type": "text", "text": "Commit api-server:abc123 not found" }],
    "isError": true
  }
}
```

The `text` field always contains JSON for successful calls. For errors it contains a plain error message string.

## Tools

### `commitmux_search`

Full-text search over commit subjects, commit bodies, and patch previews. The FTS5 index covers the first 500 characters of each commit's diff in addition to the full subject and body.

**Input schema:**

```json
{
  "type": "object",
  "properties": {
    "query":  { "type": "string",  "description": "FTS5 query string" },
    "since":  { "type": "integer", "description": "Unix timestamp lower bound on author date" },
    "repos":  { "type": "array", "items": { "type": "string" }, "description": "Filter by repo names" },
    "paths":  { "type": "array", "items": { "type": "string" }, "description": "Filter by path substrings" },
    "limit":  { "type": "integer", "description": "Max results (default 20)" }
  },
  "required": ["query"]
}
```

The `query` field is passed directly to SQLite FTS5 MATCH. Standard FTS5 syntax applies: `rate limiter` matches both words anywhere in the indexed text, `"rate limiter"` requires the phrase, `rate*` is a prefix match.

The `paths` filter is applied after FTS matching. Only commits that touched at least one file whose path contains any of the given substrings are included.

**Output: array of search results**

```json
[
  {
    "repo": "api-server",
    "sha": "a3f9c12b4e77d831290ab45c6de1f8e3",
    "subject": "Add token bucket rate limiter to middleware stack",
    "author": "Dana Blackwell",
    "date": 1740700997,
    "matched_paths": [
      "src/middleware/mod.rs",
      "src/middleware/rate_limit.rs",
      "tests/middleware_test.rs"
    ],
    "patch_excerpt": "diff --git a/src/middleware/rate_limit.rs b/src/middleware/rate_limit.rs\nnew file mode 100644\nindex 0000000..f3a2c81\n--- /dev/null\n+++ b/src/middleware/rate_limit.rs\n@@ -0,0 +1,47 @@\n+use std::sync::Arc;"
  }
]
```

`date` is a Unix timestamp (integer seconds, UTC). `patch_excerpt` is the first 300 characters of the stored patch preview.

**Typical agent usage:**

The agent searches for a concept, scans subjects and excerpts to identify relevant commits, then calls `commitmux_get_patch` on the most relevant SHA to retrieve the full diff.

---

### `commitmux_touches`

Find commits that touched a path matching a substring. Results are ordered by author date descending.

**Input schema:**

```json
{
  "type": "object",
  "properties": {
    "path_glob": { "type": "string",  "description": "Substring to match against file paths" },
    "since":     { "type": "integer", "description": "Unix timestamp lower bound on author date" },
    "repos":     { "type": "array", "items": { "type": "string" } },
    "limit":     { "type": "integer", "description": "Max results (default 50)" }
  },
  "required": ["path_glob"]
}
```

Despite the name, `path_glob` is a substring match (SQL `LIKE %pattern%`), not a glob. `src/auth/` matches any path containing that string.

**Output: array of touch results**

```json
[
  {
    "repo": "api-server",
    "sha": "b8c21d3f9a44fe",
    "subject": "Migrate auth tokens to short-lived JWTs",
    "date": 1740611200,
    "path": "src/auth/tokens.rs",
    "status": "M"
  },
  {
    "repo": "api-server",
    "sha": "c4e87f211099ba",
    "subject": "Add refresh token rotation",
    "date": 1739900000,
    "path": "src/auth/refresh.rs",
    "status": "A"
  }
]
```

Each result is one file from one commit. A commit that touched three matching paths produces three results.

Status codes:

| Code | Meaning |
|------|---------|
| `A` | Added |
| `M` | Modified |
| `D` | Deleted |
| `R` | Renamed (old path available in `commitmux_get_commit`) |
| `C` | Copied |

**Typical agent usage:**

The agent uses `commitmux_touches` to answer "what commits recently changed this area of the codebase?" before making a related change, to understand context and avoid conflicts.

---

### `commitmux_get_commit`

Retrieve full metadata for a single commit. Includes the complete file list with status codes.

**Input schema:**

```json
{
  "type": "object",
  "properties": {
    "repo": { "type": "string", "description": "Repo name as registered with add-repo" },
    "sha":  { "type": "string", "description": "Commit SHA (full or partial prefix)" }
  },
  "required": ["repo", "sha"]
}
```

The `sha` is matched exactly as stored. Use the full SHA from a `search` or `touches` result.

**Output:**

```json
{
  "repo": "api-server",
  "sha": "a3f9c12b4e77d831290ab45c6de1f8e3",
  "subject": "Add token bucket rate limiter to middleware stack",
  "body": "Fixes #482. Uses a per-IP token bucket with a 100 req/min default.\nBucket capacity and refill rate are configurable via environment variables.\n\nCo-authored-by: Jordan Lee <jordan@example.com>",
  "author": "Dana Blackwell",
  "date": 1740700997,
  "changed_files": [
    { "path": "src/middleware/mod.rs",      "status": "M", "old_path": null },
    { "path": "src/middleware/rate_limit.rs", "status": "A", "old_path": null },
    { "path": "tests/middleware_test.rs",   "status": "M", "old_path": null }
  ]
}
```

`body` is null if the commit has no body. `old_path` is non-null only for renamed files.

**Typical agent usage:**

After identifying a commit via `search`, the agent calls `get_commit` to see the full commit message and file list before deciding whether to fetch the patch. This avoids pulling a large diff for a commit that turns out to be irrelevant.

---

### `commitmux_get_patch`

Retrieve the raw unified diff for a commit. Patches are stored zstd-compressed; the server decompresses before returning.

**Input schema:**

```json
{
  "type": "object",
  "properties": {
    "repo":      { "type": "string",  "description": "Repo name" },
    "sha":       { "type": "string",  "description": "Commit SHA" },
    "max_bytes": { "type": "integer", "description": "Truncate patch text to this many bytes" }
  },
  "required": ["repo", "sha"]
}
```

Without `max_bytes`, the full decompressed patch is returned. Patches can be large. Use `max_bytes` when you only need the beginning of the diff, or when context window budget is a concern.

Truncation happens at a UTF-8 character boundary. The response is not terminated with a sentinel — it simply ends mid-diff if truncated.

**Output:**

```json
{
  "repo": "api-server",
  "sha": "a3f9c12b4e77d831290ab45c6de1f8e3",
  "patch_text": "diff --git a/src/middleware/mod.rs b/src/middleware/mod.rs\nindex 4a1b2c3..9f8e7d6 100644\n--- a/src/middleware/mod.rs\n+++ b/src/middleware/mod.rs\n@@ -1,5 +1,6 @@\n pub mod logging;\n pub mod auth;\n+pub mod rate_limit;\n\ndiff --git a/src/middleware/rate_limit.rs ..."
}
```

If no patch was stored for the commit (binary-only diff, or patch exceeded the 1 MB ingest cap), the tool returns an error: `isError: true` with message `Patch <repo>:<sha> not found`.

**Typical agent usage:**

The agent calls `get_patch` on a commit identified by `search` or `touches` to read the actual implementation and use it as reference when writing new code.

---

## Host configuration

### Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json`:

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

If `commitmux` is not on the PATH visible to Claude Desktop (common on macOS where GUI apps inherit a restricted PATH):

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

To use a specific database:

```json
{
  "mcpServers": {
    "commitmux": {
      "command": "/Users/you/.cargo/bin/commitmux",
      "args": ["serve", "--db", "/Users/you/.commitmux/db.sqlite3"]
    }
  }
}
```

The database path can also be set via the `COMMITMUX_DB` environment variable. Claude Desktop passes environment variables from its own environment to spawned subprocesses.

### Cursor

Add to `.cursor/mcp.json` in your project root, or to the global Cursor MCP config:

```json
{
  "mcpServers": {
    "commitmux": {
      "command": "commitmux",
      "args": ["serve"],
      "transport": "stdio"
    }
  }
}
```

### Zed

Add to Zed's `settings.json` under the `context_servers` key:

```json
{
  "context_servers": {
    "commitmux": {
      "command": {
        "path": "commitmux",
        "args": ["serve"]
      }
    }
  }
}
```

### Other hosts

Any MCP host with stdio transport support can run commitmux. The minimum required:

- Spawn `commitmux serve` (or equivalent with `--db` flag).
- Connect host's stdin/stdout to the process stdin/stdout.
- Send JSON-RPC 2.0 messages, one per line.
- Handle `initialize` → `tools/list` → `tools/call` sequence.

## Security model

**Read-only by design.** The MCP server exposes no tools that write, modify, or delete anything. The SQLite connection is opened in the same mode as all other commitmux commands — there is no explicit read-only flag — but the server code contains no write paths. The store trait methods exposed to the server are exclusively query methods.

**No credentials required or stored.** commitmux reads from your local git clones using libgit2. It does not authenticate to any remote. No tokens, passwords, or SSH keys are stored in the database or required at runtime.

**No network access.** `commitmux serve` makes no outbound connections. All data comes from the local SQLite database. The ingest step (`commitmux sync`) also uses libgit2 against local repos only — it does not fetch from remotes.

**Bounded surface.** The agent can only call the four defined tools. It cannot run shell commands, access arbitrary files, or query repos that have not been registered with `add-repo`. The tool surface is fixed at compile time.

**Ignore rules limit what enters the index.** Default ignored path prefixes: `node_modules/`, `vendor/`, `dist/`, `.git/`. Additional prefixes can be added at `add-repo` time with `--exclude`. This keeps lock files, generated code, and dependency trees out of the index. Sensitive files that follow a naming convention can be excluded by prefix.

**Local process.** The MCP server process runs under your user account with your file permissions. It is not a daemon and does not bind to any network port. It exits when the agent host closes its stdin.

## Freshness and staleness

The index reflects the state of your repos at the time of the last `commitmux sync`. New commits pushed or merged after that point are not visible to the agent until you sync again.

**This is intentional.** An out-of-date index is better than live access that requires credentials. The risk is that the agent makes recommendations based on stale data.

Mitigations:

- Run `commitmux sync` in a cron job or as a pre-session hook:
  ```sh
  # Example: sync every hour via cron
  0 * * * * commitmux sync >> ~/.commitmux/sync.log 2>&1
  ```

- Run `commitmux status` to check when each repo was last synced before starting an agent session.

- Use the `since` parameter in search and touches queries to limit results to a time window you trust is accurate. For example, `since: <timestamp of last sync>` avoids returning very recent commits that might not be indexed.

**The `commitmux_get_patch` tool returns `not found`** for commits that existed at ingest time but whose patch was skipped (binary diff or oversize). This is a known limitation, not a freshness issue. The commit metadata and file list are still present via `commitmux_get_commit`.

**Re-syncing is idempotent.** Upserts are keyed on `(repo, sha)`. Running `sync` on a repo that has already been synced processes only new commits and updates the ingest state. It does not re-index unchanged commits.

## Database schema reference

The SQLite database uses WAL journal mode. Tables:

```sql
repos (
    repo_id       INTEGER PRIMARY KEY,
    name          TEXT UNIQUE,
    local_path    TEXT,
    remote_url    TEXT,        -- nullable
    default_branch TEXT        -- nullable
)

commits (
    repo_id       INTEGER,
    sha           TEXT,
    author_name   TEXT,
    author_email  TEXT,
    committer_name  TEXT,
    committer_email TEXT,
    author_time   INTEGER,     -- Unix timestamp
    commit_time   INTEGER,     -- Unix timestamp
    subject       TEXT,
    body          TEXT,        -- nullable
    parent_count  INTEGER,
    patch_preview TEXT,        -- first 500 chars of diff
    PRIMARY KEY (repo_id, sha)
)

commit_files (
    repo_id  INTEGER,
    sha      TEXT,
    path     TEXT,
    status   TEXT,             -- A/M/D/R/C
    old_path TEXT              -- nullable, set for renames
)

commit_patches (
    repo_id    INTEGER,
    sha        TEXT,
    patch_blob BLOB,           -- zstd-compressed unified diff
    PRIMARY KEY (repo_id, sha)
)

ingest_state (
    repo_id         INTEGER PRIMARY KEY,
    last_synced_at  INTEGER,   -- Unix timestamp
    last_synced_sha TEXT,
    last_error      TEXT       -- nullable
)

-- FTS5 virtual table over commits
commits_fts USING fts5(subject, body, patch_preview, content='commits', content_rowid='rowid')
```

The FTS5 table is a content table backed by `commits`. It is kept in sync by the ingest write path, which manually manages FTS insertions and deletions on upsert.
