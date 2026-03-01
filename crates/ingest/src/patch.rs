use commitmux_types::{CommitFile, CommitmuxError, FileStatus, IgnoreConfig, Result};

fn is_ignored(path: &str, config: &IgnoreConfig) -> bool {
    config
        .path_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix.as_str()))
}

pub fn get_commit_files(
    repo: &git2::Repository,
    commit: &git2::Commit,
    repo_id: i64,
    config: &IgnoreConfig,
) -> Result<Vec<CommitFile>> {
    let parent_tree = if commit.parent_count() > 0 {
        Some(
            commit
                .parent(0)
                .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?
                .tree()
                .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?,
        )
    } else {
        None
    };

    let commit_tree = commit
        .tree()
        .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

    let sha = commit.id().to_string();
    let mut files = Vec::new();

    for delta in diff.deltas() {
        // Skip binary files (check via new_file or old_file flags)
        if delta.new_file().is_binary() || delta.old_file().is_binary() {
            continue;
        }

        let status = delta.status();

        // Determine the primary path (new file path, or old for deletes)
        let path = match status {
            git2::Delta::Deleted => delta.old_file().path(),
            _ => delta.new_file().path().or_else(|| delta.old_file().path()),
        };

        let path_str = match path.and_then(|p| p.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Skip ignored paths
        if is_ignored(&path_str, config) {
            continue;
        }

        // Get old_path for renames
        let old_path = if status == git2::Delta::Renamed {
            delta
                .old_file()
                .path()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string())
        } else {
            None
        };

        let file_status = match status {
            git2::Delta::Added | git2::Delta::Untracked => FileStatus::Added,
            git2::Delta::Modified | git2::Delta::Typechange => FileStatus::Modified,
            git2::Delta::Deleted => FileStatus::Deleted,
            git2::Delta::Renamed => FileStatus::Renamed,
            git2::Delta::Copied => FileStatus::Copied,
            _ => FileStatus::Unknown,
        };

        files.push(CommitFile {
            repo_id,
            sha: sha.clone(),
            path: path_str,
            status: file_status,
            old_path,
        });
    }

    Ok(files)
}

pub fn get_patch_text(
    repo: &git2::Repository,
    commit: &git2::Commit,
    config: &IgnoreConfig,
) -> Result<Option<String>> {
    let parent_tree = if commit.parent_count() > 0 {
        Some(
            commit
                .parent(0)
                .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?
                .tree()
                .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?,
        )
    } else {
        None
    };

    let commit_tree = commit
        .tree()
        .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

    let max_bytes = config.max_patch_bytes;
    let mut patch_text = String::new();
    let mut truncated = false;

    diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
        // Skip binary deltas
        if delta.new_file().is_binary() || delta.old_file().is_binary() {
            return true;
        }

        // Check path against ignore config
        let path_ignored = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .map(|s| is_ignored(s, config))
            .unwrap_or(false);

        if path_ignored {
            return true;
        }

        if truncated {
            return true;
        }

        let content = match std::str::from_utf8(line.content()) {
            Ok(s) => s,
            Err(_) => return true,
        };

        if patch_text.len() + content.len() > max_bytes {
            truncated = true;
            return true;
        }

        patch_text.push_str(content);
        true
    })
    .map_err(|e| CommitmuxError::Ingest(e.message().to_string()))?;

    if patch_text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(patch_text))
    }
}
