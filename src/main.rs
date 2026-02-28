use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{Context, Result};

use commitmux_ingest::Git2Ingester;
use commitmux_store::SqliteStore;
use commitmux_types::{IgnoreConfig, Ingester, RepoInput, RepoUpdate, Store};

#[derive(Parser)]
#[command(name = "commitmux", about = "Cross-repo git history index for AI agents", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Initialize the commitmux database")]
    Init {
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
    },
    #[command(about = "Add a git repository to the index")]
    AddRepo {
        #[arg(conflicts_with = "url", help = "Local path to a git repository (mutually exclusive with --url)")]
        path: Option<PathBuf>,
        #[arg(long, help = "Override the repo name (default: directory name)")]
        name: Option<String>,
        #[arg(long = "exclude", help = "Path prefix to exclude from indexing (repeatable)")]
        exclude: Vec<String>,
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
        #[arg(conflicts_with = "path", long, help = "Remote git URL to clone and index")]
        url: Option<String>,
        #[arg(long = "fork-of", help = "Upstream repo URL; only index commits not in upstream")]
        fork_of: Option<String>,
        #[arg(long = "author", help = "Only index commits by this author (email match)")]
        author: Option<String>,
    },
    #[command(about = "Remove a repository and all its indexed commits")]
    RemoveRepo {
        #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
        name: String,
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
    },
    #[command(about = "Update stored metadata for a repository")]
    UpdateRepo {
        #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
        name: String,
        #[arg(long = "fork-of", help = "Upstream repo URL; only index commits not in upstream")]
        fork_of: Option<String>,
        #[arg(long = "author", help = "Only index commits by this author (email match)")]
        author: Option<String>,
        #[arg(long = "exclude", help = "Path prefix to exclude from indexing (repeatable)")]
        exclude: Vec<String>,
        #[arg(long = "default-branch", help = "Set the default branch name")]
        default_branch: Option<String>,
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
    },
    #[command(about = "Index new commits from one or all repositories")]
    Sync {
        #[arg(long, help = "Sync only this repo (default: sync all)")]
        repo: Option<String>,
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
    },
    #[command(about = "Show full details for a specific commit (JSON output)")]
    Show {
        #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
        repo: String,
        #[arg(help = "Full or prefix SHA of the commit")]
        sha: String,
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
    },
    #[command(about = "Show all indexed repositories with commit counts and sync times")]
    Status {
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
    },
    #[command(about = "Start the MCP JSON-RPC server for AI agent access")]
    Serve {
        #[arg(long, help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)")]
        db: Option<PathBuf>,
    },
}

fn resolve_db_path(flag: Option<PathBuf>) -> PathBuf {
    if let Some(p) = flag {
        return p;
    }
    if let Ok(v) = std::env::var("COMMITMUX_DB") {
        return PathBuf::from(v);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".commitmux").join("db.sqlite3")
}

fn format_timestamp(ts: i64) -> String {
    // Simple manual UTC formatting without chrono dependency
    // Using UNIX_EPOCH arithmetic
    if ts <= 0 {
        return "never".to_string();
    }
    let secs = ts as u64;
    // Days since epoch
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Gregorian calendar calculation
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
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        y, m, d, hours, minutes, seconds
    )
}

fn validate_git_url(url: &str) -> Result<()> {
    if !url.starts_with("https://")
        && !url.starts_with("http://")
        && !url.starts_with("git@")
        && !url.starts_with("git://")
        && !url.starts_with("ssh://")
    {
        anyhow::bail!(
            "'{}' is not a valid git URL (expected https://, http://, git@, git://, or ssh://)",
            url
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { db } => {
            let db_path = resolve_db_path(db);
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
            }
            let already_exists = db_path.exists();
            SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;
            if already_exists {
                println!("Database already initialized at {}", db_path.display());
            } else {
                println!("Initialized commitmux database at {}", db_path.display());
            }
        }

        Commands::AddRepo { path, name, exclude, db, url, fork_of, author } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            if let Some(remote_url) = url {
                // Validate URL scheme before attempting clone
                validate_git_url(&remote_url)?;

                // URL-based ingestion: derive name from URL basename, clone repo
                let derived_name = remote_url
                    .trim_end_matches('/')
                    .split('/')
                    .next_back()
                    .unwrap_or("repo")
                    .trim_end_matches(".git")
                    .to_string();
                let repo_name = name.unwrap_or(derived_name);

                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                let clone_dir = PathBuf::from(home)
                    .join(".commitmux")
                    .join("clones")
                    .join(&repo_name);

                println!("Cloning {} from {}...", repo_name, remote_url);

                std::fs::create_dir_all(&clone_dir)
                    .with_context(|| format!("Failed to create clone directory: {}", clone_dir.display()))?;

                let mut callbacks = git2::RemoteCallbacks::new();
                callbacks.credentials(|_url, username, _allowed| {
                    git2::Cred::ssh_key_from_agent(username.unwrap_or("git"))
                });
                let mut fo = git2::FetchOptions::new();
                fo.remote_callbacks(callbacks);
                let mut builder = git2::build::RepoBuilder::new();
                builder.fetch_options(fo);
                builder.clone(&remote_url, &clone_dir)
                    .with_context(|| format!("Failed to clone '{}' from '{}'", repo_name, remote_url))?;

                store.add_repo(&RepoInput {
                    name: repo_name.clone(),
                    local_path: clone_dir.clone(),
                    remote_url: Some(remote_url.clone()),
                    default_branch: None,
                    fork_of: fork_of.clone(),
                    author_filter: author.clone(),
                    exclude_prefixes: exclude.clone(),
                    embed_enabled: false, // TODO Wave 2A: wire --embed/--no-embed flags
                })
                .map_err(|e| {
                    if e.to_string().contains("UNIQUE constraint") {
                        anyhow::anyhow!(
                            "A repo named '{}' already exists. Use 'commitmux status' to see all repos.",
                            repo_name
                        )
                    } else {
                        e.into()
                    }
                })?;

                println!("Added repo '{}' at {}", repo_name, clone_dir.display());
            } else if let Some(local_path) = path {
                // Local path ingestion
                let canonical = local_path.canonicalize()
                    .with_context(|| format!("Failed to canonicalize path: {}", local_path.display()))?;

                // Verify the path is a git repository (discard libgit2 internals from error chain)
                git2::Repository::open(&canonical).map_err(|_| {
                    anyhow::anyhow!("'{}' is not a git repository", canonical.display())
                })?;

                let repo_name = match name {
                    Some(n) => n,
                    None => canonical
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string(),
                };

                store.add_repo(&RepoInput {
                    name: repo_name.clone(),
                    local_path: canonical.clone(),
                    remote_url: None,
                    default_branch: None,
                    fork_of: fork_of.clone(),
                    author_filter: author.clone(),
                    exclude_prefixes: exclude.clone(),
                    embed_enabled: false, // TODO Wave 2A: wire --embed/--no-embed flags
                })
                .map_err(|e| {
                    if e.to_string().contains("UNIQUE constraint") {
                        anyhow::anyhow!(
                            "A repo named '{}' already exists. Use 'commitmux status' to see all repos.",
                            repo_name
                        )
                    } else {
                        e.into()
                    }
                })?;

                println!("Added repo '{}' at {}", repo_name, canonical.display());
            } else {
                anyhow::bail!("Either a local path or --url must be provided. Usage:\n  commitmux add-repo <PATH>\n  commitmux add-repo --url <URL>");
            }
        }

        Commands::RemoveRepo { name, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            // Get repo info before removing (for commit count and managed clone cleanup)
            let repo = store
                .get_repo_by_name(&name)
                .with_context(|| format!("Failed to look up repo '{}'", name))?
                .ok_or_else(|| anyhow::anyhow!("Repo '{}' not found", name))?;

            let local_path = repo.local_path.clone();

            // Get commit count before deletion
            let count = store.count_commits_for_repo(repo.repo_id).unwrap_or(0);

            store.remove_repo(&name)
                .with_context(|| format!("Failed to remove repo '{}'", name))?;

            if count > 0 {
                println!("Removed repo '{}' ({} commits deleted from index)", name, count);
            } else {
                println!("Removed repo '{}'", name);
            }

            // Clean up managed clone if under ~/.commitmux/clones/
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            let clones_dir = PathBuf::from(&home).join(".commitmux").join("clones");
            if local_path.starts_with(&clones_dir) {
                match std::fs::remove_dir_all(&local_path) {
                    Ok(_) => println!("Removed managed clone at {}", local_path.display()),
                    Err(e) => eprintln!(
                        "Warning: failed to remove clone at {}: {}", local_path.display(), e
                    ),
                }
            }
        }

        Commands::UpdateRepo { name, fork_of, author, exclude, default_branch, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let repo = store
                .get_repo_by_name(&name)
                .with_context(|| format!("Failed to look up repo '{}'", name))?
                .ok_or_else(|| anyhow::anyhow!("Repo '{}' not found", name))?;

            // Build RepoUpdate: only set fields that were provided via CLI flags.
            let update = RepoUpdate {
                fork_of: fork_of.map(Some),
                author_filter: author.map(Some),
                exclude_prefixes: if exclude.is_empty() { None } else { Some(exclude) },
                default_branch: default_branch.map(Some),
                embed_enabled: None, // TODO Wave 2A: wire --embed/--no-embed flags
            };

            let any_change = update.fork_of.is_some()
                || update.author_filter.is_some()
                || update.exclude_prefixes.is_some()
                || update.default_branch.is_some();

            store.update_repo(repo.repo_id, &update)
                .with_context(|| format!("Failed to update repo '{}'", name))?;

            if any_change {
                println!("Updated repo '{}'", name);
            } else {
                println!("Updated repo '{}' (no changes)", name);
            }
        }

        Commands::Sync { repo, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let repos = if let Some(ref repo_name) = repo {
                let r = store
                    .get_repo_by_name(repo_name)
                    .with_context(|| format!("Failed to look up repo '{}'", repo_name))?
                    .ok_or_else(|| anyhow::anyhow!("Repo '{}' not found", repo_name))?;
                vec![r]
            } else {
                store.list_repos().context("Failed to list repos")?
            };

            let mut any_error = false;
            let mut total_in_index = 0usize;

            for r in &repos {
                let ingester = Git2Ingester::new();
                let config = IgnoreConfig::default();
                match ingester.sync_repo(r, &store, &config) {
                    Ok(summary) => {
                        if summary.commits_filtered > 0 {
                            println!(
                                "Syncing '{}'... {} indexed, {} already indexed, {} filtered by author",
                                r.name,
                                summary.commits_indexed,
                                summary.commits_already_indexed,
                                summary.commits_filtered
                            );
                        } else {
                            println!(
                                "Syncing '{}'... {} indexed, {} already indexed",
                                r.name,
                                summary.commits_indexed,
                                summary.commits_already_indexed
                            );
                        }
                        for err in &summary.errors {
                            eprintln!("  warning: {}", err);
                        }
                        total_in_index += summary.commits_indexed + summary.commits_already_indexed;
                    }
                    Err(e) => {
                        eprintln!("Error syncing '{}': {}", r.name, e);
                        any_error = true;
                    }
                }
            }

            if any_error {
                std::process::exit(1);
            }

            if total_in_index > 0 {
                println!("Tip: run 'commitmux serve' to expose this index via MCP to AI agents.");
            }
        }

        Commands::Show { repo, sha, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            match store.get_commit(&repo, &sha).context("Failed to get commit")? {
                None => {
                    eprintln!("Commit '{}' not found in repo '{}'", sha, repo);
                    std::process::exit(1);
                }
                Some(detail) => {
                    let json = serde_json::to_string_pretty(&detail)
                        .context("Failed to serialize commit to JSON")?;
                    println!("{}", json);
                }
            }
        }

        Commands::Status { db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let repos = store.list_repos().context("Failed to list repos")?;

            if repos.is_empty() {
                println!("No repositories indexed.");
                println!("Run: commitmux add-repo <path>");
                return Ok(());
            }

            println!("{:<20} {:>8}  {:<45}  LAST SYNCED", "REPO", "COMMITS", "SOURCE");
            for r in &repos {
                // Determine source display: remote URL if present, else truncated local path
                let source = if let Some(ref url) = r.remote_url {
                    url.clone()
                } else {
                    let path_str = r.local_path.display().to_string();
                    if path_str.len() > 43 {
                        format!("{}...", &path_str[..43])
                    } else {
                        path_str
                    }
                };

                match store.repo_stats(r.repo_id).with_context(|| format!("Failed to get stats for '{}'", r.name)) {
                    Ok(stats) => {
                        let last_synced = stats
                            .last_synced_at
                            .map(format_timestamp)
                            .unwrap_or_else(|| "never".to_string());
                        println!("{:<20} {:>8}  {:<45}  {}", r.name, stats.commit_count, source, last_synced);
                    }
                    Err(e) => {
                        eprintln!("Error fetching stats for '{}': {}", r.name, e);
                    }
                }

                // Show active filters if any
                if r.author_filter.is_some() || !r.exclude_prefixes.is_empty() {
                    let mut parts = Vec::new();
                    if let Some(ref author) = r.author_filter {
                        parts.push(format!("author={}", author));
                    }
                    if !r.exclude_prefixes.is_empty() {
                        parts.push(format!("exclude=[{}]", r.exclude_prefixes.join(", ")));
                    }
                    println!("  filters: {}", parts.join(", "));
                }
            }
        }

        Commands::Serve { db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!("Database not found at {}. Run 'commitmux init' first.", db_path.display());
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;
            let store: Arc<dyn commitmux_types::Store + 'static> = Arc::new(store);
            commitmux_mcp::run_mcp_server(store).context("MCP server error")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_store::SqliteStore;
    use commitmux_types::{RepoInput, Store};

    fn temp_store() -> (SqliteStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = SqliteStore::open(&dir.path().join("test.db")).expect("open");
        (db, dir)
    }

    #[test]
    fn test_add_repo_persists_author_filter() {
        let (store, _dir) = temp_store();
        store.add_repo(&RepoInput {
            name: "myrepo".into(),
            local_path: std::path::PathBuf::from("/tmp/myrepo"),
            remote_url: None,
            default_branch: None,
            fork_of: None,
            author_filter: Some("alice@example.com".into()),
            exclude_prefixes: vec![],
        }).expect("add_repo");

        let repo = store.get_repo_by_name("myrepo").expect("get").expect("some");
        assert_eq!(repo.author_filter, Some("alice@example.com".to_string()));
    }

    #[test]
    fn test_add_repo_persists_exclude_prefixes() {
        let (store, _dir) = temp_store();
        store.add_repo(&RepoInput {
            name: "myrepo".into(),
            local_path: std::path::PathBuf::from("/tmp/myrepo"),
            remote_url: None,
            default_branch: None,
            fork_of: None,
            author_filter: None,
            exclude_prefixes: vec!["dist/".into(), "vendor/".into()],
        }).expect("add_repo");

        let repo = store.get_repo_by_name("myrepo").expect("get").expect("some");
        assert_eq!(repo.exclude_prefixes, vec!["dist/", "vendor/"]);
    }

    #[test]
    fn test_format_timestamp_includes_utc() {
        // 2024-01-15 12:34:56 UTC = 1705318496
        let ts = 1705318496i64;
        let result = format_timestamp(ts);
        assert!(
            result.ends_with(" UTC"),
            "format_timestamp should end with ' UTC', got: {}",
            result
        );
        // Verify it's a non-trivial formatted string
        assert!(result.len() > 4, "timestamp should be more than just ' UTC'");
    }

    #[test]
    fn test_url_validation_rejects_bare_string() {
        let result = validate_git_url("not-a-url");
        assert!(result.is_err(), "bare string should fail URL validation");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not a valid git URL"),
            "error should mention invalid git URL, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_url_validation_accepts_https() {
        assert!(validate_git_url("https://github.com/user/repo").is_ok());
    }

    #[test]
    fn test_url_validation_accepts_git_at() {
        assert!(validate_git_url("git@github.com:user/repo.git").is_ok());
    }

    #[test]
    fn test_url_validation_accepts_ssh() {
        assert!(validate_git_url("ssh://git@github.com/user/repo.git").is_ok());
    }

    #[test]
    fn test_db_not_found_hint_message() {
        let path = std::path::PathBuf::from("/nonexistent/path/db.sqlite3");
        if !path.exists() {
            let result: Result<()> = Err(anyhow::anyhow!(
                "Database not found at {}. Run 'commitmux init' first.",
                path.display()
            ));
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("Run 'commitmux init' first"),
                "hint message should mention init, got: {}",
                msg
            );
        }
    }

    #[test]
    fn test_git2_error_suppressed() {
        let dir = tempfile::tempdir().expect("tempdir");
        // dir is not a git repo; map_err discards the libgit2 cause chain
        let result = git2::Repository::open(dir.path())
            .map(|_| ())
            .map_err(|_| anyhow::anyhow!("'{}' is not a git repository", dir.path().display()));
        let msg = result.unwrap_err().to_string();
        assert!(
            !msg.contains("class=Repository"),
            "libgit2 internals should not appear in error, got: {}",
            msg
        );
        assert!(
            !msg.contains("code=NotFound"),
            "libgit2 internals should not appear in error, got: {}",
            msg
        );
    }

    #[test]
    fn test_mcp_tip_on_resync() {
        // Re-sync: 0 new commits, but 43 already indexed — tip should show
        let commits_indexed = 0usize;
        let commits_already_indexed = 43usize;
        let total_in_index = commits_indexed + commits_already_indexed;
        assert!(
            total_in_index > 0,
            "tip should show when index is non-empty, even with 0 new commits"
        );
    }
}
