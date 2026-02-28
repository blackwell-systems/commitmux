use std::path::PathBuf;
use thiserror::Error;

// ── Error ─────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CommitmuxError {
    #[cfg(feature = "rusqlite-errors")]
    #[error("store error: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("ingest error: {0}")]
    Ingest(String),
    #[cfg(feature = "git2-errors")]
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, CommitmuxError>;

// ── Domain types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Repo {
    pub repo_id: i64,
    pub name: String,
    pub local_path: PathBuf,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
    // NEW:
    pub fork_of: Option<String>,
    pub author_filter: Option<String>,
    pub exclude_prefixes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RepoInput {
    pub name: String,
    pub local_path: PathBuf,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
    // NEW:
    pub fork_of: Option<String>,
    pub author_filter: Option<String>,
    pub exclude_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RepoUpdate {
    pub fork_of: Option<Option<String>>,
    pub author_filter: Option<Option<String>>,
    pub exclude_prefixes: Option<Vec<String>>,
    pub default_branch: Option<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoListEntry {
    pub name: String,
    pub commit_count: usize,
    pub last_synced_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub repo_id: i64,
    pub sha: String,
    pub author_name: String,
    pub author_email: String,
    pub committer_name: String,
    pub committer_email: String,
    pub author_time: i64,
    pub commit_time: i64,
    pub subject: String,
    pub body: Option<String>,
    pub parent_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Unknown,
}

impl FileStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileStatus::Added => "A",
            FileStatus::Modified => "M",
            FileStatus::Deleted => "D",
            FileStatus::Renamed => "R",
            FileStatus::Copied => "C",
            FileStatus::Unknown => "?",
        }
    }
}

impl std::fmt::Display for FileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct CommitFile {
    pub repo_id: i64,
    pub sha: String,
    pub path: String,
    pub status: FileStatus,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommitPatch {
    pub repo_id: i64,
    pub sha: String,
    pub patch_blob: Vec<u8>,
    pub patch_preview: String,
}

#[derive(Debug, Clone)]
pub struct IngestState {
    pub repo_id: i64,
    pub last_synced_at: i64,
    pub last_synced_sha: Option<String>,
    pub last_error: Option<String>,
}

// ── Query option types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SearchOpts {
    pub since: Option<i64>,
    pub repos: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct TouchOpts {
    pub since: Option<i64>,
    pub repos: Option<Vec<String>>,
    pub limit: Option<usize>,
}

// ── MCP response types ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub repo: String,
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub date: i64,
    pub matched_paths: Vec<String>,
    pub patch_excerpt: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TouchResult {
    pub repo: String,
    pub sha: String,
    pub subject: String,
    pub date: i64,
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitDetail {
    pub repo: String,
    pub sha: String,
    pub subject: String,
    pub body: Option<String>,
    pub author: String,
    pub date: String,   // ISO 8601 UTC: "YYYY-MM-DDTHH:MM:SSZ"
    pub changed_files: Vec<CommitFileDetail>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitFileDetail {
    pub path: String,
    pub status: String,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PatchResult {
    pub repo: String,
    pub sha: String,
    pub patch_text: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SyncSummary {
    pub commits_indexed: usize,
    pub commits_already_indexed: usize,
    pub commits_filtered: usize,
    pub errors: Vec<String>,
}

// ── Config ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IgnoreConfig {
    pub path_prefixes: Vec<String>,
    pub max_patch_bytes: usize,
}

impl Default for IgnoreConfig {
    fn default() -> Self {
        Self {
            path_prefixes: vec![
                "node_modules/".into(),
                "vendor/".into(),
                "dist/".into(),
                ".git/".into(),
            ],
            max_patch_bytes: 1_048_576,
        }
    }
}

// ── Admin types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepoStats {
    pub repo_name: String,
    pub commit_count: usize,
    pub last_synced_at: Option<i64>,
    pub last_synced_sha: Option<String>,
    pub last_error: Option<String>,
}

// ── Core traits ───────────────────────────────────────────────────────────

pub trait Store: Send + Sync {
    // Repo management
    fn add_repo(&self, input: &RepoInput) -> Result<Repo>;
    fn list_repos(&self) -> Result<Vec<Repo>>;
    fn get_repo_by_name(&self, name: &str) -> Result<Option<Repo>>;
    fn remove_repo(&self, name: &str) -> Result<()>;
    fn update_repo(&self, repo_id: i64, update: &RepoUpdate) -> Result<Repo>;
    fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>>;

    // Ingest writes
    fn upsert_commit(&self, commit: &Commit) -> Result<()>;
    fn upsert_commit_files(&self, files: &[CommitFile]) -> Result<()>;
    fn upsert_patch(&self, patch: &CommitPatch) -> Result<()>;
    fn get_ingest_state(&self, repo_id: i64) -> Result<Option<IngestState>>;
    fn update_ingest_state(&self, state: &IngestState) -> Result<()>;
    fn commit_exists(&self, repo_id: i64, sha: &str) -> Result<bool>;

    // MCP queries
    fn search(&self, query: &str, opts: &SearchOpts) -> Result<Vec<SearchResult>>;
    fn touches(&self, path_glob: &str, opts: &TouchOpts) -> Result<Vec<TouchResult>>;
    /// sha_prefix: exact SHA or a unique hex prefix (>=4 chars recommended)
    fn get_commit(&self, repo_name: &str, sha_prefix: &str) -> Result<Option<CommitDetail>>;
    fn get_patch(&self, repo_name: &str, sha: &str, max_bytes: Option<usize>) -> Result<Option<PatchResult>>;

    // Admin
    fn repo_stats(&self, repo_id: i64) -> Result<RepoStats>;
    fn count_commits_for_repo(&self, repo_id: i64) -> Result<usize>;
}

pub trait Ingester: Send + Sync {
    fn sync_repo(&self, repo: &Repo, store: &dyn Store, config: &IgnoreConfig) -> Result<SyncSummary>;
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_smoke_construct_all_types() {
        let repo = Repo {
            repo_id: 1,
            name: "myrepo".into(),
            local_path: PathBuf::from("/tmp/myrepo"),
            remote_url: Some("https://github.com/user/myrepo".into()),
            default_branch: Some("main".into()),
            fork_of: None,
            author_filter: None,
            exclude_prefixes: vec![],
        };
        assert_eq!(repo.name, "myrepo");

        let commit = Commit {
            repo_id: 1,
            sha: "abc123".into(),
            author_name: "Alice".into(),
            author_email: "alice@example.com".into(),
            committer_name: "Alice".into(),
            committer_email: "alice@example.com".into(),
            author_time: 1700000000,
            commit_time: 1700000000,
            subject: "Initial commit".into(),
            body: None,
            parent_count: 0,
        };
        assert_eq!(commit.sha, "abc123");

        let file = CommitFile {
            repo_id: 1,
            sha: "abc123".into(),
            path: "src/main.rs".into(),
            status: FileStatus::Added,
            old_path: None,
        };
        assert_eq!(file.status, FileStatus::Added);
        assert_eq!(file.status.as_str(), "A");

        let patch = CommitPatch {
            repo_id: 1,
            sha: "abc123".into(),
            patch_blob: vec![0u8; 10],
            patch_preview: "--- a/src/main.rs\n+++ b/src/main.rs".into(),
        };
        assert!(!patch.patch_blob.is_empty());

        let state = IngestState {
            repo_id: 1,
            last_synced_at: 1700000000,
            last_synced_sha: Some("abc123".into()),
            last_error: None,
        };
        assert!(state.last_error.is_none());

        let opts = SearchOpts::default();
        assert!(opts.since.is_none());
        assert!(opts.limit.is_none());

        let config = IgnoreConfig::default();
        assert!(config.path_prefixes.contains(&"node_modules/".to_string()));
        assert_eq!(config.max_patch_bytes, 1_048_576);
    }

    #[test]
    fn test_file_status_display() {
        assert_eq!(FileStatus::Added.as_str(), "A");
        assert_eq!(FileStatus::Modified.as_str(), "M");
        assert_eq!(FileStatus::Deleted.as_str(), "D");
        assert_eq!(FileStatus::Renamed.as_str(), "R");
    }

    #[test]
    fn test_repo_new_fields_default() {
        let repo = Repo {
            repo_id: 42,
            name: "my-repo".into(),
            local_path: PathBuf::from("/tmp/my-repo"),
            remote_url: None,
            default_branch: None,
            fork_of: None,
            author_filter: None,
            exclude_prefixes: vec![],
        };
        assert!(repo.fork_of.is_none());
        assert!(repo.author_filter.is_none());
        assert!(repo.exclude_prefixes.is_empty());
    }

    #[test]
    fn test_repo_update_type() {
        let default_update = RepoUpdate::default();
        assert!(default_update.fork_of.is_none());
        assert!(default_update.author_filter.is_none());
        assert!(default_update.exclude_prefixes.is_none());
        assert!(default_update.default_branch.is_none());

        let update_with_fork = RepoUpdate {
            fork_of: Some(Some("https://github.com/foo/bar".into())),
            ..RepoUpdate::default()
        };
        assert_eq!(
            update_with_fork.fork_of,
            Some(Some("https://github.com/foo/bar".into()))
        );
    }

    #[test]
    fn test_repo_list_entry_serializes() {
        let entry = RepoListEntry {
            name: "my-repo".into(),
            commit_count: 42,
            last_synced_at: Some(1700000000),
        };
        let json_str = serde_json::to_string(&entry).expect("serialize");
        let deserialized: RepoListEntry = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(deserialized.name, "my-repo");
        assert_eq!(deserialized.commit_count, 42);
        assert_eq!(deserialized.last_synced_at, Some(1700000000));
    }
}
