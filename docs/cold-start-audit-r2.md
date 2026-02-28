# commitmux Cold-Start UX Audit — Round 2

**Date**: 2026-02-28
**Auditor**: Claude (acting as new user)
**Version**: commitmux 0.1.0
**Audit type**: Post-fix regression check + fresh cold-start pass

---

## Summary Table

| Severity | New Findings | Round 1 Regressions | Total Open |
|---|---|---|---|
| UX-critical | 0 | 0 | 0 |
| UX-improvement | 3 | 0 | 3 |
| UX-polish | 4 | 0 | 4 |
| **Total** | **7** | **0** | **7** |

---

## Round 1 Findings Resolution

| # | Finding | Status |
|---|---|---|
| R1-01 | [DISCOVERY] All subcommands missing descriptions in top-level help | **RESOLVED** |
| R1-02 | [DISCOVERY] All flags in every subcommand missing descriptions | **RESOLVED** |
| R1-03 | [DISCOVERY] `[PATH]` argument in `add-repo --help` not described | **PARTIALLY RESOLVED** — `--url`, `--fork-of`, `--author`, `--exclude` now have descriptions; `[PATH]` argument itself still has no description text |
| R1-04 | [DISCOVERY] No `--version` flag | **RESOLVED** — `commitmux --version` now outputs `commitmux 0.1.0` |
| R1-05 | [ONBOARDING] `init` idempotent but gives no indication | **RESOLVED** — second run now prints `Database already initialized at <path>` |
| R1-06 | [ONBOARDING] Empty `status` shows only header row, no hint | **RESOLVED** — now prints `No repositories indexed. Run: commitmux add-repo <path>` |
| R1-07 | [ONBOARDING] No mention of `init` being required before other commands | **OPEN** — no ordering guidance in help or output |
| R1-08 | [ADD-REPO] Adding a non-git directory succeeds silently, fails only at sync | **RESOLVED** — `add-repo /tmp` now immediately errors: `Error: '/private/tmp' is not a git repository` |
| R1-09 | [SYNC] `sync` exits 0 even when repos fail | **RESOLVED** — confirmed `any_error` flag → `std::process::exit(1)` |
| R1-10 | [SYNC] "skipped" is ambiguous — means both "already indexed" and "author-filtered" | **RESOLVED** — sync now outputs distinct counts: `N indexed, M already indexed` or `N indexed, M already indexed, K filtered by author` |
| R1-11 | [STATUS] Status does not show repo path or URL | **RESOLVED** — new `SOURCE` column added |
| R1-12 | [STATUS] Configured metadata invisible after `update-repo` | **RESOLVED** — `status` now shows `filters: author=<email>` line under affected repos |
| R1-13 | [SHOW] Commit `date` field is raw Unix timestamp | **RESOLVED** — `date` field is now ISO 8601 (e.g. `"2026-02-28T16:07:27Z"`) |
| R1-14 | [SHOW] `Commit not found` error has no context | **RESOLVED** — now prints `Commit '<sha>' not found in repo '<name>'` |
| R1-15 | [REMOVE-REPO] No confirmation, no mention of data loss | **RESOLVED** — now prints `Removed repo '<name>' (N commits deleted from index)` |
| R1-16 | [EDGE] Duplicate `add-repo` exposes raw SQLite constraint error | **RESOLVED** — now prints `Error: A repo named '<name>' already exists. Use 'commitmux status' to see all repos.` |
| R1-17 | [EDGE] `--url` with invalid string attempts clone before URL validation | **RESOLVED** — now validates URL format first: `Error: 'not-a-url' is not a valid git URL (expected https://, ...)` |
| R1-18 | [SERVE] `commitmux serve` produces no startup output | **OPEN** — server still starts silently |
| R1-19 | [SERVE] No onboarding path from CLI to MCP usage | **PARTIALLY RESOLVED** — MCP tip appears after first sync (`total_indexed > 0`); does not appear on re-sync when all commits are already indexed |
| R1-20 | [OUTPUT] `status` LAST SYNCED has no timezone label | **RESOLVED** — timestamps now show ` UTC` suffix (e.g. `2026-02-28 16:09:41 UTC`) |

**Summary**: 17 of 20 Round 1 findings are fully resolved. 2 remain open (R1-07, R1-18), 1 is partially resolved (R1-19).

---

## New Findings — Round 2

### Area 1: Discovery

#### [DISCOVERY] Positional argument `<NAME>` in `remove-repo` and `update-repo` has no description
- **Severity**: UX-polish
- **What happens**: `commitmux remove-repo --help` shows `Arguments: <NAME>` with a blank description. Same for `update-repo --help`. The user must infer that `<NAME>` refers to the repo name as registered (not a path, not a directory name).
- **Expected**: `<NAME>  Name of the indexed repository (see 'commitmux status')`
- **Repro**: `commitmux remove-repo --help`, `commitmux update-repo --help`

```
Arguments:
  <NAME>
```

---

#### [DISCOVERY] Positional argument `<REPO>` in `show` has no description
- **Severity**: UX-polish
- **What happens**: `commitmux show --help` shows `<REPO>` with a blank description. `<SHA>` has a description (`Full or prefix SHA of the commit`) but `<REPO>` does not. This inconsistency is jarring and unhelpful.
- **Expected**: `<REPO>  Name of the indexed repository (see 'commitmux status')`
- **Repro**: `commitmux show --help`

```
Arguments:
  <REPO>
  <SHA>   Full or prefix SHA of the commit
```

---

#### [DISCOVERY] `add-repo` positional `[PATH]` argument still has no description
- **Severity**: UX-polish
- **What happens**: `commitmux add-repo --help` lists all flags with clear descriptions, but the `[PATH]` argument itself still has no description text. There is also no note clarifying that `[PATH]` and `--url` are mutually exclusive alternatives. (This is a carry-over from R1-03, which was marked only partially resolved.)
- **Expected**: `[PATH]  Local path to a git repository (mutually exclusive with --url)`
- **Repro**: `commitmux add-repo --help`

```
Arguments:
  [PATH]  Local path to a git repository
```

Note: the `--url` option description reads "Remote git URL to clone and index" but there is nothing to tell the user that they use *either* `[PATH]` or `--url`, not both.

---

### Area 2: Setup / Onboarding

#### [ONBOARDING] No ordering guidance: `init` requirement is undocumented
- **Severity**: UX-improvement
- **What happens**: The top-level help and every subcommand help are silent about setup order. A new user who runs `commitmux add-repo ~/code/myrepo` before `commitmux init` will receive a cryptic database file error rather than a prompt to run `init` first.
- **Expected**: The top-level help description or a "Getting Started" note should state the required workflow: `init → add-repo → sync → serve`. At minimum, running `add-repo` against a non-existent database should print a hint: `Database not found. Run 'commitmux init' first.`
- **Repro**: `commitmux --help` (note absence of ordering guidance)

---

### Area 7: Edge Cases / Output

#### [EDGE] Non-git-repo error message exposes internal `libgit2` error chain
- **Severity**: UX-improvement
- **What happens**: `commitmux add-repo /tmp` prints:
  ```
  Error: '/private/tmp' is not a git repository

  Caused by:
      could not find repository at '/private/tmp'; class=Repository (6); code=NotFound (-3)
  ```
  The first line is clear and actionable. The `Caused by:` chain with `class=Repository (6)` and `code=NotFound (-3)` is internal libgit2 detail that adds noise and may confuse users who try to search for `class=Repository (6)` expecting documentation.
- **Expected**: The `Caused by:` chain should be suppressed for this known error case. The top-level message `Error: '/private/tmp' is not a git repository` is sufficient.
- **Repro**: `commitmux add-repo /tmp`

---

### Area 7: MCP / Serve

#### [SERVE] `commitmux serve` starts silently with no user-visible confirmation
- **Severity**: UX-improvement
- **What happens**: `commitmux serve` starts the MCP JSON-RPC server with zero output to stdout or stderr. When run from a terminal by a user manually verifying their setup, there is no indication whether the server is running and listening, or whether it exited immediately. Confirmed: server reads from stdin and exits on EOF (no TTY detected), so a terminal user who runs it interactively sees a blank prompt until they press Ctrl+D.
- **Expected**: A startup line on stderr such as `commitmux MCP server ready (JSON-RPC over stdio). Ctrl+C to stop.` would confirm the process is live. Since MCP clients connect via stdio, this startup message would not interfere with the JSON-RPC protocol (which uses stdout), as long as it is printed to stderr.
- **Repro**: `commitmux serve` in a terminal — observe: complete silence

---

### Area 3: Core Feature — Sync

#### [SYNC] MCP tip is suppressed on re-sync when no new commits are indexed
- **Severity**: UX-polish
- **What happens**: The MCP onboarding tip (`Tip: run 'commitmux serve' to expose this index via MCP to AI agents.`) only appears when `total_indexed > 0`. On a second sync run where all commits are already indexed (`0 indexed, 43 already indexed`), the tip is not shown. A new user who syncs, is interrupted before reading the tip, and re-syncs will never see it again.
- **Expected**: The tip should appear after any successful sync that results in a non-empty index, not just when new commits were indexed. Alternatively, it could always appear (once per sync) if the repo has commits.
- **Repro**:
  1. `commitmux sync` — tip appears
  2. `commitmux sync` again — tip does not appear (`0 indexed, 43 already indexed`)

---

## Area 8: Output Review Summary

After a full sync with the `commitmux` repo (43 commits), the complete `status` output is:

```
REPO                  COMMITS  SOURCE                                         LAST SYNCED
commitmux                  43  /Users/dayna.blackwell/code/commitmux          2026-02-28 16:09:41 UTC
```

Observations:
- Column alignment: clean and readable for typical path lengths
- `SOURCE` column: fits well for local paths; very long remote URLs would break alignment (no truncation)
- `LAST SYNCED` timestamps: correctly labeled `UTC`
- Sync output correctly distinguishes "already indexed" vs "filtered" as separate counts
- MCP tip appears after first sync, not after re-sync (noted above as UX-polish finding)
- Terminology is consistent: `add-repo`, `remove-repo`, `update-repo`, `sync` all use the verb-noun pattern

---

## Positive Observations (Round 2)

These behaviors worked well and require no changes:

- `commitmux --version` now works correctly: `commitmux 0.1.0`
- `commitmux --help` now lists all subcommands with clear one-line descriptions
- All flag descriptions are populated across all subcommands
- `commitmux init` correctly distinguishes first run from subsequent runs
- Empty status gives a clear actionable hint: `Run: commitmux add-repo <path>`
- `add-repo /tmp` and `add-repo <non-git-path>` fail immediately with a clear message (no deferred failure at sync time)
- Duplicate `add-repo` gives a clean, friendly error with no SQLite internals
- `--url not-a-url` is validated before any clone attempt, with clear protocol list
- `show <repo> <sha>` date field is ISO 8601
- `show <repo> zzz` error message includes both repo name and SHA
- `remove-repo` reports commit count deleted: `Removed repo '<name>' (N commits deleted from index)`
- `sync` output separately counts `already indexed` vs `filtered by author`
- `status` includes `SOURCE` column and `filters:` line for repos with active filters
- `status` LAST SYNCED timestamps include ` UTC` suffix
- `sync` exits non-zero if any repo fails
- All success/informational output goes to stdout; all errors go to stderr (consistent)
- `commitmux blorp` gives clean clap error: `error: unrecognized subcommand 'blorp'`
- Short SHA prefix matching in `show` works correctly
- `sync --repo nonexistent` gives clear `Repo 'nonexistent' not found` error
- `remove-repo nonexistent` gives clear `Repo 'nonexistent' not found` error
