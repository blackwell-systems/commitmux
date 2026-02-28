use commitmux_types::{
    Commit, CommitPatch, CommitmuxError, IgnoreConfig, IngestState, IngestSummary, Repo, Result,
    Store,
};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::patch;

pub struct Git2Ingester;

impl Git2Ingester {
    pub fn new() -> Self {
        Git2Ingester
    }
}

impl Default for Git2Ingester {
    fn default() -> Self {
        Git2Ingester::new()
    }
}

impl commitmux_types::Ingester for Git2Ingester {
    fn sync_repo(
        &self,
        repo: &Repo,
        store: &dyn Store,
        config: &IgnoreConfig,
    ) -> Result<IngestSummary> {
        let mut summary = IngestSummary {
            repo_name: repo.name.clone(),
            commits_indexed: 0,
            commits_already_indexed: 0,
            commits_filtered: 0,
            errors: Vec::new(),
        };

        // Open the git repository
        let git_repo = git2::Repository::open(&repo.local_path)
            .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

        // If this is a managed clone (has a remote_url), fetch all remotes to keep it up to date
        if repo.remote_url.is_some() {
            match git_repo.remotes() {
                Ok(remotes) => {
                    for remote_name in remotes.iter().flatten() {
                        match git_repo.find_remote(remote_name) {
                            Ok(mut remote) => {
                                let mut callbacks = git2::RemoteCallbacks::new();
                                callbacks.credentials(|_url, username, _allowed| {
                                    git2::Cred::ssh_key_from_agent(username.unwrap_or("git"))
                                });
                                let mut fo = git2::FetchOptions::new();
                                fo.remote_callbacks(callbacks);
                                if let Err(e) = remote.fetch::<&str>(&[], Some(&mut fo), None) {
                                    summary.errors.push(format!(
                                        "Warning: failed to fetch remote '{}': {}",
                                        remote_name,
                                        e.message()
                                    ));
                                }
                            }
                            Err(e) => {
                                summary.errors.push(format!(
                                    "Warning: failed to open remote '{}': {}",
                                    remote_name,
                                    e.message()
                                ));
                            }
                        }
                    }
                }
                Err(e) => {
                    summary.errors.push(format!(
                        "Warning: failed to list remotes: {}",
                        e.message()
                    ));
                }
            }
        }

        // Construct effective_config: merge persisted exclude_prefixes from Repo
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

        // Resolve the tip commit
        let tip_commit = resolve_tip(&git_repo, repo)?;
        let tip_oid = tip_commit.id();

        // Set up revwalk: topological, oldest first (REVERSE)
        let mut revwalk = git_repo
            .revwalk()
            .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

        revwalk
            .push(tip_oid)
            .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

        revwalk
            .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)
            .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

        // Fork-of upstream exclusion: hide commits reachable from upstream
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

        // Walk commits
        for oid_result in revwalk {
            let oid = match oid_result {
                Ok(oid) => oid,
                Err(e) => {
                    summary
                        .errors
                        .push(format!("Failed to get oid in revwalk: {}", e.message()));
                    continue;
                }
            };

            let git_commit = match git_repo.find_commit(oid) {
                Ok(c) => c,
                Err(e) => {
                    summary.errors.push(format!(
                        "Failed to find commit {}: {}",
                        oid,
                        e.message()
                    ));
                    continue;
                }
            };

            let sha = oid.to_string();

            // Skip commits already in the store (incremental skip)
            match store.commit_exists(repo.repo_id, &sha) {
                Ok(true) => {
                    summary.commits_already_indexed += 1;
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

            // Extract commit metadata
            let author = git_commit.author();
            let committer = git_commit.committer();

            let message = git_commit.message().unwrap_or("").to_string();
            let mut lines = message.lines();
            let subject = lines.next().unwrap_or("").trim().to_string();
            let body_lines: Vec<&str> = lines
                .skip_while(|l| l.trim().is_empty())
                .collect();
            let body = if body_lines.is_empty() {
                None
            } else {
                Some(body_lines.join("\n"))
            };

            let commit = Commit {
                repo_id: repo.repo_id,
                sha: sha.clone(),
                author_name: author.name().unwrap_or("").to_string(),
                author_email: author.email().unwrap_or("").to_string(),
                committer_name: committer.name().unwrap_or("").to_string(),
                committer_email: committer.email().unwrap_or("").to_string(),
                author_time: author.when().seconds(),
                commit_time: git_commit.time().seconds(),
                subject,
                body,
                parent_count: git_commit.parent_count() as u32,
            };

            // Author filter: skip commits not matching the configured author email
            if let Some(ref filter_email) = repo.author_filter {
                if !commit.author_email.eq_ignore_ascii_case(filter_email) {
                    summary.commits_filtered += 1;
                    continue;
                }
            }

            // Upsert commit
            if let Err(e) = store.upsert_commit(&commit) {
                summary
                    .errors
                    .push(format!("Failed to upsert commit {}: {}", sha, e));
                continue;
            }

            // Get changed files
            match patch::get_commit_files(&git_repo, &git_commit, repo.repo_id, &effective_config) {
                Ok(files) => {
                    if let Err(e) = store.upsert_commit_files(&files) {
                        summary.errors.push(format!(
                            "Failed to upsert files for commit {}: {}",
                            sha, e
                        ));
                    }
                }
                Err(e) => {
                    summary.errors.push(format!(
                        "Failed to get files for commit {}: {}",
                        sha, e
                    ));
                }
            }

            // Get and store patch text
            match patch::get_patch_text(&git_repo, &git_commit, &effective_config) {
                Ok(Some(text)) => {
                    let preview_len = text.floor_char_boundary(500);
                    let patch_preview = text[..preview_len].to_string();

                    match zstd::encode_all(text.as_bytes(), 3) {
                        Ok(patch_blob) => {
                            let cp = CommitPatch {
                                repo_id: repo.repo_id,
                                sha: sha.clone(),
                                patch_blob,
                                patch_preview,
                            };
                            if let Err(e) = store.upsert_patch(&cp) {
                                summary.errors.push(format!(
                                    "Failed to upsert patch for commit {}: {}",
                                    sha, e
                                ));
                            }
                        }
                        Err(e) => {
                            summary.errors.push(format!(
                                "Failed to compress patch for commit {}: {}",
                                sha, e
                            ));
                        }
                    }
                }
                Ok(None) => {
                    // No patch text (e.g., all binary) — that's fine
                }
                Err(e) => {
                    summary.errors.push(format!(
                        "Failed to get patch text for commit {}: {}",
                        sha, e
                    ));
                }
            }

            summary.commits_indexed += 1;
        }

        // Update ingest state
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let ingest_state = IngestState {
            repo_id: repo.repo_id,
            last_synced_at: now,
            last_synced_sha: Some(tip_oid.to_string()),
            last_error: summary.errors.last().cloned(),
        };

        // Best-effort — don't fail the whole sync if state update fails
        if let Err(e) = store.update_ingest_state(&ingest_state) {
            summary
                .errors
                .push(format!("Failed to update ingest state: {}", e));
        }

        Ok(summary)
    }
}

fn resolve_tip<'repo>(
    git_repo: &'repo git2::Repository,
    repo: &Repo,
) -> Result<git2::Commit<'repo>> {
    // Try the configured default branch first
    if let Some(ref branch_name) = repo.default_branch {
        let refname = format!("refs/heads/{}", branch_name);
        if let Ok(obj) = git_repo.revparse_single(&refname) {
            if let Ok(commit) = obj.peel_to_commit() {
                return Ok(commit);
            }
        }
        // Also try the branch name directly (may be a remote ref or short name)
        if let Ok(obj) = git_repo.revparse_single(branch_name) {
            if let Ok(commit) = obj.peel_to_commit() {
                return Ok(commit);
            }
        }
    }

    // Fall back to HEAD
    git_repo
        .head()
        .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?
        .peel_to_commit()
        .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))
}
