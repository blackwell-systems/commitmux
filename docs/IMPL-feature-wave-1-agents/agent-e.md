# Wave 1 Agent E: CLI — remove-repo, update-repo, --fork-of, --author, --exclude persistence

You are Wave 1 Agent E. Your task is to extend the CLI in `src/main.rs` with
two new subcommands (`remove-repo`, `update-repo`) and extend `add-repo` with
`--fork-of`, `--author`, and persisted `--exclude` flags.

**Prerequisite:** Wave 0 Agent A must complete before you start. Read Agent A's
completion report in `docs/IMPL-feature-wave-1.md` before proceeding.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-e 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-e"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual:   $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-e" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: wave1-agent-e"
  echo "Actual:   $ACTUAL_BRANCH"
  exit 1
fi

git worktree list | grep -q "wave1-agent-e" || {
  echo "ISOLATION FAILURE: Worktree not in git worktree list"
  exit 1
}

echo "Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `src/main.rs` — modify

Do not touch any files under `crates/`.

## 2. Interfaces You Must Implement

New CLI subcommands:

```
commitmux remove-repo <name> [--db <path>]

commitmux update-repo <name>
  [--fork-of <url>]
  [--author <email>]
  [--exclude <prefix>...]    (repeatable, replaces stored list)
  [--default-branch <branch>]
  [--db <path>]
```

Extended `add-repo`:

```
commitmux add-repo [<path> | --url <url>]
  [--name <name>]
  [--fork-of <url>]          (NEW: stored in DB)
  [--author <email>]         (NEW: stored in DB)
  [--exclude <prefix>...]    (CHANGED: now persisted, no longer just warned)
  [--db <path>]
```

## 3. Interfaces You May Call

From `Store` trait (Agent A + B):

```rust
store.remove_repo(name: &str) -> Result<()>
store.update_repo(repo_id: i64, update: &RepoUpdate) -> Result<Repo>
store.get_repo_by_name(name: &str) -> Result<Option<Repo>>
```

From `commitmux-types` (Agent A):

```rust
pub struct RepoUpdate {
    pub fork_of: Option<Option<String>>,
    pub author_filter: Option<Option<String>>,
    pub exclude_prefixes: Option<Vec<String>>,
    pub default_branch: Option<Option<String>>,
}

pub struct RepoInput {
    // now includes: fork_of, author_filter, exclude_prefixes
    pub fork_of: Option<String>,
    pub author_filter: Option<String>,
    pub exclude_prefixes: Vec<String>,
}
```

## 4. What to Implement

Read `src/main.rs` in full before editing.

### 4a. Update `Commands` enum — extend `AddRepo`

Add two new fields to the `AddRepo` variant:

```rust
AddRepo {
    #[arg(conflicts_with = "url")]
    path: Option<PathBuf>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long = "exclude")]
    exclude: Vec<String>,
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(conflicts_with = "path", long)]
    url: Option<String>,
    #[arg(long = "fork-of")]
    fork_of: Option<String>,    // NEW
    #[arg(long = "author")]
    author: Option<String>,     // NEW
},
```

### 4b. Update `AddRepo` handler

**Remove** the `eprintln!` warning block about `--exclude` entirely.

**Pass new fields to `RepoInput`** in both the URL branch and the local path
branch:

```rust
store.add_repo(&RepoInput {
    name: repo_name.clone(),
    local_path: ...,
    remote_url: ...,        // as before
    default_branch: None,
    fork_of: fork_of.clone(),
    author_filter: author.clone(),
    exclude_prefixes: exclude.clone(),
})
```

Apply identically to both branches (URL clone and local path).

### 4c. Add `RemoveRepo` to `Commands` enum

```rust
RemoveRepo {
    name: String,
    #[arg(long)]
    db: Option<PathBuf>,
},
```

### 4d. `RemoveRepo` handler

```rust
Commands::RemoveRepo { name, db } => {
    let db_path = resolve_db_path(db);
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

    // Get repo path before removing (to clean up managed clone)
    let local_path = store
        .get_repo_by_name(&name)
        .with_context(|| format!("Failed to look up repo '{}'", name))?
        .map(|r| r.local_path);

    store.remove_repo(&name)
        .with_context(|| format!("Failed to remove repo '{}'", name))?;

    println!("Removed repo '{}'", name);

    // Clean up managed clone if under ~/.commitmux/clones/
    if let Some(lp) = local_path {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let clones_dir = PathBuf::from(&home).join(".commitmux").join("clones");
        if lp.starts_with(&clones_dir) {
            match std::fs::remove_dir_all(&lp) {
                Ok(_) => println!("Removed managed clone at {}", lp.display()),
                Err(e) => eprintln!(
                    "Warning: failed to remove clone at {}: {}", lp.display(), e
                ),
            }
        }
    }
}
```

### 4e. Add `UpdateRepo` to `Commands` enum

```rust
UpdateRepo {
    name: String,
    #[arg(long = "fork-of")]
    fork_of: Option<String>,
    #[arg(long = "author")]
    author: Option<String>,
    #[arg(long = "exclude")]
    exclude: Vec<String>,
    #[arg(long = "default-branch")]
    default_branch: Option<String>,
    #[arg(long)]
    db: Option<PathBuf>,
},
```

### 4f. `UpdateRepo` handler

```rust
Commands::UpdateRepo { name, fork_of, author, exclude, default_branch, db } => {
    let db_path = resolve_db_path(db);
    let store = SqliteStore::open(&db_path)
        .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

    let repo = store
        .get_repo_by_name(&name)
        .with_context(|| format!("Failed to look up repo '{}'", name))?
        .ok_or_else(|| anyhow::anyhow!("Repo '{}' not found", name))?;

    // Build RepoUpdate: only set fields that were provided via CLI flags.
    let update = commitmux_types::RepoUpdate {
        fork_of: fork_of.map(|v| Some(v)),
        author_filter: author.map(|v| Some(v)),
        exclude_prefixes: if exclude.is_empty() { None } else { Some(exclude) },
        default_branch: default_branch.map(|v| Some(v)),
    };

    let any_change = update.fork_of.is_some()
        || update.author_filter.is_some()
        || update.exclude_prefixes.is_some()
        || update.default_branch.is_some();

    store.update_repo(repo.repo_id, &update)
        .with_context(|| format!("Failed to update repo '{}'", name))?;

    if any_change {
        println!("Updated repo '{}'", name);
    } else {
        println!("Updated repo '{}' (no changes)", name);
    }
}
```

### 4g. Update `use` imports

Add `commitmux_types::RepoUpdate` to the imports at the top of `main.rs`:

```rust
use commitmux_types::{IgnoreConfig, Ingester, RepoInput, RepoUpdate, Store};
```

## 5. Tests to Write

The main binary has no existing unit tests. Add integration-style tests using a
temp DB in `src/main.rs` under `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_store::SqliteStore;
    use commitmux_types::{RepoInput, Store};

    fn temp_store() -> (SqliteStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = SqliteStore::open(&dir.path().join("test.db")).expect("open");
        (db, dir)
    }

    #[test]
    fn test_add_repo_persists_author_filter() {
        let (store, _dir) = temp_store();
        store.add_repo(&RepoInput {
            name: "myrepo".into(),
            local_path: std::path::PathBuf::from("/tmp/myrepo"),
            remote_url: None,
            default_branch: None,
            fork_of: None,
            author_filter: Some("alice@example.com".into()),
            exclude_prefixes: vec![],
        }).expect("add_repo");

        let repo = store.get_repo_by_name("myrepo").expect("get").expect("some");
        assert_eq!(repo.author_filter, Some("alice@example.com".to_string()));
    }

    #[test]
    fn test_add_repo_persists_exclude_prefixes() {
        let (store, _dir) = temp_store();
        store.add_repo(&RepoInput {
            name: "myrepo".into(),
            local_path: std::path::PathBuf::from("/tmp/myrepo"),
            remote_url: None,
            default_branch: None,
            fork_of: None,
            author_filter: None,
            exclude_prefixes: vec!["dist/".into(), "vendor/".into()],
        }).expect("add_repo");

        let repo = store.get_repo_by_name("myrepo").expect("get").expect("some");
        assert_eq!(repo.exclude_prefixes, vec!["dist/", "vendor/"]);
    }
}
```

Note: `tests/integration.rs` is NOT in your file ownership. If you want to add
integration tests there, flag them as out-of-scope dependencies in section 8.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-e
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test -p commitmux
cargo test --test integration
```

Zero compilation errors. All existing tests pass (1 integration test +
2 new unit tests).

## 7. Constraints

- The `--exclude` warning (`eprintln!`) must be removed entirely. The flag now
  persists silently.
- `remove-repo` must fail with a clear error message if the repo is not found
  (the underlying `store.remove_repo` returns `CommitmuxError::NotFound`).
- `update-repo` with no flags prints `"Updated repo '...' (no changes)"`.
- Do NOT add clearing semantics (no `--clear-author`, etc.) — `update-repo`
  only sets new values, never clears to NULL from the CLI (the `RepoUpdate`
  type supports clearing, but the CLI does not expose it in this wave).
- `commitmux_types::RepoUpdate` must be imported cleanly; avoid wildcard imports.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-e
git add src/main.rs
git commit -m "wave1-agent-e: remove-repo, update-repo, --fork-of, --author, --exclude persistence"
```

Append your completion report to
`/Users/dayna.blackwell/code/commitmux/docs/IMPL-feature-wave-1.md`
under `### Agent E — Completion Report`.

Include:
- What you implemented
- Test results (pass/fail, count)
- Deviations from spec
- Interface contract changes
- Out-of-scope dependencies (e.g., integration test additions)
