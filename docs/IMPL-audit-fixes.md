# IMPL: R3 Audit Fixes

Source: `docs/cold-start-audit.md` (Round 3)
Findings: 10 total (1 UX-critical, 4 UX-improvement, 5 UX-polish) + R2-05 carry-over

---

## Suitability Assessment

**Verdict: SUITABLE WITH CAVEATS**

All 11 findings (10 R3 + R2-05) decompose cleanly across three disjoint files:
`README.md`, `crates/embed/src/lib.rs`, and `src/main.rs`. There are no
investigation-first items — every finding has a clear root cause and a
prescribed fix. No cross-agent interfaces need to be defined: the `embed_pending`
signature is unchanged; only its internal behavior changes (fail-fast on
connection error), and the existing `Err(e)` handler in `main.rs` picks up the
improved message automatically. Parallelization value is low (Agent C dominates
since 9 of 11 fixes land in `src/main.rs`), but the decomposition is clean and
the IMPL doc provides useful tracking for 11 scattered small changes.

```
Estimated times:
- Scout phase: ~10 min (done)
- Agent execution: ~45 min (3 parallel agents; Agent C dominates at ~35 min)
- Merge & verification: ~10 min
Total SAW time: ~65 min

Sequential baseline: ~80 min (A + B + C sequentially)
Time savings: ~15 min (19% faster)

Recommendation: Marginal speed gain, but clean parallel tracking.
```

Pre-implementation scan results:
- Total items: 11 findings (10 R3 + R2-05)
- Already implemented: 0 items
- Partially implemented: 0 items
- To-do: 11 items

All agents proceed as planned.

---

## Known Issues

None identified. Build and all 66 tests pass on `main` as of this writing.

---

## Dependency Graph

```
README.md          ──→ Agent A (leaf, no deps)
embed/src/lib.rs   ──→ Agent B (leaf, no deps)
src/main.rs        ──→ Agent C (leaf, no deps from other agents)
```

All three files are leaves with no cross-dependencies introduced by this work.
Agent B's behavior change in `embed_pending` propagates to Agent C's sync
handler automatically via the unchanged `Result<EmbedSummary, anyhow::Error>`
return type — no interface change, no coordination needed.

---

## Interface Contracts

`embed_pending` signature is **unchanged**:

```rust
pub async fn embed_pending(
    store: &dyn Store,
    embedder: &Embedder,
    repo_id: i64,
    batch_size: usize,
) -> anyhow::Result<EmbedSummary>
```

**Behavioral change (Agent B only):** On the first embedding attempt that fails
with a connection error (reqwest `is_connect()` or the error chain contains
"error sending request"), return `Err(...)` immediately instead of accumulating
in `summary.failed`. The error message must be actionable:
`"Cannot connect to Ollama at {endpoint} — is Ollama running? Try: ollama serve"`

Agent C does NOT need to change the `embed_pending` error handlers in `main.rs`.
The existing `Err(e) => eprintln!("  Warning: embedding failed for '{}': {e}", r.name)`
will display the improved message from Agent B once (not N times).

---

## File Ownership

| File | Agent | Wave | Changes |
|------|-------|------|---------|
| `README.md` | A | 1 | R3-01: PATH installation guidance |
| `crates/embed/src/lib.rs` | B | 1 | R3-08 fail-fast, R3-09 error message |
| `src/main.rs` | C | 1 | R3-02, R3-03, R3-04, R3-05, R3-06, R3-07, R3-10, R3-11, R2-05, show-error-prefix |

---

## Wave Structure

```
Wave 1: [A] [B] [C]    ← 3 parallel agents, fully independent
```

No Wave 0 needed. No downstream waves. Single wave, merge, done.

---

## Agent Prompts

### Wave 1 Agent A: README PATH installation guidance

You are Wave 1 Agent A. Add a PATH setup note to README.md so new users know
to add `~/.cargo/bin` to their shell PATH after `cargo install`.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-A 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-A"
if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"; echo "Expected: $EXPECTED_DIR"; echo "Actual: $ACTUAL_DIR"; exit 1
fi
ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-A" ]; then
  echo "ISOLATION FAILURE: Wrong branch"; echo "Expected: wave1-agent-A"; echo "Actual: $ACTUAL_BRANCH"; exit 1
fi
git worktree list | grep -q "wave1-agent-A" || { echo "ISOLATION FAILURE: Worktree not in list"; exit 1; }
echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

You own this file:
- `README.md` — modify

Do not touch any other files.

## 2. Interfaces You Must Implement

None — this is a documentation-only change.

## 3. Interfaces You May Call

None.

## 4. What to Implement

**Finding R3-01**: `~/.cargo/bin` is not on PATH by default in non-interactive
shells. Users who install via `cargo install` may get `command not found` when
running `commitmux`. The README has no mention of this.

Read the existing `README.md` first. Find the installation section (likely
mentions `cargo install`). Immediately after the `cargo install` line, add a
note like:

```
After installing, ensure `~/.cargo/bin` is on your PATH:

```sh
# Add to your shell profile (~/.zshrc, ~/.bashrc, etc.)
source "$HOME/.cargo/env"
```

Or add `export PATH="$HOME/.cargo/bin:$PATH"` to your shell profile directly.
```

Match the existing style of the README (headers, code blocks, tone). Do not
restructure any other section. Add only what is needed to solve R3-01.

## 5. Tests to Write

None required for a documentation change.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-A
# Just verify the README renders correctly and contains the new text:
grep -q "cargo/env\|cargo/bin" README.md && echo "PATH note found" || echo "MISSING"
```

No build/test cycle needed for a documentation-only change.

## 7. Constraints

- Match existing README style. Do not add new sections unless necessary.
- Do not modify any `.rs` files.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-A
git add README.md
git commit -m "wave1-agent-A: add PATH setup note for cargo install"
```

Append to `/Users/dayna.blackwell/code/commitmux/docs/IMPL-audit-fixes.md`:

```yaml
### Agent A — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-A
commit: {sha}
files_changed:
  - README.md
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added: []
verification: PASS
```

---

### Wave 1 Agent B: embed_pending fail-fast and actionable error messages

You are Wave 1 Agent B. Fix the Ollama offline UX: when Ollama is not running,
`embed_pending` should fail fast after the first connection error instead of
emitting N identical error lines.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-B 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-B"
if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"; echo "Expected: $EXPECTED_DIR"; echo "Actual: $ACTUAL_DIR"; exit 1
fi
ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-B" ]; then
  echo "ISOLATION FAILURE: Wrong branch"; echo "Expected: wave1-agent-B"; echo "Actual: $ACTUAL_BRANCH"; exit 1
fi
git worktree list | grep -q "wave1-agent-B" || { echo "ISOLATION FAILURE: Worktree not in list"; exit 1; }
echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

You own:
- `crates/embed/src/lib.rs` — modify

Do not touch any other files.

## 2. Interfaces You Must Implement

`embed_pending` signature is UNCHANGED:

```rust
pub async fn embed_pending(
    store: &dyn Store,
    embedder: &Embedder,
    repo_id: i64,
    batch_size: usize,
) -> anyhow::Result<EmbedSummary>
```

**Behavioral change:** On the first embedding API call that fails with a
connection error, return `Err(...)` immediately with an actionable message.
Do NOT continue processing remaining commits.

## 3. Interfaces You May Call

```rust
// Existing — detect connection errors on reqwest errors
// The async_openai error chain wraps reqwest errors.
// Check the error string for "error sending request" or "connection refused"
// as a reliable heuristic (reqwest's is_connect() may not be accessible
// through the anyhow chain — use string matching as fallback).
```

## 4. What to Implement

Read `crates/embed/src/lib.rs` first.

**R3-08: Fail-fast on connection error**

In the `embed_pending` loop, change the `Err(e)` arm of `embedder.embed(&doc).await`:

Currently:
```rust
Err(e) => {
    eprintln!("embed: failed to embed {}: {e}", commit.sha);
    summary.failed += 1;
}
```

Change to: detect if `e` is a connection error. A reliable check: convert the
error to string and check if it contains `"error sending request"` (reqwest's
connection failure text). If it is a connection error, return `Err` immediately
with an actionable message. If it is NOT a connection error (e.g. model not
found, bad response), continue accumulating in `summary.failed` as before.

The returned error message must be:
```
"Cannot connect to Ollama at {endpoint} — is Ollama running? Try: ollama serve"
```

To get the endpoint, the `Embedder` has a `model` field. You'll need to add an
`endpoint` field (or derive it from the config) so the error message can
include it. Read the `Embedder::new` constructor — `config.endpoint` is
available there. Add `pub endpoint: String` to `Embedder` alongside `model`.

After the fail-fast return, callers in `main.rs` already handle
`Err(e) => eprintln!("  Warning: embedding failed for '{}': {e}", r.name)` —
so the actionable message will print exactly once. No changes to `main.rs`
are needed.

**R3-09: Consistent error for non-connection embed failures**

For non-connection errors (model not found, bad response, etc.) that continue
to accumulate in `summary.failed`, the existing `eprintln!` is fine. Keep it.

## 5. Tests to Write

Add these tests to the existing `#[cfg(test)]` block in `crates/embed/src/lib.rs`:

1. `test_connection_error_detection` — verify that a simulated "error sending
   request" error string is detected as a connection error by your detection
   logic. Since we can't easily inject a real reqwest error in a unit test,
   test the detection logic itself: given an `anyhow::Error` whose string
   representation contains "error sending request", confirm it would trigger
   fail-fast. This can be a pure logic test.

2. `test_embedder_has_endpoint_field` — verify the `Embedder` struct has a
   public `endpoint` field and it is set correctly from `EmbedConfig`.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-B
cargo build -p commitmux-embed
cargo clippy -p commitmux-embed -- -D warnings
cargo test -p commitmux-embed
```

All must pass. The NullStore mock in `lib.rs` already implements all required
Store trait methods — the tests should compile cleanly.

## 7. Constraints

- `embed_pending` signature must remain identical. No changes to return type.
- Do not change behavior for non-connection errors (bad model name, bad response,
  etc.) — only connection errors trigger fail-fast.
- The `Embedder` struct gains a `pub endpoint: String` field. This is additive;
  any callers constructing `Embedder` go through `Embedder::new(config)` which
  already has `config.endpoint`, so no downstream changes are needed.
- Do not modify any file outside `crates/embed/src/lib.rs`.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-B
git add crates/embed/src/lib.rs
git commit -m "wave1-agent-B: embed_pending fail-fast on Ollama connection error"
```

Append to `/Users/dayna.blackwell/code/commitmux/docs/IMPL-audit-fixes.md`:

```yaml
### Agent B — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-B
commit: {sha}
files_changed:
  - crates/embed/src/lib.rs
files_created: []
interface_deviations:
  - "describe any deviation from the Embedder.endpoint addition or fail-fast behavior, or []"
out_of_scope_deps: []
tests_added:
  - test_connection_error_detection
  - test_embedder_has_endpoint_field
verification: PASS | FAIL ({command} — N/N tests)
```

---

### Wave 1 Agent C: main.rs UX fixes (9 findings)

You are Wave 1 Agent C. Fix 9 UX findings in `src/main.rs`: help text
improvements, config key validation, empty-value rejection, conflicting-flag
detection, status legend, show error prefix, and serve startup message.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-C 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-C"
if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"; echo "Expected: $EXPECTED_DIR"; echo "Actual: $ACTUAL_DIR"; exit 1
fi
ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-C" ]; then
  echo "ISOLATION FAILURE: Wrong branch"; echo "Expected: wave1-agent-C"; echo "Actual: $ACTUAL_BRANCH"; exit 1
fi
git worktree list | grep -q "wave1-agent-C" || { echo "ISOLATION FAILURE: Worktree not in list"; exit 1; }
echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

You own:
- `src/main.rs` — modify

Do not touch any other files.

## 2. Interfaces You Must Implement

No new public interfaces. All changes are internal to CLI handling.

## 3. Interfaces You May Call

All existing Store trait methods, embed crate functions — unchanged. Do not
call any new functions from `crates/embed/src/lib.rs`; Agent B's behavioral
changes to `embed_pending` propagate automatically.

## 4. What to Implement

Read `src/main.rs` first (799 lines). Apply ALL of the following changes:

---

**R3-02: Add key examples to `config get --help`**

Find the `ConfigAction::Get` variant arg definition:
```rust
#[arg(help = "Configuration key")]
key: String,
```
Change to:
```rust
#[arg(help = "Configuration key (e.g. embed.model, embed.endpoint)")]
key: String,
```

---

**R3-03: `--embed` flag on `add-repo` needs Ollama prerequisite hint**

Find in the `AddRepo` struct:
```rust
#[arg(long = "embed", help = "Enable semantic embeddings for this repo")]
embed: bool,
```
Change help text to:
```rust
#[arg(long = "embed", help = "Enable semantic embeddings for this repo (requires: commitmux config set embed.model <model>)")]
embed: bool,
```

---

**R3-04: `--embed-only` on `sync` needs context**

Find in the `Sync` struct:
```rust
#[arg(long = "embed-only", help = "Only generate embeddings; skip commit indexing")]
embed_only: bool,
```
Change to:
```rust
#[arg(long = "embed-only", help = "Generate embeddings for already-indexed commits; skip indexing new commits. Useful when embedding was enabled after initial sync.")]
embed_only: bool,
```

---

**R3-05: `config set` must reject unknown keys**

Find the `Commands::Config` handler, specifically the `ConfigAction::Set { key, value }` arm.
Currently it calls `store.set_config(&key, &value)` directly.

Before the `set_config` call, add a validation check:

```rust
const VALID_CONFIG_KEYS: &[&str] = &["embed.model", "embed.endpoint"];
if !VALID_CONFIG_KEYS.contains(&key.as_str()) {
    anyhow::bail!(
        "Unknown config key '{}'. Valid keys: {}",
        key,
        VALID_CONFIG_KEYS.join(", ")
    );
}
```

---

**R3-06: `config set` must reject empty string values**

In the same `ConfigAction::Set` handler, after the key validation, add:

```rust
if value.trim().is_empty() {
    anyhow::bail!("Value for '{}' cannot be empty", key);
}
```

---

**R3-07: `--embed` and `--no-embed` must be mutually exclusive (clap conflicts_with)**

Find the `UpdateRepo` struct. The `--embed` arg currently has no `conflicts_with`.
Change:
```rust
#[arg(long = "embed", help = "Enable semantic embeddings for this repo")]
embed: bool,
#[arg(long = "no-embed", help = "Disable semantic embeddings for this repo")]
no_embed: bool,
```
To:
```rust
#[arg(long = "embed", conflicts_with = "no_embed", help = "Enable semantic embeddings for this repo")]
embed: bool,
#[arg(long = "no-embed", conflicts_with = "embed", help = "Disable semantic embeddings for this repo")]
no_embed: bool,
```
Clap will produce a clear error automatically when both are provided.
The `if embed { Some(true) } else if no_embed { Some(false) } else { None }` logic
downstream is still correct and can remain unchanged.

---

**R3-10: EMBED column legend in status footer**

Find the status footer where `Embedding model:` is printed (around line 594-599):
```rust
println!("Embedding model: {} ({})", model, endpoint);
```
Change to append a legend:
```rust
println!("Embedding model: {} ({}) — ✓ = enabled", model, endpoint);
```
This removes ambiguity about the `-` column values without requiring a new line.

---

**R3-11: `show` command missing `Error:` prefix**

Find around line 515:
```rust
eprintln!("Commit '{}' not found in repo '{}'", sha, repo);
```
Change to:
```rust
eprintln!("Error: Commit '{}' not found in repo '{}'", sha, repo);
```

---

**R2-05: `serve` should print startup confirmation to stderr**

Find the `Commands::Serve` handler (around line 611):
```rust
commitmux_mcp::run_mcp_server(store).context("MCP server error")?;
```
Add a startup message before it:
```rust
eprintln!("commitmux MCP server ready (JSON-RPC over stdio). Press Ctrl+C to stop.");
commitmux_mcp::run_mcp_server(store).context("MCP server error")?;
```
This goes to stderr so it does not interfere with the JSON-RPC protocol on stdout.

---

## 5. Tests to Write

Add/update these tests in `src/main.rs` (in the existing `#[cfg(test)]` block):

1. `test_config_set_rejects_unknown_key` — call the validation logic directly
   (or test via integration test pattern). At minimum, confirm that
   `VALID_CONFIG_KEYS` contains exactly `["embed.model", "embed.endpoint"]`.

2. `test_config_set_rejects_empty_value` — verify the empty string check rejects
   `""` and `"   "` (whitespace-only).

Note: Many existing tests use the in-process test helpers. Follow the existing
test patterns at the bottom of `main.rs`.

## 6. Verification Gate

**Before running:** Check for any tests that validate the old behavior of
`--embed --no-embed` conflict (there should be none, but verify):
```bash
grep -n "no.embed\|embed.*no" src/main.rs | grep -i test
```

Then run:
```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-C
cargo build
cargo clippy -- -D warnings
cargo test
```

All 66+ tests must pass. Pay attention to:
- `test_config_set_get_roundtrip` — still passes (valid key, non-empty value)
- `test_embed_sync_tip_logic` — unaffected, still passes
- `test_tip_shows_on_resync` — unaffected, still passes

## 7. Constraints

- Do not change any Store trait methods or crate public APIs.
- Do not modify the sync handler's embed error handling — Agent B's changes
  to `embed_pending` propagate automatically via the existing `Err(e)` handler.
- `VALID_CONFIG_KEYS` should be a constant near the top of the config handler
  section, not inlined in the error message.
- The `conflicts_with` field in clap uses the field name (snake_case), not the
  CLI flag name. `no_embed` is the field name for `--no-embed`.
- All help text changes should be minimal and exact — no reformatting of
  surrounding args.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-C
git add src/main.rs
git commit -m "wave1-agent-C: UX fixes R3-02 through R3-11 and R2-05"
```

Append to `/Users/dayna.blackwell/code/commitmux/docs/IMPL-audit-fixes.md`:

```yaml
### Agent C — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-C
commit: {sha}
files_changed:
  - src/main.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_config_set_rejects_unknown_key
  - test_config_set_rejects_empty_value
verification: PASS | FAIL ({command} — N/N tests)
```

---

## Wave Execution Loop

After Wave 1 completes:
1. Read each agent's completion report above.
2. Check for `interface_deviations` (especially from Agent B — if `Embedder.endpoint`
   field name changed, update Agent C's sync handler if it references it).
3. Merge: `git merge --no-ff wave1-agent-A`, then `wave1-agent-B`, then `wave1-agent-C`.
4. Run post-merge verification (unscoped): `cargo test --workspace`
5. Fix any cascade issues (most likely: none, since ownership is fully disjoint).
6. Commit, push.

---

## Status

- [x] Wave 1 Agent A — README PATH installation guidance (R3-01)
- [x] Wave 1 Agent B — embed_pending fail-fast + error messages (R3-08, R3-09)
- [x] Wave 1 Agent C — main.rs UX fixes: R3-02, R3-03, R3-04, R3-05, R3-06, R3-07, R3-10, R3-11, R2-05

---

### Agent A — Completion Report
status: complete
worktree: .claude/worktrees/wave1-agent-A
commit: e6d2ca9
files_changed:
  - README.md
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added: []
verification: PASS

### Agent B — Completion Report
status: complete
worktree: .claude/worktrees/wave1-agent-B
commit: 7a1097f25903859f8edeccf55123cd0fc493ffb0
files_changed:
  - crates/embed/src/lib.rs
files_created: []
interface_deviations:
  - []
out_of_scope_deps: []
tests_added:
  - test_connection_error_detection
  - test_embedder_has_endpoint_field
verification: PASS (cargo test -p commitmux-embed — 6/6 tests)

### Agent C — Completion Report
status: complete
worktree: .claude/worktrees/wave1-agent-C
commit: 6ef28ca
files_changed:
  - src/main.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_config_set_rejects_unknown_key
  - test_config_set_rejects_empty_value
verification: PASS (cargo test — 15/15 tests)
