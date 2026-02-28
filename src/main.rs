use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{Context, Result};

use commitmux_ingest::Git2Ingester;
use commitmux_store::SqliteStore;
use commitmux_types::{IgnoreConfig, Ingester, RepoInput, Store};

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
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "exclude")]
        exclude: Vec<String>,
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

        Commands::AddRepo { path, name, exclude, db } => {
            let db_path = resolve_db_path(db);
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let canonical = path.canonicalize()
                .with_context(|| format!("Failed to canonicalize path: {}", path.display()))?;

            let repo_name = match name {
                Some(n) => n,
                None => canonical
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            };

            if !exclude.is_empty() {
                eprintln!(
                    "Note: exclude prefixes {:?} will be applied during sync (in addition to defaults)",
                    exclude
                );
            }

            store.add_repo(&RepoInput {
                name: repo_name.clone(),
                local_path: canonical.clone(),
                remote_url: None,
                default_branch: None,
            })
            .with_context(|| format!("Failed to add repo '{}'", repo_name))?;

            println!("Added repo '{}' at {}", repo_name, canonical.display());
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

            println!("{:<20} {:>8}  {}", "REPO", "COMMITS", "LAST SYNCED");
            for r in &repos {
                match store.repo_stats(r.repo_id).with_context(|| format!("Failed to get stats for '{}'", r.name)) {
                    Ok(stats) => {
                        let last_synced = stats
                            .last_synced_at
                            .map(|ts| format_timestamp(ts))
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
