use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{Context, Result};

use commitmux_ingest::Git2Ingester;
use commitmux_store::SqliteStore;
use commitmux_types::{IgnoreConfig, Ingester, RepoInput, RepoUpdate, Store};

#[derive(Parser)]
#[command(name = "commitmux", about = "Cross-repo git history index for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        #[arg(long)]
        db: Option<PathBuf>,
    },
    AddRepo {
        #[arg(conflicts_with = "url")]
        path: Option<PathBuf>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(conflicts_with = "path", long)]
        url: Option<String>,
        #[arg(long = "fork-of")]
        fork_of: Option<String>,
        #[arg(long = "author")]
        author: Option<String>,
    },
    RemoveRepo {
        name: String,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    UpdateRepo {
        name: String,
        #[arg(long = "fork-of")]
        fork_of: Option<String>,
        #[arg(long = "author")]
        author: Option<String>,
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        #[arg(long = "default-branch")]
        default_branch: Option<String>,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    Sync {
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    Show {
        repo: String,
        sha: String,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    Status {
        #[arg(long)]
        db: Option<PathBuf>,
    },
    Serve {
        #[arg(long)]
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
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m, d, hours, minutes, seconds
    )
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
            SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;
            println!("Initialized commitmux database at {}", db_path.display());
        }

        Commands::AddRepo { path, name, exclude, db, url, fork_of, author } => {
            let db_path = resolve_db_path(db);
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            if let Some(remote_url) = url {
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
                })
                .with_context(|| format!("Failed to add repo '{}'", repo_name))?;

                println!("Added repo '{}' at {}", repo_name, clone_dir.display());
            } else if let Some(local_path) = path {
                // Local path ingestion
                let canonical = local_path.canonicalize()
                    .with_context(|| format!("Failed to canonicalize path: {}", local_path.display()))?;

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
                })
                .with_context(|| format!("Failed to add repo '{}'", repo_name))?;

                println!("Added repo '{}' at {}", repo_name, canonical.display());
            } else {
                anyhow::bail!("Either a local path or --url must be provided. Usage:\n  commitmux add-repo <PATH>\n  commitmux add-repo --url <URL>");
            }
        }

        Commands::RemoveRepo { name, db } => {
            let db_path = resolve_db_path(db);
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            // Get repo path before removing (to clean up managed clone)
            let local_path = store
                .get_repo_by_name(&name)
                .with_context(|| format!("Failed to look up repo '{}'", name))?
                .map(|r| r.local_path);

            store.remove_repo(&name)
                .with_context(|| format!("Failed to remove repo '{}'", name))?;

            println!("Removed repo '{}'", name);

            // Clean up managed clone if under ~/.commitmux/clones/
            if let Some(lp) = local_path {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                let clones_dir = PathBuf::from(&home).join(".commitmux").join("clones");
                if lp.starts_with(&clones_dir) {
                    match std::fs::remove_dir_all(&lp) {
                        Ok(_) => println!("Removed managed clone at {}", lp.display()),
                        Err(e) => eprintln!(
                            "Warning: failed to remove clone at {}: {}", lp.display(), e
                        ),
                    }
                }
            }
        }

        Commands::UpdateRepo { name, fork_of, author, exclude, default_branch, db } => {
            let db_path = resolve_db_path(db);
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

            for r in &repos {
                let ingester = Git2Ingester::new();
                let config = IgnoreConfig::default();
                match ingester.sync_repo(r, &store, &config) {
                    Ok(summary) => {
                        println!(
                            "Syncing '{}'... {} commits indexed, {} skipped",
                            r.name, summary.commits_indexed, summary.commits_skipped
                        );
                        for err in &summary.errors {
                            eprintln!("  warning: {}", err);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error syncing '{}': {}", r.name, e);
                    }
                }
            }
        }

        Commands::Show { repo, sha, db } => {
            let db_path = resolve_db_path(db);
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            match store.get_commit(&repo, &sha).context("Failed to get commit")? {
                None => {
                    eprintln!("Commit not found");
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
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let repos = store.list_repos().context("Failed to list repos")?;

            println!("{:<20} {:>8}  LAST SYNCED", "REPO", "COMMITS");
            for r in &repos {
                match store.repo_stats(r.repo_id).with_context(|| format!("Failed to get stats for '{}'", r.name)) {
                    Ok(stats) => {
                        let last_synced = stats
                            .last_synced_at
                            .map(format_timestamp)
                            .unwrap_or_else(|| "never".to_string());
                        println!("{:<20} {:>8}  {}", r.name, stats.commit_count, last_synced);
                    }
                    Err(e) => {
                        eprintln!("Error fetching stats for '{}': {}", r.name, e);
                    }
                }
            }
        }

        Commands::Serve { db } => {
            let db_path = resolve_db_path(db);
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
}
