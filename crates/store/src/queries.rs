use rusqlite::{params, Connection, OptionalExtension};
use std::sync::MutexGuard;

use commitmux_types::{
    Commit, CommitDetail, CommitFile, CommitFileDetail, CommitPatch, CommitmuxError, EmbedCommit,
    IngestState, PatchResult, Repo, RepoInput, RepoListEntry, RepoStats, RepoUpdate, Result,
    SearchOpts, SearchResult, SemanticSearchOpts, Store, TouchOpts, TouchResult,
};

use crate::SqliteStore;

// ── Helpers ───────────────────────────────────────────────────────────────

fn parse_exclude_prefixes(s: Option<String>) -> Vec<String> {
    match s {
        None => vec![],
        Some(j) => serde_json::from_str::<Vec<String>>(&j).unwrap_or_default(),
    }
}

fn format_iso_date(ts: i64) -> String {
    // Manual UTC formatting without chrono dependency — Gregorian calendar arithmetic.
    // Returns "YYYY-MM-DDTHH:MM:SSZ"
    let secs = if ts < 0 { 0u64 } else { ts as u64 };
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Gregorian calendar calculation (same algorithm as format_timestamp in src/main.rs)
    let z = days_since_epoch as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

fn row_to_repo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Repo> {
    Ok(Repo {
        repo_id: row.get(0)?,
        name: row.get(1)?,
        local_path: std::path::PathBuf::from(row.get::<_, String>(2)?),
        remote_url: row.get(3)?,
        default_branch: row.get(4)?,
        fork_of: row.get(5)?,
        author_filter: row.get(6)?,
        exclude_prefixes: parse_exclude_prefixes(row.get(7)?),
        embed_enabled: row.get::<_, i64>(8).unwrap_or(0) != 0,
    })
}

// ── impl Store ────────────────────────────────────────────────────────────

impl Store for SqliteStore {
    // ── Repo management ───────────────────────────────────────────────────

    fn add_repo(&self, input: &RepoInput) -> Result<Repo> {
        let conn: MutexGuard<'_, Connection> = self.conn.lock().unwrap();
        let exclude_json = serde_json::to_string(&input.exclude_prefixes)
            .unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO repos (name, local_path, remote_url, default_branch, fork_of, author_filter, exclude_prefixes, embed_enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                input.name,
                input.local_path.to_string_lossy().as_ref(),
                input.remote_url,
                input.default_branch,
                input.fork_of,
                input.author_filter,
                exclude_json,
                input.embed_enabled as i64,
            ],
        )?;
        let repo_id = conn.last_insert_rowid();
        Ok(Repo {
            repo_id,
            name: input.name.clone(),
            local_path: input.local_path.clone(),
            remote_url: input.remote_url.clone(),
            default_branch: input.default_branch.clone(),
            fork_of: input.fork_of.clone(),
            author_filter: input.author_filter.clone(),
            exclude_prefixes: input.exclude_prefixes.clone(),
            embed_enabled: input.embed_enabled,
        })
    }

    fn list_repos(&self) -> Result<Vec<Repo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT repo_id, name, local_path, remote_url, default_branch, fork_of, author_filter, exclude_prefixes, embed_enabled FROM repos ORDER BY repo_id",
        )?;
        let repos: rusqlite::Result<Vec<Repo>> =
            stmt.query_map([], row_to_repo)?.collect();
        Ok(repos?)
    }

    fn get_repo_by_name(&self, name: &str) -> Result<Option<Repo>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                "SELECT repo_id, name, local_path, remote_url, default_branch, fork_of, author_filter, exclude_prefixes, embed_enabled FROM repos WHERE name = ?1",
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

        #[allow(clippy::type_complexity)]
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
        let mut param_idx = 2usize; // ?1 = like_pat, limit appended at param_idx end

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

    fn get_commit(&self, repo_name: &str, sha_prefix: &str) -> Result<Option<CommitDetail>> {
        let conn = self.conn.lock().unwrap();

        let detail: Option<(i64, String, String, Option<String>, String, i64)> = conn
            .query_row(
                "SELECT c.repo_id, c.sha, c.subject, c.body, c.author_name, c.author_time
                 FROM commits c
                 JOIN repos r ON r.repo_id = c.repo_id
                 WHERE r.name = ?1 AND c.sha LIKE ?2 || '%'
                 ORDER BY c.author_time DESC",
                params![repo_name, sha_prefix],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .optional()?;

        match detail {
            None => Ok(None),
            Some((repo_id, commit_sha, subject, body, author, raw_date)) => {
                let date = format_iso_date(raw_date);
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

    fn remove_repo(&self, name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // 1. Look up repo_id
        let repo_id: Option<i64> = conn.query_row(
            "SELECT repo_id FROM repos WHERE name = ?1",
            params![name],
            |row| row.get(0),
        ).optional()?;

        let repo_id = repo_id.ok_or_else(|| CommitmuxError::NotFound(
            format!("repo '{}' not found", name)
        ))?;

        // 2. Delete patches
        conn.execute("DELETE FROM commit_patches WHERE repo_id = ?1", params![repo_id])?;

        // 3. Delete files
        conn.execute("DELETE FROM commit_files WHERE repo_id = ?1", params![repo_id])?;

        // 4. Delete ingest state
        conn.execute("DELETE FROM ingest_state WHERE repo_id = ?1", params![repo_id])?;

        // 5. Delete commits (drop FTS entries first via rebuild after delete)
        conn.execute("DELETE FROM commits WHERE repo_id = ?1", params![repo_id])?;

        // 6. Rebuild FTS to reflect deleted commits
        conn.execute("INSERT INTO commits_fts(commits_fts) VALUES('rebuild')", [])?;

        // 7. Delete repo
        conn.execute("DELETE FROM repos WHERE repo_id = ?1", params![repo_id])?;

        Ok(())
    }

    fn commit_exists(&self, repo_id: i64, sha: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM commits WHERE repo_id = ?1 AND sha = ?2",
            params![repo_id, sha],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    fn update_repo(&self, repo_id: i64, update: &RepoUpdate) -> Result<Repo> {
        let conn = self.conn.lock().unwrap();

        let mut set_clauses: Vec<String> = Vec::new();
        let mut bind_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1usize;

        if let Some(ref v) = update.fork_of {
            set_clauses.push(format!("fork_of = ?{}", idx));
            bind_vals.push(match v {
                Some(s) => Box::new(s.clone()),
                None => Box::new(rusqlite::types::Null),
            });
            idx += 1;
        }
        if let Some(ref v) = update.author_filter {
            set_clauses.push(format!("author_filter = ?{}", idx));
            bind_vals.push(match v {
                Some(s) => Box::new(s.clone()),
                None => Box::new(rusqlite::types::Null),
            });
            idx += 1;
        }
        if let Some(ref v) = update.exclude_prefixes {
            let json = serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string());
            set_clauses.push(format!("exclude_prefixes = ?{}", idx));
            bind_vals.push(Box::new(json));
            idx += 1;
        }
        if let Some(ref v) = update.default_branch {
            set_clauses.push(format!("default_branch = ?{}", idx));
            bind_vals.push(match v {
                Some(s) => Box::new(s.clone()),
                None => Box::new(rusqlite::types::Null),
            });
            idx += 1;
        }
        if let Some(v) = update.embed_enabled {
            set_clauses.push(format!("embed_enabled = ?{}", idx));
            bind_vals.push(Box::new(v as i64));
            idx += 1;
        }

        if !set_clauses.is_empty() {
            let sql = format!(
                "UPDATE repos SET {} WHERE repo_id = ?{}",
                set_clauses.join(", "),
                idx
            );
            bind_vals.push(Box::new(repo_id));
            let params: Vec<&dyn rusqlite::types::ToSql> =
                bind_vals.iter().map(|b| b.as_ref()).collect();
            conn.execute(&sql, params.as_slice())?;
        }

        // Re-fetch
        let repo = conn.query_row(
            "SELECT repo_id, name, local_path, remote_url, default_branch, fork_of, author_filter, exclude_prefixes, embed_enabled FROM repos WHERE repo_id = ?1",
            params![repo_id],
            row_to_repo,
        )?;

        Ok(repo)
    }

    fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT r.name, COUNT(c.sha), i.last_synced_at
             FROM repos r
             LEFT JOIN commits c ON c.repo_id = r.repo_id
             LEFT JOIN ingest_state i ON i.repo_id = r.repo_id
             GROUP BY r.repo_id
             ORDER BY r.repo_id"
        )?;
        let rows: rusqlite::Result<Vec<RepoListEntry>> = stmt.query_map([], |row| {
            Ok(RepoListEntry {
                name: row.get(0)?,
                commit_count: row.get::<_, i64>(1)? as usize,
                last_synced_at: row.get(2)?,
            })
        })?.collect();
        Ok(rows?)
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

    fn count_commits_for_repo(&self, repo_id: i64) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM commits WHERE repo_id = ?1",
            params![repo_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    // ── Embedding support ─────────────────────────────────────────────────

    fn get_config(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT value FROM config WHERE key = ?1",
            params![key],
            |row| row.get(0),
        ).optional()?;
        Ok(result)
    }

    fn set_config(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn get_commits_without_embeddings(&self, repo_id: i64, limit: usize) -> Result<Vec<EmbedCommit>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT c.sha, c.subject, c.body, c.patch_preview,
                    c.author_name, c.author_time, r.name
             FROM commits c
             JOIN repos r ON r.repo_id = c.repo_id
             LEFT JOIN commit_embed_map m ON m.repo_id = c.repo_id AND m.sha = c.sha
             WHERE c.repo_id = ?1
               AND m.embed_id IS NULL
             ORDER BY c.author_time DESC
             LIMIT ?2",
        )?;
        let result: rusqlite::Result<Vec<EmbedCommit>> = stmt
            .query_map(params![repo_id, limit as i64], |row| {
                Ok(EmbedCommit {
                    repo_id,
                    sha: row.get(0)?,
                    subject: row.get(1)?,
                    body: row.get(2)?,
                    files_changed: vec![],  // empty for perf — patch_preview captures diff content
                    patch_preview: row.get(3)?,
                    author_name: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    author_time: row.get(5)?,
                    repo_name: row.get(6)?,
                })
            })?
            .collect();
        Ok(result?)
    }

    #[allow(clippy::too_many_arguments)]
    fn store_embedding(
        &self,
        repo_id: i64,
        sha: &str,
        subject: &str,
        author_name: &str,
        repo_name: &str,
        author_time: i64,
        patch_preview: Option<&str>,
        embedding: &[f32],
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Insert into key map (idempotent)
        conn.execute(
            "INSERT OR IGNORE INTO commit_embed_map (repo_id, sha) VALUES (?1, ?2)",
            params![repo_id, sha],
        )?;
        let embed_id: i64 = conn.query_row(
            "SELECT embed_id FROM commit_embed_map WHERE repo_id = ?1 AND sha = ?2",
            params![repo_id, sha],
            |row| row.get(0),
        )?;
        // Convert Vec<f32> to bytes for sqlite-vec
        let embedding_bytes: Vec<u8> = embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        // sqlite-vec vec0 tables don't support INSERT OR REPLACE; delete first for idempotency.
        conn.execute(
            "DELETE FROM commit_embeddings WHERE embed_id = ?1",
            params![embed_id],
        )?;
        conn.execute(
            "INSERT INTO commit_embeddings
                 (embed_id, embedding, sha, subject, repo_name, author_name, author_time, patch_preview)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![embed_id, embedding_bytes, sha, subject, repo_name, author_name, author_time, patch_preview],
        )?;
        Ok(())
    }

    fn search_semantic(&self, embedding: &[f32], opts: &SemanticSearchOpts) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock().unwrap();
        let limit = opts.limit.unwrap_or(10);

        let embedding_bytes: Vec<u8> = embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        let repos_json = opts.repos.as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_else(|_| "[]".into()))
            .unwrap_or_else(|| "[]".into());
        let since = opts.since.unwrap_or(0);

        let sql =
            "SELECT ce.repo_name, ce.sha, ce.subject, ce.author_name, ce.author_time,
                    ce.patch_preview, distance
             FROM commit_embeddings ce
             WHERE ce.embedding MATCH ?1
               AND k = ?2
               AND ('' = ?3 OR ce.repo_name IN (SELECT value FROM json_each(?3)))
               AND (?4 = 0 OR ce.author_time >= ?4)
             ORDER BY distance";

        let mut stmt = conn.prepare(sql)?;
        let results: rusqlite::Result<Vec<SearchResult>> = stmt
            .query_map(params![embedding_bytes, limit as i64, repos_json, since], |row| {
                Ok(SearchResult {
                    repo: row.get(0)?,
                    sha: row.get(1)?,
                    subject: row.get(2)?,
                    author: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    date: row.get(4)?,
                    matched_paths: vec![],
                    patch_excerpt: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                })
            })?
            .collect();
        Ok(results?)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_types::{Commit, RepoInput, Store};
    use crate::SqliteStore;
    use std::path::PathBuf;

    fn make_store() -> SqliteStore {
        SqliteStore::open_in_memory().expect("open in-memory store")
    }

    fn make_repo_input(name: &str) -> RepoInput {
        RepoInput {
            name: name.to_string(),
            local_path: PathBuf::from(format!("/tmp/{}", name)),
            remote_url: None,
            default_branch: Some("main".to_string()),
            fork_of: None,
            author_filter: None,
            exclude_prefixes: vec![],
            embed_enabled: false,
        }
    }

    fn make_commit(repo_id: i64, sha: &str, subject: &str, author_time: i64) -> Commit {
        Commit {
            repo_id,
            sha: sha.to_string(),
            author_name: "Test Author".to_string(),
            author_email: "test@example.com".to_string(),
            committer_name: "Test Author".to_string(),
            committer_email: "test@example.com".to_string(),
            author_time,
            commit_time: author_time,
            subject: subject.to_string(),
            body: None,
            parent_count: 0,
        }
    }

    #[test]
    fn test_count_commits_for_repo() {
        let store = make_store();
        let repo = store.add_repo(&make_repo_input("countrepo")).expect("add repo");

        store.upsert_commit(&make_commit(repo.repo_id, "sha0000000000001", "commit 1", 1700000000)).expect("upsert 1");
        store.upsert_commit(&make_commit(repo.repo_id, "sha0000000000002", "commit 2", 1700000001)).expect("upsert 2");
        store.upsert_commit(&make_commit(repo.repo_id, "sha0000000000003", "commit 3", 1700000002)).expect("upsert 3");

        let count = store.count_commits_for_repo(repo.repo_id).expect("count_commits_for_repo");
        assert_eq!(count, 3, "expected 3 commits for repo");
    }

    #[test]
    fn test_get_commit_date_is_iso8601() {
        let store = make_store();
        let repo = store.add_repo(&make_repo_input("daterepo")).expect("add repo");

        // UNIX epoch 0 = 1970-01-01T00:00:00Z
        store.upsert_commit(&make_commit(repo.repo_id, "epoch000000000001", "epoch commit", 0)).expect("upsert epoch commit");

        let detail = store
            .get_commit("daterepo", "epoch000000000001")
            .expect("get_commit")
            .expect("should be Some");

        assert_eq!(
            detail.date, "1970-01-01T00:00:00Z",
            "expected ISO 8601 UTC date string for epoch 0"
        );
    }

    #[test]
    fn test_count_commits_after_remove() {
        let store = make_store();
        let repo = store.add_repo(&make_repo_input("removecountrepo")).expect("add repo");

        store.upsert_commit(&make_commit(repo.repo_id, "rc0000000000001a", "rc commit 1", 1700000000)).expect("upsert 1");
        store.upsert_commit(&make_commit(repo.repo_id, "rc0000000000002b", "rc commit 2", 1700000001)).expect("upsert 2");

        // Verify commits are present before removal
        let before = store.count_commits_for_repo(repo.repo_id).expect("count before remove");
        assert_eq!(before, 2, "expected 2 commits before remove");

        // Remove the repo
        store.remove_repo("removecountrepo").expect("remove repo");

        // After removal, count_commits_for_repo on the old repo_id should return 0
        let after = store.count_commits_for_repo(repo.repo_id).expect("count after remove");
        assert_eq!(after, 0, "expected 0 commits after repo removal");
    }

    #[test]
    fn test_format_iso_date_epoch() {
        assert_eq!(format_iso_date(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_format_iso_date_known_timestamp() {
        // 2026-02-28T15:34:55Z = 1772234095
        // Verify a known timestamp: 2000-01-01T00:00:00Z = 946684800
        assert_eq!(format_iso_date(946684800), "2000-01-01T00:00:00Z");
    }
}
