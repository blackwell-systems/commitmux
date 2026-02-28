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

    /// Run all pragmas and schema DDL.
    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(schema::SCHEMA_SQL)?;
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_types::{Commit, CommitPatch, RepoInput, SearchOpts, Store};
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
