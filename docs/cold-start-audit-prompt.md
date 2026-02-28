# Cold-Start UX Audit Prompt

**Metadata:**
- Audit Date: 2026-02-28 (Round 5 — verify R4 fixes, regression check)
- Tool Version: commitmux 0.1.0 (post-Wave 1: commits bed85dd, d6c6bb6, 9a9c0fc, dd00fe8)
- Sandbox mode: local
- Sandbox: COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3
- Environment: host (macOS, Apple Silicon)

---

You are performing a UX audit of `commitmux` — a tool that builds a cross-repo git history index for AI agents, with keyword search (FTS5) and semantic vector search (embeddings via Ollama).

You are acting as a **new user** encountering this tool for the first time, specifically testing the semantic search feature after R4 fixes.

**Ollama is running** on this machine with `nomic-embed-text:latest` available. This is Round 5, verifying that R4 fixes work correctly:

**R4 fixes to verify:**
1. **R4-01 (CRITICAL):** Semantic search SQL bug fixed—should return results now (commits d6c6bb6, aef795f)
2. **R4-02:** Help text improvements—setup guidance added (commit dd00fe8)
3. **R4-03:** Status display—shows `✓` (complete), `⋯` (pending), `-` (disabled) (commit bed85dd)
4. **R4-04, R4-05:** Input validation—rejects empty query, limit=0, nonexistent repos (commit 9a9c0fc)
5. **Regression check:** Ensure R3 fixes still work, no new issues introduced

State is isolated to a temp directory:
`COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3`

The tool's real production data is unaffected.

**IMPORTANT:** `commitmux` is installed at `~/.cargo/bin/commitmux`. Run ALL commitmux commands as:
```
env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux <subcommand>
```

Shorthand used in this prompt: `CM` = `env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux`

---

## Audit Areas

### Area 1: Ollama Readiness

Before touching commitmux, verify the embedding backend:

```bash
ollama list
curl -s http://localhost:11434/v1/models
```

Note: which models are available, whether `nomic-embed-text` is listed, and whether the API responds.

---

### Area 2: Discovery — Embedding-Relevant Help Text

Check the help text a new user would read before enabling embeddings:

```bash
CM --help
CM config --help
CM config set --help
CM config get --help
CM add-repo --help
CM sync --help
```

Note: Is it clear how to enable semantic search? Does help text explain the Ollama dependency? Would a new user know to run `config set embed.model` before `add-repo --embed`?

---

### Area 3: Setup — Init and Embedding Configuration

```bash
CM init
CM config get embed.model
CM config get embed.endpoint
CM config set embed.model nomic-embed-text
CM config set embed.endpoint http://localhost:11434/v1
CM config get embed.model
CM config get embed.endpoint
```

Note the output at each step. Does `(not set)` clearly communicate what needs to be done?

---

### Area 4: Add Repos and Generate Embeddings

Add two repos — one with embeddings enabled, one without:

```bash
CM add-repo /Users/dayna.blackwell/code/commitmux --embed
CM add-repo /Users/dayna.blackwell/code/bubbletea-components
CM status
```

Then sync both:

```bash
CM sync
```

Carefully observe the sync output:
- Does it show embedding progress separately from commit indexing?
- Does it report how many commits were embedded?
- Is there any indication when embedding is slow?

Check status after sync:

```bash
CM status
```

Note: Does the EMBED column correctly show `✓` for commitmux and `-` for bubbletea-components? Is the footer clear?

---

### Area 5: Verify Embeddings Were Generated

```bash
CM show commitmux $(CM sync --repo commitmux 2>&1 | head -1 || echo "")
```

Actually, get a specific SHA to look up:

```bash
CM sync --repo commitmux
```

Then pick a known recent SHA and verify it exists:

```bash
CM show commitmux 9768e91
```

Also run a keyword search to confirm indexing worked:

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search","arguments":{"query":"embedding","limit":3}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

Note: Does `commitmux serve` print a startup message to stderr? Does it correctly shut down when stdin closes?

---

### Area 6: commitmux_search_semantic — Core Feature

This is the main focus of R4. Test semantic search with several queries via MCP JSON-RPC:

**Query 1: Conceptual match (should find embedding/vector commits)**
```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"adding vector search and natural language queries","limit":5}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

**Query 2: Infrastructure/setup (should find init, schema, database commits)**
```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"database schema initialization and setup","limit":5}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

**Query 3: Bug fix / error handling (should find fix commits)**
```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"fixing error handling and improving user feedback messages","limit":5}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

**Query 4: Very low relevance (should return empty or low-score results)**
```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"kubernetes cluster autoscaling and pod scheduling","limit":5}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

**Query 5: With repo filter**
```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"animation and UI components","repos":["bubbletea-components"],"limit":5}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

Note: The `repos` filter restricts to `bubbletea-components`, which has **no embeddings**. Observe what happens — does the tool return an empty result set, an error, or silently ignore the filter and search all repos?

**Query 6: Limit=1**
```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"git repository indexing","limit":1}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

For each query, evaluate:
- Are the top results semantically relevant to the query?
- Are `score` values present in the output? Are they in a sensible range (0–1)?
- Is the result format consistent with `commitmux_search` output?
- Is the response time reasonable (subjectively)?

---

### Area 7: tools/list — Verify commitmux_search_semantic Is Exposed

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

Note: Is `commitmux_search_semantic` listed? Is its description accurate and helpful? Does it explain that it requires embeddings to be enabled per-repo? Does it mention Ollama as the dependency?

---

### Area 8: Semantic Search on Repo Without Embeddings

Enable embeddings on bubbletea-components without syncing embeddings, then search it:

```bash
CM update-repo bubbletea-components --embed
CM status
```

Now search — note that bubbletea-components has no embeddings yet (sync was done before --embed was set):

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"bubble tea components and UI","repos":["bubbletea-components"],"limit":5}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

Note: What does the user see when a repo has embed_enabled=true but no embeddings have been generated yet? Empty results? An error? A hint to run `sync --embed-only`?

Then backfill embeddings and search again:

```bash
CM sync --embed-only --repo bubbletea-components
CM status
```

Search again after backfill — do results now appear?

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"bubble tea UI components","limit":5}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

---

### Area 9: commitmux_search_semantic — Missing Required Argument

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

Note: Is the error message clear when `query` is missing? Does it include `isError: true` in the JSON-RPC response?

---

### Area 10: Embed-Only Sync — Output and Progress

Add a third repo without --embed to test the embed-only flow:

```bash
CM add-repo /Users/dayna.blackwell/code/scout-and-wave
CM sync --repo scout-and-wave
CM update-repo scout-and-wave --embed
CM sync --embed-only --repo scout-and-wave
```

Carefully observe:
- Does the embed-only sync output differ from regular sync output?
- Is there a progress indicator showing how many commits are being embedded?
- Is there a final count ("Embedded X commits, Y failed")?
- Does the status update after embed-only sync completes?

```bash
CM status
```

---

### Area 11: Output Format Review

For each `commitmux_search_semantic` result, evaluate the output structure:

- Are `repo`, `sha`, `subject`, `author`, `date`, `score` all present?
- Is `score` a float between 0 and 1?
- Is `patch_excerpt` included? If so, is it truncated appropriately?
- Is the JSON well-formed and parseable?
- Compare the output schema to `commitmux_search` — are they consistent?

---

### Area 12: Edge Cases

```bash
# Search with empty string query
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":""}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve

# Search with limit=0
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"test","limit":0}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve

# Search with nonexistent repo filter
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"test","repos":["nonexistent-repo"]}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve

# Search with since timestamp (far future — should return empty)
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1.0"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"commitmux_search_semantic","arguments":{"query":"embedding","since":9999999999}}}\n' | env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.ZX12lOOrXD/db.sqlite3 ~/.cargo/bin/commitmux serve
```

---

Run ALL areas. Do not skip any.
Note exact output, errors, exit codes, and behavior at each step.

## Findings Format

For each issue found, use:

### [AREA] Finding Title
- **Severity**: UX-critical / UX-improvement / UX-polish
- **What happens**: What the user actually sees
- **Expected**: What better behavior looks like
- **Repro**: Exact command(s)

Severity guide:
- **UX-critical**: Broken, misleading, or completely missing behavior that blocks the user
- **UX-improvement**: Confusing or unhelpful behavior that a user would notice and dislike
- **UX-polish**: Minor friction, inconsistency, or missed opportunity for clarity

Also note **positive observations** — things that worked well or exceeded expectations.

## Report

- Group findings by area
- Include a summary table at the top: total count by severity
- Note R3 regression check: verify R3 findings are still fixed (PATH note in help, config set validation, serve startup message, --embed/--no-embed conflict error)
- Write the complete report to `docs/cold-start-audit.md` using the Write tool
