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

### Fixed

- `add-repo` command was missing the `--db` flag, preventing database path override.
- Panic on patch preview retrieval when a multi-byte UTF-8 character fell on a truncation boundary.
- SSH agent authentication failures when cloning or fetching from SSH remotes.
- License copyright attribution.

[Unreleased]: https://github.com/blackwell-systems/commitmux/commits/master
