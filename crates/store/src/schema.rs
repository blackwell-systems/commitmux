/// All DDL for CommitMux SQLite schema.
/// Run in order; all statements are idempotent (IF NOT EXISTS).
pub const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS repos (
    repo_id       INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL UNIQUE,
    local_path    TEXT NOT NULL,
    remote_url    TEXT,
    default_branch TEXT
);

CREATE TABLE IF NOT EXISTS commits (
    repo_id         INTEGER NOT NULL,
    sha             TEXT NOT NULL,
    author_name     TEXT,
    author_email    TEXT,
    committer_name  TEXT,
    committer_email TEXT,
    author_time     INTEGER,
    commit_time     INTEGER,
    subject         TEXT,
    body            TEXT,
    parent_count    INTEGER,
    patch_preview   TEXT,
    PRIMARY KEY (repo_id, sha)
);

CREATE TABLE IF NOT EXISTS commit_files (
    repo_id  INTEGER NOT NULL,
    sha      TEXT NOT NULL,
    path     TEXT NOT NULL,
    status   TEXT,
    old_path TEXT
);

CREATE INDEX IF NOT EXISTS idx_commit_files_repo_sha
    ON commit_files (repo_id, sha);

CREATE INDEX IF NOT EXISTS idx_commit_files_path
    ON commit_files (path);

CREATE TABLE IF NOT EXISTS commit_patches (
    repo_id    INTEGER NOT NULL,
    sha        TEXT NOT NULL,
    patch_blob BLOB,
    PRIMARY KEY (repo_id, sha)
);

CREATE TABLE IF NOT EXISTS ingest_state (
    repo_id         INTEGER PRIMARY KEY,
    last_synced_at  INTEGER,
    last_synced_sha TEXT,
    last_error      TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS commits_fts
    USING fts5(subject, body, patch_preview, content='commits', content_rowid='rowid');
"#;
