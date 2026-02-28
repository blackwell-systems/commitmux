use rusqlite::{params, Connection, OptionalExtension};
use std::sync::MutexGuard;

use commitmux_types::{
    Commit, CommitDetail, CommitFile, CommitFileDetail, CommitPatch, IngestState,
    PatchResult, Repo, RepoInput, RepoStats, Result, SearchOpts, SearchResult, Store, TouchOpts,
    TouchResult,
};

use crate::SqliteStore;

// ── Helpers ───────────────────────────────────────────────────────────────

fn row_to_repo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Repo> {
    Ok(Repo {
        repo_id: row.get(0)?,
        name: row.get(1)?,
        local_path: std::path::PathBuf::from(row.get::<_, String>(2)?),
        remote_url: row.get(3)?,
        default_branch: row.get(4)?,
    })
}

// ── impl Store ────────────────────────────────────────────────────────────

impl Store for SqliteStore {
    // ── Repo management ───────────────────────────────────────────────────

    fn add_repo(&self, input: &RepoInput) -> Result<Repo> {
        let conn: MutexGuard<'_, Connection> = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO repos (name, local_path, remote_url, default_branch) VALUES (?1, ?2, ?3, ?4)",
            params![
                input.name,
                input.local_path.to_string_lossy().as_ref(),
                input.remote_url,
                input.default_branch,
            ],
        )?;
        let repo_id = conn.last_insert_rowid();
        Ok(Repo {
            repo_id,
            name: input.name.clone(),
            local_path: input.local_path.clone(),
            remote_url: input.remote_url.clone(),
            default_branch: input.default_branch.clone(),
        })
    }

    fn list_repos(&self) -> Result<Vec<Repo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT repo_id, name, local_path, remote_url, default_branch FROM repos ORDER BY repo_id",
        )?;
        let repos: rusqlite::Result<Vec<Repo>> =
            stmt.query_map([], row_to_repo)?.collect();
        Ok(repos?)
    }

    fn get_repo_by_name(&self, name: &str) -> Result<Option<Repo>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                "SELECT repo_id, name, local_path, remote_url, default_branch FROM repos WHERE name = ?1",
                params![name],
                row_to_repo,
            )
            .optional()?;
        Ok(result)
    }

    // ── Ingest writes ─────────────────────────────────────────────────────

    fn upsert_commit(&self, commit: &Commit) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // First, determine the rowid of any existing row so we can delete from FTS.
        let existing_rowid: Option<i64> = conn
            .query_row(
                "SELECT rowid FROM commits WHERE repo_id = ?1 AND sha = ?2",
                params![commit.repo_id, commit.sha],
                |row| row.get(0),
            )
            .optional()?;

        // If replacing, remove old FTS entry first.
        if let Some(old_rowid) = existing_rowid {
            let old_subject: String = conn.query_row(
                "SELECT COALESCE(subject,'') FROM commits WHERE rowid = ?1",
                params![old_rowid],
                |r| r.get(0),
            )?;
            let old_body: Option<String> = conn.query_row(
                "SELECT body FROM commits WHERE rowid = ?1",
                params![old_rowid],
                |r| r.get(0),
            )?;
            let old_preview: Option<String> = conn.query_row(
                "SELECT patch_preview FROM commits WHERE rowid = ?1",
                params![old_rowid],
                |r| r.get(0),
            )?;
            conn.execute(
                "INSERT INTO commits_fts(commits_fts, rowid, subject, body, patch_preview)
                 VALUES('delete', ?1, ?2, ?3, ?4)",
                params![old_rowid, old_subject, old_body, old_preview],
            )?;
        }

        conn.execute(
            "INSERT OR REPLACE INTO commits
                (repo_id, sha, author_name, author_email, committer_name, committer_email,
                 author_time, commit_time, subject, body, parent_count, patch_preview)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'')",
            params![
                commit.repo_id,
                commit.sha,
                commit.author_name,
                commit.author_email,
                commit.committer_name,
                commit.committer_email,
                commit.author_time,
                commit.commit_time,
                commit.subject,
                commit.body,
                commit.parent_count,
            ],
        )?;

        let new_rowid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO commits_fts(rowid, subject, body, patch_preview)
             VALUES (?1, ?2, ?3, '')",
            params![new_rowid, commit.subject, commit.body],
        )?;

        Ok(())
    }

    fn upsert_commit_files(&self, files: &[CommitFile]) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        if files.is_empty() {
            return Ok(());
        }

        // Determine the unique (repo_id, sha) pairs and delete existing entries.
        // Using a simple approach: delete by sha for each unique pair.
        if let Some(first) = files.first() {
            conn.execute(
                "DELETE FROM commit_files WHERE repo_id = ?1 AND sha = ?2",
                params![first.repo_id, first.sha],
            )?;
        }

        for file in files {
            conn.execute(
                "INSERT INTO commit_files (repo_id, sha, path, status, old_path)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    file.repo_id,
                    file.sha,
                    file.path,
                    file.status.as_str(),
                    file.old_path,
                ],
            )?;
        }
        Ok(())
    }

    fn upsert_patch(&self, patch: &CommitPatch) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Compress the blob with zstd level 3.
        let compressed =
            zstd::encode_all(patch.patch_blob.as_slice(), 3).map_err(commitmux_types::CommitmuxError::Io)?;

        conn.execute(
            "INSERT OR REPLACE INTO commit_patches (repo_id, sha, patch_blob)
             VALUES (?1, ?2, ?3)",
            params![patch.repo_id, patch.sha, compressed],
        )?;

        // Update patch_preview in commits (first 500 chars of preview).
        let preview: String = patch.patch_preview.chars().take(500).collect();

        // Get the current rowid and existing FTS data so we can update FTS.
        let row: Option<(i64, String, Option<String>)> = conn
            .query_row(
                "SELECT rowid, COALESCE(subject,''), body FROM commits WHERE repo_id = ?1 AND sha = ?2",
                params![patch.repo_id, patch.sha],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        if let Some((rowid, subject, body)) = row {
            // Fetch old patch_preview for FTS delete.
            let old_preview: Option<String> = conn
                .query_row(
                    "SELECT patch_preview FROM commits WHERE rowid = ?1",
                    params![rowid],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();

            // Remove old FTS entry.
            conn.execute(
                "INSERT INTO commits_fts(commits_fts, rowid, subject, body, patch_preview)
                 VALUES('delete', ?1, ?2, ?3, ?4)",
                params![rowid, subject, body, old_preview],
            )?;

            // Update commits table.
            conn.execute(
                "UPDATE commits SET patch_preview = ?1 WHERE rowid = ?2",
                params![preview, rowid],
            )?;

            // Re-insert fresh FTS entry.
            conn.execute(
                "INSERT INTO commits_fts(rowid, subject, body, patch_preview)
                 VALUES (?1, ?2, ?3, ?4)",
                params![rowid, subject, body, preview],
            )?;
        }

        Ok(())
    }

    fn get_ingest_state(&self, repo_id: i64) -> Result<Option<IngestState>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                "SELECT repo_id, last_synced_at, last_synced_sha, last_error
                 FROM ingest_state WHERE repo_id = ?1",
                params![repo_id],
                |row| {
                    Ok(IngestState {
                        repo_id: row.get(0)?,
                        last_synced_at: row.get(1)?,
                        last_synced_sha: row.get(2)?,
                        last_error: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    fn update_ingest_state(&self, state: &IngestState) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO ingest_state
                (repo_id, last_synced_at, last_synced_sha, last_error)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                state.repo_id,
                state.last_synced_at,
                state.last_synced_sha,
                state.last_error,
            ],
        )?;
        Ok(())
    }

    // ── MCP queries ───────────────────────────────────────────────────────

    fn search(&self, query: &str, opts: &SearchOpts) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock().unwrap();

        let limit = opts.limit.unwrap_or(50) as i64;

        // Build dynamic WHERE clauses.
        let mut extra_conditions = String::new();
        let mut bind_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        // The FTS MATCH param is always first positional.
        // We'll build the full SQL with numbered params.
        let mut param_idx = 2usize; // ?1 is the FTS query

        if let Some(since) = opts.since {
            extra_conditions.push_str(&format!(" AND c.author_time >= ?{}", param_idx));
            bind_vals.push(Box::new(since));
            param_idx += 1;
        }

        // repos filter
        let repo_placeholders: Option<String> = opts.repos.as_ref().map(|repos| {
            let ph: String = repos
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", param_idx + i))
                .collect::<Vec<_>>()
                .join(",");
            ph
        });
        if let Some(ref repos) = opts.repos {
            if let Some(ref ph) = repo_placeholders {
                extra_conditions.push_str(&format!(" AND r.name IN ({})", ph));
                for r in repos {
                    bind_vals.push(Box::new(r.clone()));
                    param_idx += 1;
                }
            }
        }

        let sql = format!(
            "SELECT c.repo_id, c.sha, c.subject, c.author_name, c.author_time, c.patch_preview, r.name
             FROM commits_fts
             JOIN commits c ON c.rowid = commits_fts.rowid
             JOIN repos r ON r.repo_id = c.repo_id
             WHERE commits_fts MATCH ?1{}
             ORDER BY c.author_time DESC
             LIMIT ?{}", extra_conditions, param_idx
        );

        bind_vals.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;

        // Build all params: query first, then dynamic ones.
        let all_params: Vec<&dyn rusqlite::types::ToSql> = std::iter::once(&query as &dyn rusqlite::types::ToSql)
            .chain(bind_vals.iter().map(|b| b.as_ref()))
            .collect();

        let rows: rusqlite::Result<Vec<(i64, String, String, String, i64, String, String)>> =
            stmt.query_map(all_params.as_slice(), |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    row.get(6)?,
                ))
            })?
            .collect();
        let rows = rows?;

        let mut results = Vec::with_capacity(rows.len());
        for (repo_id, sha, subject, author, date, patch_preview, repo_name) in rows {
            // paths filter: if set, check commit_files
            if let Some(ref path_filters) = opts.paths {
                let mut matched = false;
                for pf in path_filters {
                    let like_pat = format!("%{}%", pf);
                    let count: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM commit_files WHERE repo_id = ?1 AND sha = ?2 AND path LIKE ?3",
                        params![repo_id, sha, like_pat],
                        |r| r.get(0),
                    )?;
                    if count > 0 {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    continue;
                }
            }

            // Gather matched paths.
            let mut path_stmt = conn.prepare(
                "SELECT path FROM commit_files WHERE repo_id = ?1 AND sha = ?2 ORDER BY path",
            )?;
            let matched_paths: rusqlite::Result<Vec<String>> =
                path_stmt.query_map(params![repo_id, sha], |r| r.get(0))?.collect();
            let matched_paths = matched_paths?;

            let patch_excerpt: String = patch_preview.chars().take(300).collect();

            results.push(SearchResult {
                repo: repo_name,
                sha,
                subject,
                author,
                date,
                matched_paths,
                patch_excerpt,
            });
        }

        Ok(results)
    }

    fn touches(&self, path_glob: &str, opts: &TouchOpts) -> Result<Vec<TouchResult>> {
        let conn = self.conn.lock().unwrap();

        let limit = opts.limit.unwrap_or(50) as i64;
        let like_pat = format!("%{}%", path_glob);

        let mut extra_conditions = String::new();
        let mut bind_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut param_idx = 3usize; // ?1 = like_pat, ?2 = limit (added at end)

        if let Some(since) = opts.since {
            extra_conditions.push_str(&format!(" AND c.author_time >= ?{}", param_idx));
            bind_vals.push(Box::new(since));
            param_idx += 1;
        }

        if let Some(ref repos) = opts.repos {
            let ph: String = repos
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", param_idx + i))
                .collect::<Vec<_>>()
                .join(",");
            extra_conditions.push_str(&format!(" AND r.name IN ({})", ph));
            for r in repos {
                bind_vals.push(Box::new(r.clone()));
                param_idx += 1;
            }
        }

        let sql = format!(
            "SELECT cf.path, cf.status, c.sha, c.subject, c.author_time, r.name
             FROM commit_files cf
             JOIN commits c ON cf.repo_id = c.repo_id AND cf.sha = c.sha
             JOIN repos r ON r.repo_id = c.repo_id
             WHERE cf.path LIKE ?1{}
             ORDER BY c.author_time DESC
             LIMIT ?{}",
            extra_conditions, param_idx
        );

        bind_vals.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;

        let all_params: Vec<&dyn rusqlite::types::ToSql> =
            std::iter::once(&like_pat as &dyn rusqlite::types::ToSql)
                .chain(bind_vals.iter().map(|b| b.as_ref()))
                .collect();

        let rows: rusqlite::Result<Vec<TouchResult>> =
            stmt.query_map(all_params.as_slice(), |row| {
                Ok(TouchResult {
                    path: row.get(0)?,
                    status: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    sha: row.get(2)?,
                    subject: row.get(3)?,
                    date: row.get(4)?,
                    repo: row.get(5)?,
                })
            })?
            .collect();

        Ok(rows?)
    }

    fn get_commit(&self, repo_name: &str, sha: &str) -> Result<Option<CommitDetail>> {
        let conn = self.conn.lock().unwrap();

        let detail: Option<(i64, String, String, Option<String>, String, i64)> = conn
            .query_row(
                "SELECT c.repo_id, c.sha, c.subject, c.body, c.author_name, c.author_time
                 FROM commits c
                 JOIN repos r ON r.repo_id = c.repo_id
                 WHERE r.name = ?1 AND c.sha = ?2",
                params![repo_name, sha],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .optional()?;

        match detail {
            None => Ok(None),
            Some((repo_id, commit_sha, subject, body, author, date)) => {
                let mut fstmt = conn.prepare(
                    "SELECT path, status, old_path FROM commit_files
                     WHERE repo_id = ?1 AND sha = ?2 ORDER BY path",
                )?;
                let files: rusqlite::Result<Vec<CommitFileDetail>> =
                    fstmt.query_map(params![repo_id, commit_sha], |row| {
                        Ok(CommitFileDetail {
                            path: row.get(0)?,
                            status: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                            old_path: row.get(2)?,
                        })
                    })?
                    .collect();
                let changed_files = files?;

                Ok(Some(CommitDetail {
                    repo: repo_name.to_string(),
                    sha: commit_sha,
                    subject,
                    body,
                    author,
                    date,
                    changed_files,
                }))
            }
        }
    }

    fn get_patch(&self, repo_name: &str, sha: &str, max_bytes: Option<usize>) -> Result<Option<PatchResult>> {
        let conn = self.conn.lock().unwrap();

        let blob: Option<Vec<u8>> = conn
            .query_row(
                "SELECT cp.patch_blob
                 FROM commit_patches cp
                 JOIN repos r ON r.repo_id = cp.repo_id
                 WHERE r.name = ?1 AND cp.sha = ?2",
                params![repo_name, sha],
                |row| row.get(0),
            )
            .optional()?;

        match blob {
            None => Ok(None),
            Some(compressed) => {
                let decompressed = zstd::decode_all(compressed.as_slice())
                    .map_err(commitmux_types::CommitmuxError::Io)?;

                let mut patch_text =
                    String::from_utf8_lossy(&decompressed).into_owned();

                if let Some(max) = max_bytes {
                    if patch_text.len() > max {
                        // Truncate at a valid UTF-8 boundary.
                        let truncated: String = patch_text.chars().take(max).collect();
                        patch_text = truncated;
                    }
                }

                Ok(Some(PatchResult {
                    repo: repo_name.to_string(),
                    sha: sha.to_string(),
                    patch_text,
                }))
            }
        }
    }

    fn repo_stats(&self, repo_id: i64) -> Result<RepoStats> {
        let conn = self.conn.lock().unwrap();

        let repo_name: String = conn.query_row(
            "SELECT name FROM repos WHERE repo_id = ?1",
            params![repo_id],
            |row| row.get(0),
        )?;

        let commit_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM commits WHERE repo_id = ?1",
            params![repo_id],
            |row| row.get(0),
        )?;

        let ingest: Option<(i64, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT last_synced_at, last_synced_sha, last_error FROM ingest_state WHERE repo_id = ?1",
                params![repo_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let (last_synced_at, last_synced_sha, last_error) = match ingest {
            Some((ts, sha, err)) => (Some(ts), sha, err),
            None => (None, None, None),
        };

        Ok(RepoStats {
            repo_name,
            commit_count: commit_count as usize,
            last_synced_at,
            last_synced_sha,
            last_error,
        })
    }
}
