mod schema;
mod queries;

use std::sync::Mutex;
use commitmux_types::Result;

/// SQLite-backed implementation of the [`commitmux_types::Store`] trait.
pub struct SqliteStore {
    pub(crate) conn: Mutex<rusqlite::Connection>,
}

impl SqliteStore {
    /// Open a persistent on-disk database at `path`.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init()?;
        Ok(store)
    }

    /// Open an in-memory database (primarily for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init()?;
        Ok(store)
    }

    /// Run all pragmas, schema DDL, and column migrations.
    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(schema::SCHEMA_SQL)?;
        // Apply column migrations one at a time; ignore "duplicate column name" errors
        // so that init() remains idempotent on databases that already have the columns.
        for &sql in schema::REPO_MIGRATIONS {
            match conn.execute_batch(sql) {
                Ok(()) => {}
                Err(rusqlite::Error::SqliteFailure(_, Some(ref msg)))
                    if msg.contains("duplicate column name") => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_types::{Commit, CommitFile, CommitPatch, FileStatus, RepoInput, RepoUpdate, SearchOpts, Store};
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
        }
    }

    fn make_commit(repo_id: i64, sha: &str, subject: &str) -> Commit {
        Commit {
            repo_id,
            sha: sha.to_string(),
            author_name: "Alice".to_string(),
            author_email: "alice@example.com".to_string(),
            committer_name: "Alice".to_string(),
            committer_email: "alice@example.com".to_string(),
            author_time: 1700000000,
            commit_time: 1700000000,
            subject: subject.to_string(),
            body: None,
            parent_count: 0,
        }
    }

    #[test]
    fn test_add_repo_and_list() {
        let store = make_store();

        store.add_repo(&make_repo_input("alpha")).expect("add alpha");
        store.add_repo(&make_repo_input("beta")).expect("add beta");

        let repos = store.list_repos().expect("list repos");
        assert_eq!(repos.len(), 2);

        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn test_upsert_commit_idempotent() {
        let store = make_store();

        let repo = store.add_repo(&make_repo_input("myrepo")).expect("add repo");
        let commit = make_commit(repo.repo_id, "deadbeef", "First commit");

        store.upsert_commit(&commit).expect("first upsert");
        store.upsert_commit(&commit).expect("second upsert");

        // Query count directly.
        let conn = store.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM commits WHERE repo_id = ?1 AND sha = ?2",
                rusqlite::params![repo.repo_id, "deadbeef"],
                |r| r.get(0),
            )
            .expect("count query");
        assert_eq!(count, 1, "expected exactly 1 row after idempotent upsert");
    }

    #[test]
    fn test_search_fts() {
        let store = make_store();

        let repo = store.add_repo(&make_repo_input("searchrepo")).expect("add repo");
        let commit = make_commit(repo.repo_id, "cafebabe", "xyzzy_unique_token initial work");
        store.upsert_commit(&commit).expect("upsert commit");

        // Upsert a patch so patch_preview is indexed.
        // patch_blob contains raw bytes; upsert_patch compresses internally.
        let raw_patch = b"diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1 +1 @@\n-old\n+new xyzzy_unique_token\n";
        let patch = CommitPatch {
            repo_id: repo.repo_id,
            sha: "cafebabe".to_string(),
            patch_blob: raw_patch.to_vec(),
            patch_preview: "xyzzy_unique_token preview".to_string(),
        };
        store.upsert_patch(&patch).expect("upsert patch");

        let opts = SearchOpts::default();
        let results = store.search("xyzzy_unique_token", &opts).expect("search");
        assert!(!results.is_empty(), "expected at least one search result");
        assert_eq!(results[0].sha, "cafebabe");
    }

    #[test]
    fn test_remove_repo_deletes_all() {
        let store = make_store();

        // Add a repo and upsert a commit with files and a patch
        let repo = store.add_repo(&make_repo_input("rmrepo")).expect("add repo");
        let commit = make_commit(repo.repo_id, "deadbeef01234567", "remove test commit");
        store.upsert_commit(&commit).expect("upsert commit");

        let files = vec![CommitFile {
            repo_id: repo.repo_id,
            sha: "deadbeef01234567".to_string(),
            path: "src/main.rs".to_string(),
            status: FileStatus::Added,
            old_path: None,
        }];
        store.upsert_commit_files(&files).expect("upsert files");

        let raw_patch = b"diff --git a/src/main.rs b/src/main.rs\n";
        let patch = CommitPatch {
            repo_id: repo.repo_id,
            sha: "deadbeef01234567".to_string(),
            patch_blob: raw_patch.to_vec(),
            patch_preview: "preview".to_string(),
        };
        store.upsert_patch(&patch).expect("upsert patch");

        // Remove the repo
        store.remove_repo("rmrepo").expect("remove_repo");

        // list_repos should be empty
        let repos = store.list_repos().expect("list repos");
        assert!(repos.is_empty(), "expected no repos after remove");

        // get_commit should return None
        let detail = store.get_commit("rmrepo", "deadbeef01234567").expect("get_commit after remove");
        assert!(detail.is_none(), "expected no commit after remove");

        // FTS search should return empty
        let results = store.search("remove test commit", &SearchOpts::default()).expect("search");
        assert!(results.is_empty(), "expected no FTS results after remove");
    }

    #[test]
    fn test_remove_repo_not_found() {
        let store = make_store();
        let result = store.remove_repo("nonexistent");
        assert!(result.is_err(), "expected error for nonexistent repo");
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.to_lowercase().contains("not found"),
            "expected 'not found' in error, got: {}",
            err_str
        );
    }

    #[test]
    fn test_commit_exists() {
        let store = make_store();
        let repo = store.add_repo(&make_repo_input("existsrepo")).expect("add repo");
        let commit = make_commit(repo.repo_id, "abc123def456", "test commit");
        store.upsert_commit(&commit).expect("upsert commit");

        let exists = store.commit_exists(repo.repo_id, "abc123def456").expect("commit_exists");
        assert!(exists, "expected commit to exist");

        let missing = store.commit_exists(repo.repo_id, "unknown").expect("commit_exists unknown");
        assert!(!missing, "expected unknown sha to not exist");
    }

    #[test]
    fn test_update_repo_author_filter() {
        let store = make_store();
        let repo = store.add_repo(&make_repo_input("updaterepo")).expect("add repo");

        // Set author_filter
        let update = RepoUpdate {
            author_filter: Some(Some("alice@example.com".into())),
            ..RepoUpdate::default()
        };
        let updated = store.update_repo(repo.repo_id, &update).expect("update_repo set");
        assert_eq!(
            updated.author_filter,
            Some("alice@example.com".to_string()),
            "expected author_filter to be set"
        );

        // Clear author_filter
        let clear = RepoUpdate {
            author_filter: Some(None),
            ..RepoUpdate::default()
        };
        let cleared = store.update_repo(repo.repo_id, &clear).expect("update_repo clear");
        assert!(cleared.author_filter.is_none(), "expected author_filter to be cleared");
    }

    #[test]
    fn test_list_repos_with_stats() {
        let store = make_store();
        let repo1 = store.add_repo(&make_repo_input("statsrepo1")).expect("add repo1");
        let repo2 = store.add_repo(&make_repo_input("statsrepo2")).expect("add repo2");

        // 1 commit for repo1
        store.upsert_commit(&make_commit(repo1.repo_id, "aaa111", "commit for r1")).expect("upsert");

        // 2 commits for repo2
        store.upsert_commit(&make_commit(repo2.repo_id, "bbb222", "commit1 for r2")).expect("upsert");
        store.upsert_commit(&make_commit(repo2.repo_id, "ccc333", "commit2 for r2")).expect("upsert");

        let entries = store.list_repos_with_stats().expect("list_repos_with_stats");
        assert_eq!(entries.len(), 2, "expected 2 entries");

        let e1 = entries.iter().find(|e| e.name == "statsrepo1").expect("statsrepo1");
        assert_eq!(e1.commit_count, 1, "expected 1 commit for statsrepo1");

        let e2 = entries.iter().find(|e| e.name == "statsrepo2").expect("statsrepo2");
        assert_eq!(e2.commit_count, 2, "expected 2 commits for statsrepo2");
    }

    #[test]
    fn test_get_commit_short_sha() {
        let store = make_store();
        let repo = store.add_repo(&make_repo_input("shortsharepo")).expect("add repo");
        // SHA starts with deadbe
        let commit = make_commit(repo.repo_id, "deadbe1234567890", "short sha test");
        store.upsert_commit(&commit).expect("upsert commit");

        let result = store.get_commit("shortsharepo", "deadbe").expect("get_commit prefix");
        assert!(result.is_some(), "expected commit via prefix");
        let detail = result.unwrap();
        assert_eq!(detail.sha, "deadbe1234567890");
        assert_eq!(detail.subject, "short sha test");
    }

    #[test]
    fn test_exclude_prefixes_roundtrip() {
        let store = make_store();
        let input = RepoInput {
            name: "prefixrepo".to_string(),
            local_path: PathBuf::from("/tmp/prefixrepo"),
            remote_url: None,
            default_branch: Some("main".to_string()),
            fork_of: None,
            author_filter: None,
            exclude_prefixes: vec!["dist/".into(), "vendor/".into()],
        };
        store.add_repo(&input).expect("add repo");

        let repo = store.get_repo_by_name("prefixrepo").expect("get_repo_by_name").expect("should exist");
        assert_eq!(repo.exclude_prefixes, vec!["dist/".to_string(), "vendor/".to_string()]);
    }

    #[test]
    fn test_get_patch_roundtrip() {
        let store = make_store();

        let repo = store.add_repo(&make_repo_input("patchrepo")).expect("add repo");
        let commit = make_commit(repo.repo_id, "1234abcd", "patch roundtrip test");
        store.upsert_commit(&commit).expect("upsert commit");

        let original_text = "diff --git a/hello.rs b/hello.rs\n--- a/hello.rs\n+++ b/hello.rs\n@@ -1 +1 @@\n-fn main(){}\n+fn main() { println!(\"hello\"); }\n";

        // patch_blob contains raw (uncompressed) bytes; upsert_patch compresses internally.
        let patch = CommitPatch {
            repo_id: repo.repo_id,
            sha: "1234abcd".to_string(),
            patch_blob: original_text.as_bytes().to_vec(),
            patch_preview: original_text.chars().take(500).collect(),
        };
        store.upsert_patch(&patch).expect("upsert patch");

        let result = store
            .get_patch("patchrepo", "1234abcd", None)
            .expect("get_patch")
            .expect("patch should be Some");

        assert_eq!(result.patch_text, original_text);
        assert_eq!(result.repo, "patchrepo");
        assert_eq!(result.sha, "1234abcd");
    }
}
