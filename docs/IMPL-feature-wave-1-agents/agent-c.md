# Wave 1 Agent C: Ingest walker — author filter, exclude persistence, fork-of, incremental skip

You are Wave 1 Agent C. Your task is to extend the `sync_repo` function in
`crates/ingest/src/walker.rs` to apply four new ingest behaviors driven by
fields on the `Repo` struct: author filter, persisted exclude prefixes,
fork-of upstream exclusion, and incremental commit skipping.

**Prerequisite:** Wave 0 Agent A must complete before you start. Read Agent A's
completion report in `docs/IMPL-feature-wave-1.md` before proceeding.

## 0. CRITICAL: Isolation Verification (RUN FIRST)

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-c 2>/dev/null || true

ACTUAL_DIR=$(pwd)
EXPECTED_DIR="/Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-c"

if [ "$ACTUAL_DIR" != "$EXPECTED_DIR" ]; then
  echo "ISOLATION FAILURE: Wrong directory"
  echo "Expected: $EXPECTED_DIR"
  echo "Actual:   $ACTUAL_DIR"
  exit 1
fi

ACTUAL_BRANCH=$(git branch --show-current)
if [ "$ACTUAL_BRANCH" != "wave1-agent-c" ]; then
  echo "ISOLATION FAILURE: Wrong branch"
  echo "Expected: wave1-agent-c"
  echo "Actual:   $ACTUAL_BRANCH"
  exit 1
fi

git worktree list | grep -q "wave1-agent-c" || {
  echo "ISOLATION FAILURE: Worktree not in git worktree list"
  exit 1
}

echo "Isolation verified: $ACTUAL_DIR on $ACTUAL_BRANCH"
```

## 1. File Ownership

- `crates/ingest/src/walker.rs` — modify

Do not touch `crates/ingest/src/lib.rs`, `crates/ingest/src/patch.rs`, or any
other file.

**Exception:** If `MockStore.commit_exists` is still `unimplemented!()` after
Agent A's merge (not a real implementation), you must update it in
`crates/ingest/src/lib.rs` to make your tests pass. Document this in section 8.

## 2. Interfaces You Must Implement

No public signature changes. All new behavior is internal to `sync_repo`. You
consume these new fields from `Repo` (Agent A's output):

```rust
repo.fork_of: Option<String>        // upstream remote URL for merge-base exclusion
repo.author_filter: Option<String>  // if set, only index commits with this author email
repo.exclude_prefixes: Vec<String>  // merged with config.path_prefixes for this sync
```

And this new `Store` method (Agent A's output):
```rust
store.commit_exists(repo_id: i64, sha: &str) -> Result<bool>
```

## 3. Interfaces You May Call

```rust
// From commitmux-types (Agent A):
repo.fork_of: Option<String>
repo.author_filter: Option<String>
repo.exclude_prefixes: Vec<String>
store.commit_exists(repo_id: i64, sha: &str) -> Result<bool>

// git2:
git2::Repository::find_remote(name: &str) -> Result<Remote>
git2::Repository::remote(name: &str, url: &str) -> Result<Remote>
git2::Repository::remote_set_url(name: &str, url: &str) -> Result<()>
git2::Repository::merge_base(one: Oid, two: Oid) -> Result<Oid>
git2::RevWalk::hide(oid: Oid) -> Result<()>
git2::Repository::revparse_single(spec: &str) -> Result<Object>
```

## 4. What to Implement

Read `crates/ingest/src/walker.rs` in full before making any changes.

### 4a. Effective IgnoreConfig — merge persisted excludes

At the top of `sync_repo`, before the revwalk setup, construct an
`effective_config`:

```rust
let effective_config = if repo.exclude_prefixes.is_empty() {
    config.clone()
} else {
    let mut merged = config.clone();
    for p in &repo.exclude_prefixes {
        if !merged.path_prefixes.contains(p) {
            merged.path_prefixes.push(p.clone());
        }
    }
    merged
};
```

Use `&effective_config` in all calls to `patch::get_commit_files(...)` and
`patch::get_patch_text(...)`. Do NOT use `config` directly after this point.

### 4b. Fork-of upstream exclusion

After `let tip_commit = resolve_tip(...)` and before setting up the revwalk,
add fork-of handling:

```rust
if let Some(ref upstream_url) = repo.fork_of {
    // Step 1: ensure "upstream" remote exists with correct URL
    let needs_create = match git_repo.find_remote("upstream") {
        Ok(existing) => {
            let existing_url = existing.url().unwrap_or("").to_string();
            if existing_url != upstream_url.as_str() {
                // Wrong URL — update it
                if let Err(e) = git_repo.remote_set_url("upstream", upstream_url) {
                    summary.errors.push(format!(
                        "Warning: failed to update upstream remote URL: {}", e.message()
                    ));
                }
            }
            false
        }
        Err(_) => true,
    };

    if needs_create {
        if let Err(e) = git_repo.remote("upstream", upstream_url) {
            summary.errors.push(format!(
                "Warning: failed to add upstream remote: {}", e.message()
            ));
            // Skip fork-of logic entirely
        }
    }

    // Step 2: fetch upstream (non-fatal)
    if let Ok(mut remote) = git_repo.find_remote("upstream") {
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(|_url, username, _allowed| {
            git2::Cred::ssh_key_from_agent(username.unwrap_or("git"))
        });
        let mut fo = git2::FetchOptions::new();
        fo.remote_callbacks(callbacks);
        if let Err(e) = remote.fetch::<&str>(&[], Some(&mut fo), None) {
            summary.errors.push(format!(
                "Warning: failed to fetch upstream: {}", e.message()
            ));
        }
    }

    // Step 3: resolve upstream tip (try HEAD, main, master)
    let upstream_tip = ["refs/remotes/upstream/HEAD",
                        "refs/remotes/upstream/main",
                        "refs/remotes/upstream/master"]
        .iter()
        .find_map(|refname| {
            git_repo.revparse_single(refname)
                .ok()
                .and_then(|obj| obj.peel_to_commit().ok())
        });

    if let Some(upstream_commit) = upstream_tip {
        // Step 4: find merge base and hide upstream commits from walk
        match git_repo.merge_base(tip_oid, upstream_commit.id()) {
            Ok(base_oid) => {
                if let Err(e) = revwalk.hide(base_oid) {
                    summary.errors.push(format!(
                        "Warning: failed to hide upstream commits: {}", e.message()
                    ));
                }
            }
            Err(e) => {
                summary.errors.push(format!(
                    "Warning: no merge base with upstream ({}): {}",
                    upstream_url, e.message()
                ));
            }
        }
    } else {
        summary.errors.push(format!(
            "Warning: could not resolve upstream tip for '{}'", upstream_url
        ));
    }
}
```

Place this block AFTER `revwalk.push(tip_oid)` and
`revwalk.set_sorting(...)` but BEFORE the commit walk loop.

### 4c. Incremental skip — skip already-indexed commits

Inside the revwalk loop, immediately after `let sha = oid.to_string();`:

```rust
// Skip commits already in the store
match store.commit_exists(repo.repo_id, &sha) {
    Ok(true) => {
        summary.commits_skipped += 1;
        continue;
    }
    Ok(false) => { /* proceed */ }
    Err(e) => {
        summary.errors.push(format!(
            "Warning: failed to check commit existence for {}: {}", sha, e
        ));
        // Proceed to index it anyway (conservative)
    }
}
```

### 4d. Author filter — skip non-matching commits

After the `let commit = Commit { ... }` construction block, before
`store.upsert_commit(&commit)`:

```rust
if let Some(ref filter_email) = repo.author_filter {
    if !commit.author_email.eq_ignore_ascii_case(filter_email) {
        summary.commits_skipped += 1;
        continue;
    }
}
```

**Order matters:** incremental skip (4c) runs BEFORE author filter (4d). A
commit that is already indexed is skipped regardless of author. A commit not
yet indexed is checked against the author filter before indexing.

## 5. Tests to Write

Add tests to the existing `#[cfg(test)]` module in `crates/ingest/src/lib.rs`.

**Before writing tests**, verify that `MockStore.commit_exists` is a real
implementation (not `unimplemented!()`). If it is `unimplemented!()`, add a
real implementation:

```rust
fn commit_exists(&self, _repo_id: i64, sha: &str) -> Result<bool> {
    Ok(self.commits.lock().unwrap().iter().any(|c| c.sha == sha))
}
```

1. `test_author_filter_skips_non_matching` — create a temp git repo with two
   commits by different authors (`alice@example.com` and `bob@example.com`).
   Construct a `Repo` with `author_filter: Some("alice@example.com".into())`.
   Run `sync_repo`. Verify: `commits_indexed == 1`, `commits_skipped == 1`,
   and only Alice's commit is in `store.commits`.

2. `test_exclude_prefixes_from_repo` — create a temp git repo with one commit
   that touches both `src/main.rs` and `generated/api.rs`. Set
   `repo.exclude_prefixes = vec!["generated/".into()]`. Run `sync_repo`. Verify
   `generated/api.rs` does NOT appear in `store.files` but `src/main.rs` does.

3. `test_incremental_skip_already_indexed` — create a temp git repo with 2
   commits. Run `sync_repo` once (MockStore starts empty, so `commit_exists`
   returns false for both, indexes 2). Run `sync_repo` again on the same
   MockStore (now `commit_exists` returns true for both). Verify second run:
   `commits_indexed == 0`, `commits_skipped == 2`.

**Helper update:** For test 1, you'll need to create two commits with different
authors. In git2, use `git2::Signature::new("Alice", "alice@example.com", &time)`.

## 6. Verification Gate

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-c
cargo build --workspace
cargo clippy --workspace -- -D warnings
cargo test -p commitmux-ingest
```

All ingest tests must pass (3 existing + 3 new = 6 total).

## 7. Constraints

- Do NOT mutate the `config` parameter. Always create `effective_config`.
- Author filter comparison must use `eq_ignore_ascii_case`.
- Fork-of failures are ALL non-fatal. Every error is pushed to
  `summary.errors` and the sync continues.
- The incremental skip does NOT skip the ingest state update at the end.
- If `store.commit_exists` errors, index the commit anyway (conservative —
  better to re-index than to silently miss).
- Do not add `git2` features or change `Cargo.toml`.

## 8. Report

```bash
cd /Users/dayna.blackwell/code/commitmux/.claude/worktrees/wave1-agent-c
git add crates/ingest/src/walker.rs
# If you updated MockStore.commit_exists in lib.rs:
# git add crates/ingest/src/lib.rs
git commit -m "wave1-agent-c: author filter, exclude persistence, fork-of merge-base, incremental skip"
```

Append your completion report to
`/Users/dayna.blackwell/code/commitmux/docs/IMPL-feature-wave-1.md`
under `### Agent C — Completion Report`.

Include:
- What you implemented
- Test results (pass/fail, count)
- Deviations from spec
- Interface contract changes
- Out-of-scope files touched (list each with justification)
