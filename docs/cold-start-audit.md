# commitmux Cold-Start UX Audit

**Date**: 2026-02-28
**Auditor**: Claude (acting as new user)
**Version**: commitmux (built from master, post-README commits)

---

## Summary Table

| Severity | Count |
|---|---|
| UX-critical | 3 |
| UX-improvement | 9 |
| UX-polish | 6 |
| **Total** | **18** |

---

## Findings

### 1. Discovery

#### [DISCOVERY] All subcommands are missing descriptions in top-level help
- **Severity**: UX-critical
- **What happens**: `commitmux --help` lists all 8 subcommands with no descriptions next to them. Every entry is blank after the command name. A new user cannot distinguish `init` from `sync` from `serve` without prior knowledge.
- **Expected**: Each subcommand should have a one-line description, e.g. `init    Initialize the commitmux database`, `serve   Start the MCP JSON-RPC server`, etc.
- **Repro**: `commitmux --help`

```
Commands:
  init
  add-repo
  remove-repo
  update-repo
  sync
  show
  status
  serve
  help         Print this message or the help of the given subcommand(s)
```

---

#### [DISCOVERY] All flags in every subcommand help are missing descriptions
- **Severity**: UX-critical
- **What happens**: Every subcommand's `--help` output shows flags with no description text. `add-repo --help` shows `--name`, `--exclude`, `--url`, `--fork-of`, `--author` with no explanation of what any of them do.
- **Expected**: Each flag should have a brief description, e.g. `--name <NAME>    Override the repo name (defaults to directory name)`, `--author <AUTHOR>    Only index commits by this author (email or name substring)`.
- **Repro**: `commitmux add-repo --help`, `commitmux update-repo --help`, `commitmux sync --help`, etc.

```
Options:
      --name <NAME>
      --exclude <EXCLUDE>
      --db <DB>
      --url <URL>
      --fork-of <FORK_OF>
      --author <AUTHOR>
  -h, --help               Print help
```

---

#### [DISCOVERY] The `[PATH]` argument in `add-repo --help` is not described
- **Severity**: UX-improvement
- **What happens**: `add-repo --help` shows `Arguments: [PATH]` with no explanation of what it is, whether it should be absolute or relative, or that it must point to a git repo. The relationship between `[PATH]` and `--url` is also unexplained.
- **Expected**: The `PATH` argument should be described as the local filesystem path to a git repository. A note should clarify that `PATH` and `--url` are mutually exclusive alternatives.
- **Repro**: `commitmux add-repo --help`

---

#### [DISCOVERY] No `--version` flag
- **Severity**: UX-polish
- **What happens**: `commitmux --version` returns an error: `error: unexpected argument '--version' found`.
- **Expected**: Standard CLI tools expose `--version` so users can confirm what build they are running.
- **Repro**: `commitmux --version`

---

### 2. Setup / Onboarding

#### [ONBOARDING] `init` is idempotent but gives no indication of it
- **Severity**: UX-improvement
- **What happens**: Running `commitmux init` a second time prints the exact same success message as the first run: `Initialized commitmux database at <path>`. There is no way for the user to know whether the database was newly created or already existed.
- **Expected**: On second run the output should distinguish the two states, e.g. `Database already exists at <path>` or `Database at <path> is up to date`.
- **Repro**: Run `commitmux init` twice in a row.

---

#### [ONBOARDING] Empty `status` shows only a header row — no hint that repos need to be added
- **Severity**: UX-improvement
- **What happens**: After `init` but before any `add-repo`, `commitmux status` outputs only the column headers with no rows and no explanatory text. A new user may not know whether the tool is working or broken.
- **Expected**: An empty state message such as `No repos indexed. Run: commitmux add-repo <path>` would orient the user toward the next step.
- **Repro**: `commitmux init && commitmux status`

```
REPO                  COMMITS  LAST SYNCED
```

---

#### [ONBOARDING] No mention of `init` being required before other commands
- **Severity**: UX-polish
- **What happens**: The top-level `--help` does not indicate that `init` must be run first. New users may run `add-repo` before `init` and get a cryptic database error.
- **Expected**: The top-level help or a `Getting Started` note should indicate the required workflow: `init` → `add-repo` → `sync`.
- **Repro**: `commitmux --help` (note absence of ordering guidance)

---

### 3. Core Feature — Add and Sync

#### [ADD-REPO] Adding a non-git directory succeeds silently, fails only at sync time
- **Severity**: UX-critical
- **What happens**: `commitmux add-repo /tmp` succeeds with `Added repo 'tmp' at /private/tmp` even though `/tmp` is not a git repository. The error is deferred until `commitmux sync`, which prints `Error syncing 'tmp': ingest error: could not find repository at '/private/tmp'`. The repo remains in the index in a permanently broken state with 0 commits and `never` as `LAST SYNCED`.
- **Expected**: `add-repo` should validate that the given path is a git repository at registration time and fail immediately with a clear error. If deferred validation is intentional (e.g. to support repos not yet cloned), the user should be warned that the path is not currently a git repo.
- **Repro**: `commitmux add-repo /tmp`

---

#### [SYNC] `sync` exits 0 even when one or more repos fail
- **Severity**: UX-improvement
- **What happens**: When `commitmux sync` is run and one repo errors (e.g. not a git repo), the process exits with code 0. The error message is printed to stderr but the exit code gives no signal of partial failure.
- **Expected**: If any repo fails to sync, `commitmux sync` should exit with a non-zero exit code so that scripts and CI can detect failures.
- **Repro**: Add a non-git path with `commitmux add-repo /tmp`, then run `commitmux sync`; check `echo $?` → `0`.

---

#### [SYNC] "skipped" in sync output is ambiguous — means both "already indexed" and "author-filtered"
- **Severity**: UX-improvement
- **What happens**: `commitmux sync` reports `N commits indexed, M skipped`. "Skipped" is used for two different reasons: commits already present in the index, and commits filtered out by an `--author` setting. A user who sets an author filter and sees `34 skipped` cannot tell whether the commits were previously indexed or filtered.
- **Expected**: Use distinct language for the two cases, e.g. `34 already indexed` vs `34 filtered by author`. Or report them as separate numbers: `0 indexed, 34 already indexed, 0 filtered`.
- **Repro**: Add a repo, sync (34 indexed), sync again (34 skipped). Then set `--author user@example.com` on the same repo, sync (34 skipped). Output is identical despite different causes.

---

#### [STATUS] Status does not show repo path or URL
- **Severity**: UX-improvement
- **What happens**: `commitmux status` shows only `REPO`, `COMMITS`, and `LAST SYNCED` columns. When a custom `--name` is used, there is no way to see which local path or remote URL the name corresponds to.
- **Expected**: Status should include a `PATH` or `SOURCE` column (or show it on a detail line) so users can verify what each name refers to.
- **Repro**: `commitmux add-repo ~/code/someproject --name custom-name && commitmux status`

---

#### [STATUS] Configured metadata (author filter, fork-of, exclude) is invisible after `update-repo`
- **Severity**: UX-improvement
- **What happens**: After `commitmux update-repo custom-name --author user@example.com`, running `commitmux status` shows no indication that an author filter is active. A user debugging why 0 commits are indexed cannot see from `status` that filtering is the cause.
- **Expected**: Status should show active filters, either as additional columns or in a verbose mode. At minimum, a `*` or `(filtered)` annotation next to 0-commit repos with active filters would help.
- **Repro**: `commitmux update-repo <name> --author user@example.com && commitmux status`

---

### 4. Data / Tracking

#### [SHOW] Commit `date` field is a raw Unix timestamp, not a human-readable date
- **Severity**: UX-improvement
- **What happens**: `commitmux show <repo> <sha>` outputs JSON with `"date": 1772284088`. For human consumers reviewing output in a terminal, this is not readable without running a separate conversion.
- **Expected**: The `date` field should be an ISO 8601 string (e.g. `"2026-02-28T13:08:08Z"`) or include both formats. Since the MCP tool consumers are AI agents, an ISO string is also more useful for them than a raw integer epoch.
- **Repro**: `commitmux show claudewatch e4ee79d`

```json
{
  "date": 1772284088,
  ...
}
```

---

#### [SHOW] `Commit not found` error message does not include the repo name or SHA that was searched
- **Severity**: UX-polish
- **What happens**: `commitmux show <repo> zzz` outputs only `Commit not found` with no context about what repo or SHA was searched. If the user mistyped either argument, they cannot confirm which part was wrong from the error.
- **Expected**: `Commit 'zzz' not found in repo 'claudewatch'`
- **Repro**: `commitmux show claudewatch zzz`

---

### 5. Destructive / Write Operations

#### [REMOVE-REPO] `remove-repo` has no confirmation prompt and no mention of data loss
- **Severity**: UX-polish
- **What happens**: `commitmux remove-repo custom-name` immediately removes the repo and all its indexed commits with the single-line output `Removed repo 'custom-name'`. There is no `--yes` flag, no confirmation prompt, and no mention of how many commits were deleted.
- **Expected**: Either a `--yes`/`--force` flag to acknowledge destructive intent, or an output line noting how many commits were removed, e.g. `Removed repo 'custom-name' (34 commits deleted from index)`.
- **Repro**: `commitmux remove-repo <name>`

---

### 6. Edge Cases

#### [EDGE] Duplicate `add-repo` exposes raw SQLite constraint error
- **Severity**: UX-improvement
- **What happens**: Adding a repo with a name that already exists produces:
  ```
  Error: Failed to add repo 'tmp'

  Caused by:
      0: store error: UNIQUE constraint failed: repos.name
      1: UNIQUE constraint failed: repos.name
      2: Error code 2067: A UNIQUE constraint failed
  ```
  The raw SQLite error chain is exposed including the internal error code (`2067`) and the table name (`repos.name`).
- **Expected**: A clean message like `Error: a repo named 'tmp' already exists. Use 'commitmux status' to see all repos.`
- **Repro**: `commitmux add-repo /tmp` (after `/tmp` is already registered)

---

#### [EDGE] `--url` with invalid string attempts a clone before any URL validation
- **Severity**: UX-polish
- **What happens**: `commitmux add-repo --url not-a-url` prints `Cloning not-a-url from not-a-url...` to stdout before failing with `Error: Failed to clone 'not-a-url' from 'not-a-url' — Caused by: unsupported URL protocol; class=Net (12)`. The `class=Net (12)` detail is an internal libgit2 error code.
- **Expected**: The URL should be validated before a clone attempt. The error message should say `'not-a-url' is not a valid URL` without exposing internal library error classes.
- **Repro**: `commitmux add-repo --url not-a-url`

---

#### [EDGE] `commitmux` with no subcommand prints help to stderr and exits 2
- **Severity**: UX-polish
- **What happens**: Running `commitmux` with no arguments outputs the full help text to stderr (not stdout) and exits with code 2. Exit code 2 is typically reserved for argument parse errors; printing usage text to stderr is unconventional when no error has been made.
- **Expected**: Running a CLI tool with no arguments is common and not an error. Help should go to stdout and exit with 0, or a brief hint like `Run 'commitmux --help' for usage` should go to stderr with exit 1.
- **Repro**: `commitmux 2>/dev/null` → no output; `commitmux 1>/dev/null` → shows help; `echo $?` → 2

---

### 7. MCP / Serve

#### [SERVE] `commitmux serve` produces no startup output whatsoever
- **Severity**: UX-improvement
- **What happens**: `commitmux serve` starts silently. There is no output to stdout or stderr indicating the server is running, what transport it uses, or how to connect to it. A user who runs it in a terminal has no confirmation that anything happened.
- **Expected**: At minimum, a startup line such as `commitmux MCP server listening on stdio` or `MCP server started (JSON-RPC over stdio). Press Ctrl+C to stop.` so the user can confirm the server launched.
- **Repro**: `commitmux serve` (observe: complete silence until Ctrl+C)

---

#### [SERVE] No onboarding path from CLI to MCP usage
- **Severity**: UX-improvement
- **What happens**: The tool's primary value proposition is providing an MCP interface for AI agents, but nothing in the CLI experience points the user toward `serve`. The top-level help has no description for `serve`, `commitmux serve --help` has no description, and there is no `Getting Started` or `Next Steps` output anywhere.
- **Expected**: After a successful `sync`, the tool could print a tip such as `Tip: run 'commitmux serve' to expose this index via MCP to AI agents. See docs/mcp.md for configuration.` Alternatively, the `serve` subcommand description in help should explain its purpose.
- **Repro**: Complete the full `init` → `add-repo` → `sync` flow and observe: no mention of MCP, serve, or next steps.

---

### 8. Output Review

#### [OUTPUT] `status` LAST SYNCED timestamp is an absolute datetime with no timezone indicator
- **Severity**: UX-polish
- **What happens**: `commitmux status` shows `LAST SYNCED` as `2026-02-28 15:34:55` with no timezone label. It is unclear whether this is local time or UTC.
- **Expected**: Include a timezone label (e.g. `2026-02-28 15:34:55 MST` or `2026-02-28 22:34:55 UTC`) or use a relative format (e.g. `2 minutes ago`) for better readability.
- **Repro**: `commitmux sync && commitmux status`

---

## Positive Observations

These behaviors worked well and require no changes:

- `add-repo` with `--name` correctly overrides the default name derived from directory.
- Short SHA prefix matching in `show` works correctly (7-char prefix resolves to full SHA).
- `remove-repo nonexistent` gives a clear structured error with the repo name included.
- `sync --repo nonexistent` gives a clear `Repo 'nonexistent' not found` error.
- `show` missing-args error from clap is informative and lists both required arguments.
- Success messages (`Added repo`, `Removed repo`, `Updated repo`, `Syncing...`) are consistently terse and machine-friendly.
- All error output correctly goes to stderr; success output goes to stdout — with one exception noted above (`sync` progress lines go to stdout).
- No color output means the tool is pipe-friendly and works in all terminal environments without extra flags.
