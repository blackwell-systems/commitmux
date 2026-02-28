# Wave 2 Agent A: CLI — --embed flags, config subcommand, status EMBED column

You are Wave 2 Agent A. Your task is to update `src/main.rs` with:
- `--embed` / `--no-embed` flags on `add-repo` and `update-repo`
- `config` subcommand (`set` and `get`)
- `--embed-only` flag on `sync`
- `EMBED` column in `commitmux status` output
- `embed_enabled: <value>` field in all `RepoInput` construction sites

Wave 1A and 1B are already merged. All new Store trait methods and the `crates/embed` crate exist.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-a 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-a"
if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"; echo "Actual: $ACTUAL_DIR"; exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave2-agent-a" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: wave2-agent-a"; echo "Actual: $ACTUAL_BRANCH"; exit 1
fi

git worktree list | grep -q "wave2-agent-a" || { echo "ISOLATION FAILURE: Not in worktree list"; exit 1; }
echo "✓ Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `src/main.rs` — modify

Do NOT touch any other files.

## 2. Interfaces You Must Implement

No new public interfaces. All changes are internal to `fn main()` and clap struct definitions.

## 3. Interfaces You May Call

```rust
// From crates/types (Wave 1A):
Repo { ..., embed_enabled: bool }
RepoInput { ..., embed_enabled: bool }
RepoUpdate { ..., embed_enabled: Option<bool> }
EmbedCommit, SemanticSearchOpts   // (not needed in main.rs directly)
store.get_config(key) -> Result<Option<String>>
store.set_config(key, value) -> Result<()>

// From crates/embed (Wave 1B):
commitmux_embed::EmbedConfig
commitmux_embed::Embedder
commitmux_embed::EmbedSummary
commitmux_embed::embed_pending(store, embedder, repo_id, batch_size) -> Future<EmbedSummary>
```

## 4. What to Implement

Read `src/main.rs` in full before making any changes. Read `docs/vector-embeddings.md` for context.

### 4a. Add `commitmux-embed` dependency to root `Cargo.toml`

In the root `Cargo.toml`, add:
```toml
commitmux-embed = { path = "crates/embed" }
```

Wait — this file is owned by Wave 1B. Check whether Wave 1B already added it to `Cargo.toml`'s
`[dependencies]` section (they added the workspace member entry; the binary crate dependency is
separate). If NOT already present, you must add it. This is an exception justified by atomicity:
`src/main.rs` cannot compile without the dependency declared.

### 4b. `--embed` / `--no-embed` on `AddRepo`

In the `AddRepo` variant, add:
```rust
#[arg(long = "embed", help = "Enable semantic embeddings for this repo")]
embed: bool,
```

Note: clap's `bool` with `long` flag acts as a flag (present = true). For `--no-embed` as an
explicit negation, use a `flag` pair. The simplest approach: just `--embed` flag (bool).
`--no-embed` can be added later if needed. For now, `add-repo --embed` sets `embed_enabled = true`.

In the handler, pass `embed_enabled: embed` in the `RepoInput` construction.

### 4c. `--embed` / `--no-embed` on `UpdateRepo`

In the `UpdateRepo` variant, add:
```rust
#[arg(long = "embed", help = "Enable semantic embeddings for this repo")]
embed: bool,
#[arg(long = "no-embed", help = "Disable semantic embeddings for this repo")]
no_embed: bool,
```

In the handler:
```rust
let embed_enabled = if embed { Some(true) } else if no_embed { Some(false) } else { None };
let update = RepoUpdate {
    embed_enabled,
    // ...existing fields...
};
```

### 4d. `config` subcommand

Add a new `Config` variant to `Commands`:

```rust
#[command(about = "Get or set global configuration values")]
Config {
    #[command(subcommand)]
    action: ConfigAction,
    #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
    db: Option<PathBuf>,
},
```

And a new enum:

```rust
#[derive(Subcommand)]
enum ConfigAction {
    #[command(about = "Set a configuration value")]
    Set {
        #[arg(help = "Configuration key (e.g. embed.model, embed.endpoint)")]
        key: String,
        #[arg(help = "Value to set")]
        value: String,
    },
    #[command(about = "Get a configuration value")]
    Get {
        #[arg(help = "Configuration key")]
        key: String,
    },
}
```

Handler in `match cli.command`:
```rust
Commands::Config { action, db } => {
    let db_path = resolve_db_path(db);
    if !db_path.exists() {
        anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
    }
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("Failed to open database at {}", db_path.display()))?;
    match action {
        ConfigAction::Set { key, value } => {
            store.set_config(&key, &value).context("Failed to set config")?;
            println!("Set {} = {}", key, value);
        }
        ConfigAction::Get { key } => {
            match store.get_config(&key).context("Failed to get config")? {
                Some(value) => println!("{}", value),
                None => println!("(not set)"),
            }
        }
    }
}
```

### 4e. `--embed-only` flag on `Sync`

In the `Sync` variant, add:
```rust
#[arg(long = "embed-only", help = "Only generate embeddings; skip commit indexing")]
embed_only: bool,
```

In the `Sync` handler, after the existing sync loop, if `embed_only` is true:
- Skip the git ingest loop entirely
- For each repo (or the specified repo if `--repo` is set) where `repo.embed_enabled`:
  - Load `EmbedConfig::from_store(&store)`, construct `Embedder::new(&config)`
  - Call `embed_pending` via `tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(...)`
  - Print progress: `"Embedding '{}'... {} embedded, {} failed"`

If `embed_only` is false (normal sync), add embed backfill after ingest for repos with `embed_enabled`:
```rust
// After ingest loop, for each repo where r.embed_enabled:
if r.embed_enabled {
    match EmbedConfig::from_store(&store) {
        Ok(config) => {
            let embedder = commitmux_embed::Embedder::new(&config);
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            match rt.block_on(commitmux_embed::embed_pending(&store, &embedder, r.repo_id, 50)) {
                Ok(esummary) => {
                    if esummary.embedded > 0 || esummary.failed > 0 {
                        println!("  Embedded {} commits ({} failed)", esummary.embedded, esummary.failed);
                    }
                }
                Err(e) => eprintln!("  Warning: embedding failed for '{}': {e}", r.name),
            }
        }
        Err(e) => eprintln!("  Warning: embed config error for '{}': {e}", r.name),
    }
}
```

Add `tokio` to root `Cargo.toml` dependencies (runtime only, not full tokio):
```toml
tokio = { version = "1", features = ["rt"] }
```

### 4f. EMBED column in `commitmux status`

In `Commands::Status`, determine whether any repo has `embed_enabled = true`.
If yes, add an `EMBED` column to the header and row output, and a footer line.

```rust
let any_embed = repos.iter().any(|r| r.embed_enabled);

if any_embed {
    println!("{:<20} {:>8}  {:<45}  {:<22}  EMBED", "REPO", "COMMITS", "SOURCE", "LAST SYNCED");
} else {
    println!("{:<20} {:>8}  {:<45}  LAST SYNCED", "REPO", "COMMITS", "SOURCE");
}

// In the per-repo output:
if any_embed {
    let embed_col = if r.embed_enabled { "✓" } else { "-" };
    println!("{:<20} {:>8}  {:<45}  {:<22}  {}", r.name, stats.commit_count, source, last_synced, embed_col);
} else {
    println!("{:<20} {:>8}  {:<45}  {}", r.name, stats.commit_count, source, last_synced);
}
```

After the repos loop, if `any_embed`, print:
```rust
let model = store.get_config("embed.model").ok().flatten()
    .unwrap_or_else(|| "nomic-embed-text (default)".into());
let endpoint = store.get_config("embed.endpoint").ok().flatten()
    .unwrap_or_else(|| "http://localhost:11434/v1 (default)".into());
println!("\nEmbedding model: {} ({})", model, endpoint);
```

## 5. Tests to Write

Add to `src/main.rs` `#[cfg(test)]`:

1. `test_config_set_get_roundtrip` — create a temp store, call `store.set_config("embed.model", "test-model")`, call `store.get_config("embed.model")`, assert value matches.
2. `test_embed_sync_tip_logic` — `embed_only = true` with `embed_enabled = false` repo: verify no embedding is attempted (logic test on condition, not actual call).

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-a
cargo build
cargo clippy -- -D warnings
cargo test -p commitmux
```

All existing tests must pass. New tests must pass.

## 7. Constraints

- All informational output goes to stdout. Error messages go to stderr via anyhow.
- `tokio::runtime::Builder::new_current_thread()` is the right runtime choice — single-threaded,
  no need for a multi-threaded executor for one blocking call.
- Do NOT add `tokio` as a `full` feature — only `"rt"` (and `"rt-multi-thread"` is not needed).
- The `--embed-only` flag is independent of `--repo` — if both are specified, embed only the
  specified repo.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave2-agent-a
git add src/main.rs Cargo.toml
git commit -m "wave2-agent-a: add --embed flags, config subcommand, --embed-only sync, status EMBED column"
```

Append to `docs/IMPL-vector-embeddings.md` under `### Agent 2A — Completion Report`:

```yaml
### Agent 2A — Completion Report
status: complete | partial | blocked
worktree: .claude/worktrees/wave2-agent-a
commit: {sha}
files_changed:
  - src/main.rs
  - Cargo.toml
files_created: []
interface_deviations: []
out_of_scope_deps: []
tests_added:
  - test_config_set_get_roundtrip
  - test_embed_sync_tip_logic
verification: PASS | FAIL ({command} — N/N tests)
```
