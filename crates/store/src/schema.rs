/// All DDL for CommitMux SQLite schema.
/// Run in order; all statements are idempotent (IF NOT EXISTS).
pub const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS repos (
    repo_id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name             TEXT NOT NULL UNIQUE,
    local_path       TEXT NOT NULL,
    remote_url       TEXT,
    default_branch   TEXT,
    fork_of          TEXT,
    author_filter    TEXT,
    exclude_prefixes TEXT
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

CREATE TABLE IF NOT EXISTS config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS commit_embed_map (
    embed_id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo_id  INTEGER NOT NULL,
    sha      TEXT NOT NULL,
    UNIQUE(repo_id, sha)
);

CREATE VIRTUAL TABLE IF NOT EXISTS commit_embeddings USING vec0(
    embed_id       INTEGER PRIMARY KEY,
    embedding      FLOAT[768],
    +sha           TEXT,
    +subject       TEXT,
    +repo_name     TEXT,
    +author_name   TEXT,
    +author_time   INTEGER,
    +patch_preview TEXT
);

"#;

/// Migration statements for new `repos` columns.
/// Each is attempted individually; "duplicate column name" errors are ignored
/// so that migrations are idempotent on databases that already have the column.
pub const REPO_MIGRATIONS: &[&str] = &[
    "ALTER TABLE repos ADD COLUMN fork_of TEXT",
    "ALTER TABLE repos ADD COLUMN author_filter TEXT",
    "ALTER TABLE repos ADD COLUMN exclude_prefixes TEXT",
];

/// Migration statements for embedding support columns.
/// Each is attempted individually; "duplicate column name" errors are ignored
/// so that migrations are idempotent on databases that already have the column.
pub const EMBED_MIGRATIONS: &[&str] = &[
    "ALTER TABLE repos ADD COLUMN embed_enabled INTEGER NOT NULL DEFAULT 0",
];
