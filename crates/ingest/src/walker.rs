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
            commits_skipped: 0,
            errors: Vec::new(),
        };

        // Open the git repository
        let git_repo = git2::Repository::open(&repo.local_path)
            .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

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

        // Walk commits
        for oid_result in revwalk {
            let oid = match oid_result {
                Ok(oid) => oid,
                Err(e) => {
                    summary
                        .errors
                        .push(format!("Failed to get oid in revwalk: {}", e.message()));
                    summary.commits_skipped += 1;
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
                    summary.commits_skipped += 1;
                    continue;
                }
            };

            let sha = oid.to_string();

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

            // Upsert commit
            if let Err(e) = store.upsert_commit(&commit) {
                summary
                    .errors
                    .push(format!("Failed to upsert commit {}: {}", sha, e));
                summary.commits_skipped += 1;
                continue;
            }

            // Get changed files
            match patch::get_commit_files(&git_repo, &git_commit, repo.repo_id, config) {
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
            match patch::get_patch_text(&git_repo, &git_commit, config) {
                Ok(Some(text)) => {
                    let preview_len = text.len().min(500);
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
