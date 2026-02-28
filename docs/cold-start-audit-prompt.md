# Cold-Start UX Audit Prompt

**Metadata:**
- Audit Date: 2026-02-28
- Tool Version: commitmux (no --version flag)
- Sandbox mode: local
- Sandbox: COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.w5JNXOXLkR/db.sqlite3
- Environment: host (macOS)

---

You are performing a UX audit of `commitmux` — a tool that builds a cross-repo git history index for AI agents.
You are acting as a **new user** encountering this tool for the first time.

You are running `commitmux` on the host with state isolated to a temp directory via env vars:
`COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.w5JNXOXLkR/db.sqlite3`
The tool's real data at `~/.commitmux/db.sqlite3` is unaffected.

Run all commands using: `env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.w5JNXOXLkR/db.sqlite3 ~/.cargo/bin/commitmux`

For brevity, the audit areas below write this as: `commitmux` — but every command MUST be prefixed with `env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.w5JNXOXLkR/db.sqlite3 ~/.cargo/bin/commitmux`.

## Audit Areas

### 1. Discovery

- `commitmux --help` — read top-level help: is the description clear? are subcommands listed with descriptions?
- `commitmux init --help`
- `commitmux add-repo --help`
- `commitmux remove-repo --help`
- `commitmux update-repo --help`
- `commitmux sync --help`
- `commitmux show --help`
- `commitmux status --help`
- `commitmux serve --help`

Note: does each subcommand have a description in the top-level `--help`? Are flag descriptions present or blank?

### 2. Setup / Onboarding

- `commitmux init` — initialize the database. What output does the user see? Is the path shown?
- `commitmux status` — before any repos are added. What does an empty index look like?
- `commitmux init` again — what happens on second run? Is it idempotent and clear?

### 3. Core Feature — Add a repo and sync

- `commitmux add-repo /tmp` — add a local path (use /tmp as it exists but is not a git repo). What error does the user see?
- `commitmux add-repo /var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.w5JNXOXLkR` — add the temp dir (not a git repo). Error message quality?
- Find a real git repo to add: `ls ~/code/ | head -5` then pick one
- `commitmux add-repo ~/code/<repo>` — add a real local git repo. What output is shown?
- `commitmux status` — after adding. Is the repo listed? What info is shown?
- `commitmux sync` — sync all repos. Progress output? Duration? Counts?
- `commitmux sync --repo <name>` — sync a single repo.
- `commitmux status` — after sync. Are commit counts and last-synced timestamps shown?

### 4. Data / Tracking

- `commitmux show <repo> <sha>` — use a real SHA from the synced repo. Is output readable? Format?
- `commitmux show <repo> abc123` — short SHA. Does prefix matching work?
- `commitmux show <repo> zzz` — nonexistent SHA. Error message quality?

### 5. Add repo with flags

- `commitmux add-repo ~/code/<repo2> --name custom-name` — custom name. Confirm in status.
- `commitmux add-repo ~/code/<repo> --exclude vendor/` — exclude prefix. No confirmation of what was excluded?
- `commitmux update-repo <name> --author user@example.com` — update author filter.
- `commitmux status` — does the display reflect any of the metadata set above?

### 6. Destructive / Write Operations

- `commitmux remove-repo nonexistent` — remove a repo that doesn't exist. Error quality?
- `commitmux remove-repo <name>` — remove a real repo. Output? Confirmation prompt?
- `commitmux status` — confirm removal is reflected.

### 7. Edge Cases

- `commitmux` — no subcommand. What does the user see?
- `commitmux blorp` — unknown subcommand. Error message?
- `commitmux add-repo` — no path and no --url. Error message?
- `commitmux show` — missing required args. Error message?
- `commitmux sync --repo nonexistent` — sync a repo name that doesn't exist.
- `commitmux add-repo --url not-a-url` — invalid URL. How does it fail?

### 8. Output Review

- Re-run `commitmux status` after a full sync and evaluate:
  - Column alignment and header clarity
  - Whether "LAST SYNCED" timestamps are human-readable
  - Whether units are clear (commits indexed, commits skipped)
  - Terminology consistency: "repo" vs "repository" across subcommands
  - Whether the tool has any color output or is plain text

### 9. MCP / Serve

- `commitmux serve` — start the MCP server. Does it print anything? Does it block? Send Ctrl+C after 2 seconds and note behavior.
- Note: this is the primary value proposition. Is there any onboarding text pointing users toward the MCP interface?

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

## Report

- Group findings by area
- Include a summary table at the top: total count by severity
- Write the complete report to `/Users/dayna.blackwell/code/commitmux/docs/cold-start-audit.md` using the Write tool

IMPORTANT: Run ALL commands prefixed with `env COMMITMUX_DB=/var/folders/3z/jbjrfl4578z013f8fdh5w0tr0000gp/T/tmp.w5JNXOXLkR/db.sqlite3 ~/.cargo/bin/commitmux`.
Do not bypass the sandbox — do not run commitmux without the COMMITMUX_DB env var.
