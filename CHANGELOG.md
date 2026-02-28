# Changelog

All notable changes to commitmux will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `add-repo --url <git-url>` flag: register a remote repository by URL. commitmux auto-clones the repo to `~/.commitmux/clones/<name>/` on first sync.
- Automatic remote fetch before ingest when a repo was registered with `--url`. Running `commitmux sync` on a URL-based repo pulls the latest commits from the remote before walking history.
- SSH agent authentication support for clone and fetch operations against SSH remotes.
- MIT license.
- MCP integration reference (`docs/mcp.md`): full tool surface, host configuration examples for Claude Desktop, Cursor, and Zed, security model, freshness considerations, and raw protocol examples.
- README with quick-start guide, full CLI reference, MCP tools reference, configuration, and MCP host setup instructions.
- `update-repo` command: update stored metadata for a repository (name, author filter, exclude prefixes, fork-of URL, default branch).
- `add-repo --fork-of <url>`: only index commits not present in the upstream repo, using merge-base exclusion.
- `add-repo --author <email>`: only index commits by a specific author (email substring match). Also available on `update-repo`.
- `add-repo --exclude <prefix>`: exclude a path prefix from indexing (repeatable). Also available on `update-repo`.
- `commitmux_list_repos` MCP tool: returns all indexed repositories with commit counts and last-synced timestamps.
- Incremental sync: subsequent `sync` runs skip commits already present in the index, reporting `already indexed` counts separately from newly indexed counts.
- `--version` flag: `commitmux --version` now returns the build version from Cargo.toml.
- Descriptions for all subcommands and flags in `--help` output.
- SOURCE column in `commitmux status`: shows remote URL or local path for each repo so custom `--name` entries can be traced back to their source.
- Active filter display in `commitmux status`: repos with `--author` or `--exclude` filters show an indented `filters:` line.
- Empty-state message in `commitmux status`: when no repos are indexed, prints a hint to run `commitmux add-repo`.
- MCP onboarding tip after a successful sync: `Tip: run 'commitmux serve' to expose this index via MCP to AI agents.`
- `remove-repo` now reports the number of commits deleted from the index.
- Git repository validation on `add-repo <path>`: fails immediately with a clear error if the path is not a git repository, rather than deferring the error to sync time.
- URL scheme validation on `add-repo --url`: rejects unrecognized schemes before attempting a clone.

### Changed

- `commitmux sync` output now distinguishes between commits skipped because they were already indexed (`already indexed`) and commits skipped by an author filter (`filtered by author`). Previously both were reported as `skipped`.
- `commitmux sync` exits with code 1 if any repository fails to sync, enabling scripts and CI to detect partial failures.
- `commitmux show` not-found error now includes the repo name and SHA searched: `Commit 'abc123' not found in repo 'myrepo'`.
- `commitmux status` timestamps now include a UTC label (e.g. `2026-02-28 15:34:55 UTC`).
- `commitmux show` date field is now an ISO 8601 UTC string (e.g. `"2026-02-28T15:34:55Z"`) instead of a raw Unix timestamp integer.
- Duplicate `add-repo` name error now shows a clean message (`A repo named '...' already exists`) instead of exposing the raw SQLite UNIQUE constraint error.

### Fixed

- `add-repo` command was missing the `--db` flag, preventing database path override.
- Panic on patch preview retrieval when a multi-byte UTF-8 character fell on a truncation boundary.
- SSH agent authentication failures when cloning or fetching from SSH remotes.
- License copyright attribution.

[Unreleased]: https://github.com/blackwell-systems/commitmux/commits/master
