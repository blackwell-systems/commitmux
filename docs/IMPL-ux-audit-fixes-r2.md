# IMPL: UX Audit Fixes — Round 2

**Feature:** Fix Round 2 UX audit findings from `docs/cold-start-audit-r2.md`
**Date:** 2026-02-28
**Scout version:** scout v0.2.1 / agent-template v0.3.2

---

## Suitability Assessment

Verdict: **SUITABLE WITH CAVEATS**

Six of the seven findings all modify `src/main.rs` (arg descriptions, error handling, sync tip logic, and init ordering guidance). Only one finding — the `serve` startup message — touches a different file (`crates/mcp/src/lib.rs`). This limits true parallel benefit to two agents with partially disjoint ownership. The parallelization value is low: changes are short, `cargo test` completes in under 15 seconds, and both agents touch simple output/string-literal changes with no cross-agent interfaces. However, the IMPL doc provides value as a structured audit trail and verification checklist. Proceeding as SUITABLE WITH CAVEATS, with the caveat that SAW overhead likely approaches or exceeds the parallel time savings — sequential execution is equally valid.

Pre-implementation scan results:
- Total items: 7 findings
- Already implemented: 0 items (0% of work)
- Partially implemented: 1 item (finding R2-03 / add-repo [PATH]) — the description exists but mutual exclusivity with `--url` is not mentioned
- To-do: 6 items

Agent adjustments:
- Agent A handles findings R2-01, R2-02, R2-03 (partial — complete the description), R2-04, R2-05, R2-07 — all in `src/main.rs`
- Agent B handles finding R2-06 — in `crates/mcp/src/lib.rs`
- No agents changed to "verify + add tests" (none pre-implemented)

Estimated times:
- Scout phase: ~10 min (dependency mapping, interface contracts, IMPL doc)
- Agent execution: ~15 min (2 agents × ~10 min avg, running in parallel; Agent A has 6 changes, Agent B has 1)
- Merge & verification: ~3 min
- Total SAW time: ~28 min

Sequential baseline: ~20 min (2 agents × 10 min avg sequential time)
Time savings: ~-8 min (SAW is slower due to overhead)

Recommendation: Marginal gains at best; overhead likely dominates. Proceed if the IMPL doc's audit-trail and verification-checklist value is desired. Sequential execution is equally valid for a batch this small.

---

## Known Issues

None identified. Full `cargo test --workspace` passes clean (25 tests across 4 crates + integration test) as of 2026-02-28.

---

## Dependency Graph

```
src/main.rs (Agent A)
  - No cross-agent dependencies.
  - Depends on: clap arg declaration macros (no code change needed),
    existing anyhow error handling, existing SqliteStore::open call.
  - All changes are isolated string literals and control-flow conditions
    within the existing match arms.

crates/mcp/src/lib.rs (Agent B)
  - No cross-agent dependencies.
  - Change: add a single eprintln! to run_stdio() before the read loop.
  - Does not touch pub fn run_mcp_server signature or any type.
```

Roots (no upstream dependencies): both Agent A and Agent B are roots — neither depends on the other's output.

Cascade candidates (files that reference interfaces whose semantics change):
- None. No type renames, no function signature changes, no new public symbols.
- The `run_mcp_server` function signature in `crates/mcp/src/lib.rs` does NOT change.
- The clap struct fields in `src/main.rs` do NOT change types or names; only `#[arg(help = ...)]` strings are added.

Type rename cascade check: No type renames are introduced in this batch. No cascade search required.

---

## Interface Contracts

No cross-agent interfaces are introduced. Both agents implement standalone string/output changes.

**Agent A** modifies only `#[arg(help = ...)]` attributes and output/error-handling logic within `fn main()`. No new public functions. No type changes.

**Agent B** adds a single `eprintln!` call inside `McpServer::run_stdio()`. The public signature of `run_mcp_server` is unchanged:

```rust
// crates/mcp/src/lib.rs — unchanged public signature
pub fn run_mcp_server(store: Arc<dyn Store + 'static>) -> anyhow::Result<()>
```

No contracts need to be written between agents because there are no cross-agent call sites.

---

## File Ownership

| File | Agent | Wave | Depends On |
|------|-------|------|------------|
| `src/main.rs` | A | 1 | — |
| `crates/mcp/src/lib.rs` | B | 1 | — |

No file is owned by more than one agent.

---

## Wave Structure

```
Wave 1: [A] [B]     <- 2 parallel agents, fully independent
         |
    (A + B complete)
         |
    post-merge verification
```

Wave 0 is omitted: there are no correctness prerequisites — neither agent's work gates the other's ability to verify.

---

## Agent Prompts

---

### Wave 1 Agent A: Fix six UX findings in src/main.rs

You are Wave 1 Agent A. Your task is to fix six UX issues in `src/main.rs`: add missing help text to positional arguments in `remove-repo`, `update-repo`, `show`, and `add-repo`; add a "db not found" hint when `init` has not been run; suppress the libgit2 error chain from the non-git-repo error; and show the MCP tip on re-sync when the repo has commits but no new ones were indexed.

#### 0. CRITICAL: Isolation Verification (RUN FIRST)

**Step 1: Attempt environment correction**

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a 2>/dev/null || true
```

**Step 2: Verify isolation (strict fail-fast after self-correction attempt)**

```bash
ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory (even after cd attempt)"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual: $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave1-agent-a"

if [ "$ACTUAL_BRANCH" != "$EXPECTED_BRANCH" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: $EXPECTED_BRANCH"
  echo "Actual: $ACTUAL_BRANCH"
  exit 1
fi

git worktree list | grep -q "$EXPECTED_BRANCH" || {
  echo "ISOLATION FAILURE: Worktree not in git worktree list"
  exit 1
}

echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

**If verification fails:** Write error to completion report and exit immediately (do NOT modify files):

```
### Agent A — Completion Report

**ISOLATION VERIFICATION FAILED**

Expected: .claude/worktrees/wave1-agent-a on branch wave1-agent-a
Actual: [paste output from pwd and git branch]

**No work performed.** Cannot proceed without confirmed isolation.
```

**If verification passes:** Document briefly in completion report, then proceed.

#### 1. File Ownership

You own these files. Do not touch any other files.

- `/Users/dayna.blackwell/code/commitmux/src/main.rs` — modify

#### 2. Interfaces You Must Implement

No new public interfaces. All changes are internal to `fn main()` and the clap struct definitions.

#### 3. Interfaces You May Call

All interfaces are existing and unchanged:

```rust
// In src/main.rs — existing error handling pattern:
SqliteStore::open(&db_path)
    .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

// git2 open — you will change how this error is propagated:
git2::Repository::open(&canonical)
    .with_context(|| format!("'{}' is not a git repository", canonical.display()))?;
```

#### 4. What to Implement

Read `/Users/dayna.blackwell/code/commitmux/src/main.rs` in full before making any changes. Read `/Users/dayna.blackwell/code/commitmux/docs/cold-start-audit-r2.md` to understand each finding.

Apply these six changes:

**Change 1: `remove-repo` `<NAME>` description (R2-01)**

In the `RemoveRepo` variant of `Commands`, add a help attribute to the `name` field:

```rust
// Before:
RemoveRepo {
    name: String,

// After:
RemoveRepo {
    #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
    name: String,
```

**Change 2: `update-repo` `<NAME>` description (R2-01)**

In the `UpdateRepo` variant of `Commands`, add a help attribute to the `name` field:

```rust
// Before:
UpdateRepo {
    name: String,

// After:
UpdateRepo {
    #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
    name: String,
```

**Change 3: `show` `<REPO>` description (R2-02)**

In the `Show` variant of `Commands`, add a help attribute to the `repo` field:

```rust
// Before:
Show {
    repo: String,

// After:
Show {
    #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
    repo: String,
```

**Change 4: `add-repo` `[PATH]` mutual exclusivity note (R2-03)**

The `path` field already has `help = "Local path to a git repository"`. Update it to mention mutual exclusivity with `--url`:

```rust
// Before:
#[arg(conflicts_with = "url", help = "Local path to a git repository")]
path: Option<PathBuf>,

// After:
#[arg(conflicts_with = "url", help = "Local path to a git repository (mutually exclusive with --url)")]
path: Option<PathBuf>,
```

**Change 5: init-first hint when DB is missing (R2-04)**

In `Commands::AddRepo`, `Commands::RemoveRepo`, `Commands::UpdateRepo`, `Commands::Sync`, `Commands::Show`, and `Commands::Status`, the pattern `SqliteStore::open(&db_path).with_context(|| ...)` currently produces a generic error when the database file does not exist. Intercept this specific case and emit a helpful hint.

The cleanest approach: after resolving `db_path`, check whether the file exists before calling `SqliteStore::open`. If it does not exist, bail with:

```
Database not found at {path}. Run 'commitmux init' first.
```

Do this by adding a helper at the top of `fn main()` scope (or inline) that replaces the bare `SqliteStore::open` call with a two-step check in each command arm that calls `open`. The `Init` command itself must NOT do this check (it creates the DB). Example pattern for each affected arm:

```rust
// Replace this pattern:
let store = SqliteStore::open(&db_path)
    .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

// With:
if !db_path.exists() {
    anyhow::bail!(
        "Database not found at {}. Run 'commitmux init' first.",
        db_path.display()
    );
}
let store = SqliteStore::open(&db_path)
    .with_context(|| format!("Failed to open database at {}", db_path.display()))?;
```

Apply this to the following arms: `AddRepo`, `RemoveRepo`, `UpdateRepo`, `Sync`, `Show`, `Status`, `Serve`. Do NOT apply it to `Init`.

**Change 6: Suppress libgit2 error chain for non-git-repo (R2-05)**

In `Commands::AddRepo`, the local path branch calls:

```rust
git2::Repository::open(&canonical)
    .with_context(|| format!("'{}' is not a git repository", canonical.display()))?;
```

The `.with_context()` wraps the git2 error, producing a `Caused by:` chain with libgit2 internals. Replace this with a pattern that discards the underlying error:

```rust
git2::Repository::open(&canonical).map_err(|_| {
    anyhow::anyhow!("'{}' is not a git repository", canonical.display())
})?;
```

This produces the same user-visible first line but suppresses the `Caused by: could not find repository at ...; class=Repository (6); code=NotFound (-3)` chain.

**Change 7: Show MCP tip on re-sync when index is non-empty (R2-07)**

In `Commands::Sync`, the tip currently reads:

```rust
if total_indexed > 0 {
    println!("Tip: run 'commitmux serve' to expose this index via MCP to AI agents.");
}
```

This suppresses the tip when a re-sync finds 0 new commits. Change the condition to show the tip whenever the overall index is non-empty after sync. Compute a `total_in_index` counter by accumulating `summary.commits_indexed + summary.commits_already_indexed` for each repo, then show the tip if `total_in_index > 0`:

```rust
let mut total_in_index = 0usize;

// Inside the Ok(summary) arm, after total_indexed:
total_in_index += summary.commits_indexed + summary.commits_already_indexed;

// After the loop:
if total_in_index > 0 {
    println!("Tip: run 'commitmux serve' to expose this index via MCP to AI agents.");
}
```

Keep `total_indexed` as-is (it is not displayed, it was only used to gate the tip). Remove `total_indexed` if it is no longer used after this change, or retain it if it improves readability. Either is acceptable; the key requirement is that the tip appears on re-sync.

#### 5. Tests to Write

Add these tests to the `#[cfg(test)]` mod in `src/main.rs`:

1. `test_db_not_found_hint_message` — construct a `PathBuf` pointing to a nonexistent file and call the db-existence check pattern; verify the error message contains `"Run 'commitmux init' first"`. (Test the logic directly, not via CLI invocation.)

2. `test_git2_error_suppressed` — call `git2::Repository::open` on a temp dir that is not a git repo, apply the `.map_err(|_| anyhow::anyhow!(...))` pattern, and verify the resulting error message does NOT contain `"class=Repository"` or `"code=NotFound"`.

3. `test_mcp_tip_on_resync` — unit-test the tip condition: when `total_in_index > 0` and `total_indexed == 0` (re-sync with no new commits), the tip would be shown. This can be a simple assertion on the boolean condition, documented as verifying the logic change from the audit.

#### 6. Verification Gate

Before running verification, check for any tests that assert old behavior for the changed output:

```bash
grep -n "Tip: run" /Users/dayna.blackwell/code/commitmux/src/main.rs
grep -rn "total_indexed" /Users/dayna.blackwell/code/commitmux/src/
grep -rn "is not a git repository" /Users/dayna.blackwell/code/commitmux/
```

Run these commands. All must pass before you report completion.

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a
cargo build
cargo clippy -- -D warnings
cargo test -p commitmux
```

The focused test target `-p commitmux` runs only the tests in `src/main.rs` (the binary crate). The integration test in `tests/integration.rs` is included in the `commitmux` package and will also run.

#### 7. Constraints

- Do NOT modify any file in `crates/`. Agent B owns `crates/mcp/src/lib.rs`; all other crate files are out of scope.
- Do NOT change any public struct/enum/function signatures in `src/main.rs` — there are none to export, but do not add new `pub` items.
- All informational output (hints, tips) goes to stdout. All error messages go to stderr (via `anyhow` bail/propagation, which writes to stderr in the `main() -> Result<()>` return path).
- The init-first hint (Change 5) must go to stderr as an error (via `anyhow::bail!`), not stdout. This is consistent with how all other "cannot proceed" errors are reported.
- The db-existence check must use `db_path.exists()` — do not use `metadata()` or `try_open`. Simple `.exists()` is sufficient and consistent with the rest of the codebase.
- Keep the git2 dependency as-is; do not add or remove crate features.

If you discover that correct implementation requires changing a file not in your ownership list, do NOT modify it. Report it in section 8 as an out-of-scope dependency.

**Build failures from out-of-scope symbols:** If the build fails because a symbol owned by Agent B does not yet exist in your isolated worktree, do NOT fix it by modifying the defining file. Note the failure in your completion report under `out_of_scope_build_blockers`.

#### 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-a
git add src/main.rs
git commit -m "wave1-agent-a: fix six R2 UX audit findings in src/main.rs"
```

Append your completion report to `/Users/dayna.blackwell/code/commitmux/docs/IMPL-ux-audit-fixes-r2.md` under `### Agent A — Completion Report`:

```yaml
### Agent A — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-a
commit: {sha}
files_changed:
  - src/main.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_db_not_found_hint_message
  - test_git2_error_suppressed
  - test_mcp_tip_on_resync
verification: PASS | FAIL ({command} — N/N tests)
```

---

### Wave 1 Agent B: Add startup message to commitmux serve

You are Wave 1 Agent B. Your task is to add a user-visible startup message to `commitmux serve` so that users running it from a terminal see confirmation that the MCP server is live.

#### 0. CRITICAL: Isolation Verification (RUN FIRST)

**Step 1: Attempt environment correction**

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b 2>/dev/null || true
```

**Step 2: Verify isolation (strict fail-fast after self-correction attempt)**

```bash
ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory (even after cd attempt)"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual: $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
EXPECTED_BRANCH="wave1-agent-b"

if [ "$ACTUAL_BRANCH" != "$EXPECTED_BRANCH" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: $EXPECTED_BRANCH"
  echo "Actual: $ACTUAL_BRANCH"
  exit 1
fi

git worktree list | grep -q "$EXPECTED_BRANCH" || {
  echo "ISOLATION FAILURE: Worktree not in git worktree list"
  exit 1
}

echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

**If verification fails:** Write error to completion report and exit immediately (do NOT modify files):

```
### Agent B — Completion Report

**ISOLATION VERIFICATION FAILED**

Expected: .claude/worktrees/wave1-agent-b on branch wave1-agent-b
Actual: [paste output from pwd and git branch]

**No work performed.** Cannot proceed without confirmed isolation.
```

**If verification passes:** Document briefly in completion report, then proceed.

#### 1. File Ownership

You own these files. Do not touch any other files.

- `/Users/dayna.blackwell/code/commitmux/crates/mcp/src/lib.rs` — modify

#### 2. Interfaces You Must Implement

No new public interfaces. The public signature of `run_mcp_server` must NOT change:

```rust
// Must remain unchanged:
pub fn run_mcp_server(store: Arc<dyn Store + 'static>) -> anyhow::Result<()>
```

#### 3. Interfaces You May Call

All interfaces are existing and unchanged. You will use:

```rust
// Already imported in crates/mcp/src/lib.rs:
use std::io::{BufRead, Write};
// eprintln! is available as a macro — no import needed
```

#### 4. What to Implement

Read `/Users/dayna.blackwell/code/commitmux/crates/mcp/src/lib.rs` in full before making any changes. Read `/Users/dayna.blackwell/code/commitmux/docs/cold-start-audit-r2.md` finding R2-06 (`[SERVE]`) to understand the requirement.

The `run_stdio` method in `McpServer` currently begins the read loop with no output:

```rust
fn run_stdio(&self) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
```

Add a startup message printed to **stderr** immediately before the read loop begins. The message must:
- Go to stderr, not stdout (stdout is the JSON-RPC transport channel; writing non-JSON to it would corrupt the MCP protocol)
- Confirm the server is live and listening
- Tell the user how to stop it

Exact message:

```
commitmux MCP server ready (JSON-RPC over stdio). Ctrl+C to stop.
```

Emit it with `eprintln!`:

```rust
fn run_stdio(&self) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    eprintln!("commitmux MCP server ready (JSON-RPC over stdio). Ctrl+C to stop.");

    for line in stdin.lock().lines() {
```

This is a one-line change. The placement immediately before the loop is deliberate: if `SqliteStore::open` fails in `Commands::Serve` (caught before `run_mcp_server` is called), the startup message is never printed, which is correct behavior.

#### 5. Tests to Write

Add one test to the `#[cfg(test)]` mod in `crates/mcp/src/lib.rs`:

1. `test_run_stdio_startup_message_goes_to_stderr` — This is a design-verification test, not an I/O capture test. Since `eprintln!` writes to stderr and stderr is not easily captured in Rust unit tests without external crates, write the test as a documentation test that asserts the contract: the startup message string is the expected literal. Example approach:

```rust
#[test]
fn test_startup_message_string() {
    // Verify the startup message matches the documented spec.
    // This is a compile-time constant check, not a runtime I/O test.
    let msg = "commitmux MCP server ready (JSON-RPC over stdio). Ctrl+C to stop.";
    assert!(msg.contains("JSON-RPC over stdio"), "startup message should mention transport");
    assert!(msg.contains("Ctrl+C"), "startup message should mention how to stop");
    assert!(!msg.is_empty());
}
```

Note: if you find a clean way to capture stderr output in the existing test infrastructure (e.g., by refactoring `run_stdio` to accept a writer parameter), that is acceptable but not required. Keep the change minimal.

#### 6. Verification Gate

Before running verification, check for any tests that assert the current (silent) server startup behavior:

```bash
grep -rn "run_mcp_server\|run_stdio\|startup\|server ready" /Users/dayna.blackwell/code/commitmux/crates/mcp/
grep -rn "run_mcp_server\|serve" /Users/dayna.blackwell/code/commitmux/tests/
```

Run these commands. All must pass before you report completion.

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b
cargo build
cargo clippy -- -D warnings
cargo test -p commitmux-mcp
```

#### 7. Constraints

- Do NOT modify `src/main.rs`. Agent A owns that file.
- Do NOT modify `crates/mcp/src/tools.rs` unless the test strictly requires it — prefer adding tests to `lib.rs`.
- The startup message MUST go to stderr. Writing anything non-JSON to stdout before or during the read loop would corrupt the MCP JSON-RPC protocol.
- Do not flush stderr explicitly — `eprintln!` auto-flushes.
- Do not add any new crate dependencies to `crates/mcp/Cargo.toml`.
- The public `run_mcp_server` signature must remain identical.

If you discover that correct implementation requires changing a file not in your ownership list, do NOT modify it. Report it in section 8 as an out-of-scope dependency.

#### 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-b
git add crates/mcp/src/lib.rs
git commit -m "wave1-agent-b: add startup message to commitmux serve (R2-06)"
```

Append your completion report to `/Users/dayna.blackwell/code/commitmux/docs/IMPL-ux-audit-fixes-r2.md` under `### Agent B — Completion Report`:

```yaml
### Agent B — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave1-agent-b
commit: {sha}
files_changed:
  - crates/mcp/src/lib.rs
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_startup_message_string
verification: PASS | FAIL ({command} — N/N tests)
```

---

## Wave Execution Loop

After Wave 1 completes:

1. Read completion reports from `### Agent A — Completion Report` and `### Agent B — Completion Report` in this document. Check for interface contract deviations and out-of-scope dependencies.

2. Merge both agent worktrees back into the main branch:
   ```bash
   cd /Users/dayna.blackwell/code/commitmux
   git merge wave1-agent-a
   git merge wave1-agent-b
   ```

3. Run the full verification gate against the merged result:
   ```bash
   cd /Users/dayna.blackwell/code/commitmux
   cargo build && cargo clippy -- -D warnings && cargo test --workspace
   ```

   Pay attention to cascade candidates (none identified — see Dependency Graph section). No files outside agent scope reference changed interfaces.

4. Fix any compiler errors or integration issues flagged by agents.

5. Update the Status section below: tick completed checkboxes.

6. Commit the wave's changes:
   ```bash
   git commit -m "merge wave1: fix R2 UX audit findings (A + B)"
   ```

7. No further waves — Wave 1 is the only wave.

If verification fails, fix before proceeding.

---

## Status

- [ ] Wave 1 Agent A — Fix six UX findings in `src/main.rs`: arg descriptions (R2-01, R2-02, R2-03), init-first hint (R2-04), libgit2 error suppression (R2-05), MCP tip on re-sync (R2-07)
- [ ] Wave 1 Agent B — Add startup message to `crates/mcp/src/lib.rs` (R2-06)
