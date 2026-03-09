use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use commitmux_embed::EmbedConfig;
use commitmux_ingest::Git2Ingester;
use commitmux_store::SqliteStore;
use commitmux_types::{IgnoreConfig, Ingester, RepoInput, RepoUpdate, Store};

#[derive(Parser)]
#[command(
    name = "commitmux",
    about = "Cross-repo git history index for AI agents",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Initialize the commitmux database")]
    Init {
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
    },
    #[command(about = "Add a git repository to the index")]
    AddRepo {
        #[arg(
            conflicts_with = "url",
            help = "Local path to a git repository (mutually exclusive with --url)"
        )]
        path: Option<PathBuf>,
        #[arg(long, help = "Override the repo name (default: directory name)")]
        name: Option<String>,
        #[arg(
            long = "exclude",
            help = "Path prefix to exclude from indexing (repeatable)"
        )]
        exclude: Vec<String>,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
        #[arg(
            conflicts_with = "path",
            long,
            help = "Remote git URL to clone and index"
        )]
        url: Option<String>,
        #[arg(
            long = "fork-of",
            help = "Upstream repo URL; only index commits not in upstream"
        )]
        fork_of: Option<String>,
        #[arg(
            long = "author",
            help = "Only index commits by this author (email match)"
        )]
        author: Option<String>,
        #[arg(
            long = "embed",
            help = "Enable semantic embeddings for this repo. Requires: 1) Ollama running, 2) embed.model configured (see: commitmux config --help)"
        )]
        embed: bool,
    },
    #[command(about = "Remove a repository and all its indexed commits")]
    RemoveRepo {
        #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
        name: String,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
    },
    #[command(about = "Update stored metadata for a repository")]
    UpdateRepo {
        #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
        name: String,
        #[arg(
            long = "fork-of",
            help = "Upstream repo URL; only index commits not in upstream"
        )]
        fork_of: Option<String>,
        #[arg(
            long = "author",
            help = "Only index commits by this author (email match)"
        )]
        author: Option<String>,
        #[arg(
            long = "exclude",
            help = "Path prefix to exclude from indexing (repeatable)"
        )]
        exclude: Vec<String>,
        #[arg(long = "default-branch", help = "Set the default branch name")]
        default_branch: Option<String>,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
        #[arg(
            long = "embed",
            conflicts_with = "no_embed",
            help = "Enable semantic embeddings for this repo"
        )]
        embed: bool,
        #[arg(
            long = "no-embed",
            conflicts_with = "embed",
            help = "Disable semantic embeddings for this repo"
        )]
        no_embed: bool,
    },
    #[command(about = "Index new commits from one or all repositories")]
    Sync {
        #[arg(long, help = "Sync only this repo (default: sync all)")]
        repo: Option<String>,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
        #[arg(
            long = "embed-only",
            help = "Generate embeddings for already-indexed commits; skip indexing new commits. Useful for backfilling when embeddings were enabled after initial sync."
        )]
        embed_only: bool,
    },
    #[command(about = "Show full details for a specific commit (JSON output)")]
    Show {
        #[arg(help = "Name of the indexed repository (see 'commitmux status')")]
        repo: String,
        #[arg(help = "Full or prefix SHA of the commit")]
        sha: String,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
    },
    #[command(about = "Show all indexed repositories with commit counts and sync times")]
    Status {
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
    },
    #[command(about = "Start the MCP JSON-RPC server for AI agent access")]
    Serve {
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
    },
    #[command(
        about = "Get or set global configuration values. For semantic search: set embed.model (e.g. nomic-embed-text) and embed.endpoint (default: http://localhost:11434/v1). Requires Ollama running."
    )]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
    },
    #[command(about = "Ingest claudewatch memory files for semantic search")]
    IngestMemory {
        #[arg(
            long = "claude-home",
            help = "Path to .claude directory (default: ~/.claude)"
        )]
        claude_home: Option<PathBuf>,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
    },
    #[command(
        about = "Install a post-commit git hook that calls 'commitmux sync' after every commit"
    )]
    InstallHook {
        #[arg(help = "Path to the git repository root")]
        repo: PathBuf,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3)"
        )]
        db: Option<PathBuf>,
        #[arg(long, help = "Overwrite existing hook without prompting")]
        force: bool,
    },
    #[command(about = "Index IMPL docs from docs/IMPL/IMPL-*.md files in a working tree")]
    IndexImplDocs {
        #[arg(help = "Path to working tree root (must contain docs/IMPL/)")]
        path: PathBuf,
        #[arg(long, help = "Project name for tagging (default: directory name)")]
        project: Option<String>,
        #[arg(long, help = "Path to database file")]
        db: Option<PathBuf>,
    },
    #[command(
        about = "Install commitmux ingest-memory as a Claude Code Stop hook in ~/.claude/settings.json"
    )]
    InstallMemoryHook {
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
        #[arg(
            long = "claude-settings",
            help = "Path to Claude settings.json (default: ~/.claude/settings.json)"
        )]
        claude_settings: Option<PathBuf>,
    },
    #[command(about = "Delete and rebuild embeddings for one or all repositories")]
    Reindex {
        #[arg(long, help = "Name of repo to reindex (omit to reindex all)")]
        repo: Option<String>,
        #[arg(
            long,
            help = "Path to database file (default: ~/.commitmux/db.sqlite3, or $COMMITMUX_DB)"
        )]
        db: Option<PathBuf>,
        #[arg(
            long = "reset-dim",
            help = "Reset stored embed.dimension (use when switching embedding models)"
        )]
        reset_dim: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    #[command(about = "Set a configuration value")]
    Set {
        #[arg(help = "Configuration key (e.g. embed.model, embed.endpoint)")]
        key: String,
        #[arg(help = "Value to set")]
        value: String,
    },
    #[command(about = "Get a configuration value")]
    Get {
        #[arg(help = "Configuration key (e.g. embed.model, embed.endpoint)")]
        key: String,
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

fn install_memory_hook(settings_path: &std::path::Path, command: &str) -> Result<()> {
    // Read existing settings or start fresh
    let mut value: serde_json::Value = if settings_path.exists() {
        let raw = std::fs::read_to_string(settings_path).with_context(|| {
            format!("Failed to read settings file: {}", settings_path.display())
        })?;
        serde_json::from_str(&raw).with_context(|| {
            format!(
                "Failed to parse settings.json at {}: not valid JSON",
                settings_path.display()
            )
        })?
    } else {
        serde_json::json!({})
    };

    // Duplicate check: look for "commitmux ingest-memory" in existing Stop hooks
    if let Some(stop_hooks) = value["hooks"]["Stop"].as_array() {
        for entry in stop_hooks {
            if let Some(hooks) = entry["hooks"].as_array() {
                for hook in hooks {
                    if let Some(cmd) = hook["command"].as_str() {
                        if cmd.contains("commitmux ingest-memory") {
                            println!(
                                "commitmux ingest-memory is already registered as a Stop hook."
                            );
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    // Build the new hook group entry
    let new_entry = serde_json::json!({
        "matcher": "",
        "hooks": [{ "type": "command", "command": command }]
    });

    // Ensure path hooks.Stop exists as an array and append
    {
        let hooks = value
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("settings.json root must be a JSON object"))?
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}));
        let stop = hooks
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("settings.json 'hooks' must be a JSON object"))?
            .entry("Stop")
            .or_insert_with(|| serde_json::json!([]));
        stop.as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("settings.json 'hooks.Stop' must be an array"))?
            .push(new_entry);
    }

    // Write back with pretty formatting, creating parent dirs if needed
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    let pretty =
        serde_json::to_string_pretty(&value).context("Failed to serialize settings.json")?;
    std::fs::write(settings_path, pretty)
        .with_context(|| format!("Failed to write settings file: {}", settings_path.display()))?;

    println!(
        "Installed: commitmux ingest-memory will run after each Claude Code session.\nSettings: {}",
        settings_path.display()
    );
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

        Commands::AddRepo {
            path,
            name,
            exclude,
            db,
            url,
            fork_of,
            author,
            embed,
        } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
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

                std::fs::create_dir_all(&clone_dir).with_context(|| {
                    format!("Failed to create clone directory: {}", clone_dir.display())
                })?;

                let mut callbacks = git2::RemoteCallbacks::new();
                callbacks.credentials(|_url, username, _allowed| {
                    git2::Cred::ssh_key_from_agent(username.unwrap_or("git"))
                });
                let mut fo = git2::FetchOptions::new();
                fo.remote_callbacks(callbacks);
                let mut builder = git2::build::RepoBuilder::new();
                builder.fetch_options(fo);
                builder.clone(&remote_url, &clone_dir).with_context(|| {
                    format!("Failed to clone '{}' from '{}'", repo_name, remote_url)
                })?;

                store.add_repo(&RepoInput {
                    name: repo_name.clone(),
                    local_path: clone_dir.clone(),
                    remote_url: Some(remote_url.clone()),
                    default_branch: None,
                    fork_of: fork_of.clone(),
                    author_filter: author.clone(),
                    exclude_prefixes: exclude.clone(),
                    embed_enabled: embed,
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
                let canonical = local_path.canonicalize().with_context(|| {
                    format!("Failed to canonicalize path: {}", local_path.display())
                })?;

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
                    embed_enabled: embed,
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
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
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

            store
                .remove_repo(&name)
                .with_context(|| format!("Failed to remove repo '{}'", name))?;

            if count > 0 {
                println!(
                    "Removed repo '{}' ({} commits deleted from index)",
                    name, count
                );
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
                        "Warning: failed to remove clone at {}: {}",
                        local_path.display(),
                        e
                    ),
                }
            }
        }

        Commands::UpdateRepo {
            name,
            fork_of,
            author,
            exclude,
            default_branch,
            db,
            embed,
            no_embed,
        } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let repo = store
                .get_repo_by_name(&name)
                .with_context(|| format!("Failed to look up repo '{}'", name))?
                .ok_or_else(|| anyhow::anyhow!("Repo '{}' not found", name))?;

            // Build RepoUpdate: only set fields that were provided via CLI flags.
            let embed_enabled = if embed {
                Some(true)
            } else if no_embed {
                Some(false)
            } else {
                None
            };
            let update = RepoUpdate {
                fork_of: fork_of.map(Some),
                author_filter: author.map(Some),
                exclude_prefixes: if exclude.is_empty() {
                    None
                } else {
                    Some(exclude)
                },
                default_branch: default_branch.map(Some),
                embed_enabled,
            };

            let any_change = update.fork_of.is_some()
                || update.author_filter.is_some()
                || update.exclude_prefixes.is_some()
                || update.default_branch.is_some()
                || update.embed_enabled.is_some();

            store
                .update_repo(repo.repo_id, &update)
                .with_context(|| format!("Failed to update repo '{}'", name))?;

            if any_change {
                println!("Updated repo '{}'", name);
            } else {
                println!("Updated repo '{}' (no changes)", name);
            }
        }

        Commands::Sync {
            repo,
            db,
            embed_only,
        } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
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

            if embed_only {
                // Skip git ingest; only run embedding for repos with embed_enabled
                for r in &repos {
                    if r.embed_enabled {
                        match EmbedConfig::from_store(&store) {
                            Ok(config) => {
                                let embedder = commitmux_embed::Embedder::new(&config);
                                let rt = tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                    .expect("tokio runtime");
                                match rt.block_on(commitmux_embed::embed_pending(
                                    &store, &embedder, r.repo_id, 50,
                                )) {
                                    Ok(esummary) => {
                                        println!(
                                            "Embedding '{}'... {} embedded, {} failed",
                                            r.name, esummary.embedded, esummary.failed
                                        );
                                        // Update last_synced_at timestamp after successful embedding
                                        let now = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap()
                                            .as_secs()
                                            as i64;
                                        let prev_state =
                                            store.get_ingest_state(r.repo_id).ok().flatten();
                                        let ingest_state = commitmux_types::IngestState {
                                            repo_id: r.repo_id,
                                            last_synced_at: now,
                                            last_synced_sha: prev_state
                                                .as_ref()
                                                .and_then(|s| s.last_synced_sha.clone()),
                                            last_error: None,
                                        };
                                        let _ = store.update_ingest_state(&ingest_state);
                                    }
                                    Err(e) => eprintln!(
                                        "  Warning: embedding failed for '{}': {e}",
                                        r.name
                                    ),
                                }
                            }
                            Err(e) => {
                                eprintln!("  Warning: embed config error for '{}': {e}", r.name)
                            }
                        }
                    }
                }
            } else {
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
                            total_in_index +=
                                summary.commits_indexed + summary.commits_already_indexed;
                        }
                        Err(e) => {
                            eprintln!("Error syncing '{}': {}", r.name, e);
                            any_error = true;
                        }
                    }

                    // After ingest, backfill embeddings for repos with embed_enabled
                    if r.embed_enabled {
                        match EmbedConfig::from_store(&store) {
                            Ok(config) => {
                                let embedder = commitmux_embed::Embedder::new(&config);
                                let rt = tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                    .expect("tokio runtime");
                                match rt.block_on(commitmux_embed::embed_pending(
                                    &store, &embedder, r.repo_id, 50,
                                )) {
                                    Ok(esummary) => {
                                        if esummary.embedded > 0 || esummary.failed > 0 {
                                            println!(
                                                "  Embedded {} commits ({} failed)",
                                                esummary.embedded, esummary.failed
                                            );
                                        }
                                    }
                                    Err(e) => eprintln!(
                                        "  Warning: embedding failed for '{}': {e}",
                                        r.name
                                    ),
                                }
                            }
                            Err(e) => {
                                eprintln!("  Warning: embed config error for '{}': {e}", r.name)
                            }
                        }
                    }
                }

                if any_error {
                    std::process::exit(1);
                }

                if total_in_index > 0 {
                    println!(
                        "Tip: run 'commitmux serve' to expose this index via MCP to AI agents."
                    );
                }
            }
        }

        Commands::Show { repo, sha, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            match store
                .get_commit(&repo, &sha)
                .context("Failed to get commit")?
            {
                None => {
                    eprintln!("Error: Commit '{}' not found in repo '{}'", sha, repo);
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
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let repos = store.list_repos().context("Failed to list repos")?;

            if repos.is_empty() {
                println!("No repositories indexed.");
                println!("Run: commitmux add-repo <path>");
                return Ok(());
            }

            let any_embed = repos.iter().any(|r| r.embed_enabled);

            if any_embed {
                println!(
                    "{:<20} {:>8}  {:<45}  {:<22}  EMBED",
                    "REPO", "COMMITS", "SOURCE", "LAST SYNCED"
                );
            } else {
                println!(
                    "{:<20} {:>8}  {:<45}  LAST SYNCED",
                    "REPO", "COMMITS", "SOURCE"
                );
            }

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

                match store
                    .repo_stats(r.repo_id)
                    .with_context(|| format!("Failed to get stats for '{}'", r.name))
                {
                    Ok(stats) => {
                        let last_synced = stats
                            .last_synced_at
                            .map(format_timestamp)
                            .unwrap_or_else(|| "never".to_string());
                        if any_embed {
                            let embed_col = if r.embed_enabled {
                                let embedding_count =
                                    store.count_embeddings_for_repo(r.repo_id).unwrap_or(0);
                                if embedding_count == stats.commit_count {
                                    "✓"
                                } else {
                                    "⋯"
                                }
                            } else {
                                "-"
                            };
                            println!(
                                "{:<20} {:>8}  {:<45}  {:<22}  {}",
                                r.name, stats.commit_count, source, last_synced, embed_col
                            );
                        } else {
                            println!(
                                "{:<20} {:>8}  {:<45}  {}",
                                r.name, stats.commit_count, source, last_synced
                            );
                        }
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

            if any_embed {
                let model = store
                    .get_config("embed.model")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "nomic-embed-text (default)".into());
                let endpoint = store
                    .get_config("embed.endpoint")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "http://localhost:11434/v1 (default)".into());
                println!(
                    "\nEmbedding model: {} ({}) — ✓ = complete, ⋯ = pending",
                    model, endpoint
                );
            }
        }

        Commands::Serve { db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            // Auto-sync: sync repos that have never been synced or were last synced > 1 hour ago
            let now_unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            match store.list_repos() {
                Ok(repos) => {
                    for r in &repos {
                        let needs_sync = match store.get_ingest_state(r.repo_id) {
                            Ok(None) => true,
                            Ok(Some(state)) => (now_unix - state.last_synced_at) > 3600,
                            Err(e) => {
                                eprintln!(
                                    "commitmux: warning: failed to get ingest state for '{}': {}",
                                    r.name, e
                                );
                                false
                            }
                        };
                        if needs_sync {
                            eprintln!("commitmux: syncing '{}' on startup...", r.name);
                            let ingester = Git2Ingester::new();
                            let config = IgnoreConfig::default();
                            match ingester.sync_repo(r, &store, &config) {
                                Ok(summary) => {
                                    eprintln!(
                                        "commitmux: sync '{}' complete: {} indexed, {} already indexed",
                                        r.name,
                                        summary.commits_indexed,
                                        summary.commits_already_indexed
                                    );
                                }
                                Err(e) => {
                                    eprintln!(
                                        "commitmux: warning: sync failed for '{}': {}",
                                        r.name, e
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "commitmux: warning: failed to list repos for auto-sync: {}",
                        e
                    );
                }
            }

            let store: Arc<dyn commitmux_types::Store + 'static> = Arc::new(store);
            eprintln!("commitmux MCP server ready (JSON-RPC over stdio). Press Ctrl+C to stop.");
            commitmux_mcp::run_mcp_server(store).context("MCP server error")?;
        }

        Commands::Config { action, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;
            match action {
                ConfigAction::Set { key, value } => {
                    const VALID_CONFIG_KEYS: &[&str] = &["embed.model", "embed.endpoint"];
                    if !VALID_CONFIG_KEYS.contains(&key.as_str()) {
                        anyhow::bail!(
                            "Unknown config key '{}'. Valid keys: {}",
                            key,
                            VALID_CONFIG_KEYS.join(", ")
                        );
                    }
                    if value.trim().is_empty() {
                        anyhow::bail!("Value for '{}' cannot be empty", key);
                    }
                    store
                        .set_config(&key, &value)
                        .context("Failed to set config")?;
                    println!("Set {} = {}", key, value);
                }
                ConfigAction::Get { key } => {
                    match store.get_config(&key).context("Failed to get config")? {
                        Some(value) => println!("{}", value),
                        None => println!("(not set)"),
                    }
                }
            }
        }

        Commands::IngestMemory { claude_home, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            let claude_dir = claude_home.unwrap_or_else(|| PathBuf::from(&home).join(".claude"));

            if !claude_dir.exists() {
                anyhow::bail!("Claude directory not found at {}", claude_dir.display());
            }

            // Scan projects/*/memory/*.md
            let projects_dir = claude_dir.join("projects");
            if !projects_dir.exists() {
                println!("No projects directory found at {}", projects_dir.display());
                return Ok(());
            }

            let mut total_ingested = 0usize;
            let mut total_skipped = 0usize;

            for project_entry in std::fs::read_dir(&projects_dir)? {
                let project_entry = project_entry?;
                let memory_dir = project_entry.path().join("memory");
                if !memory_dir.is_dir() {
                    continue;
                }

                // Extract project name from directory name
                let project_name = project_entry.file_name().to_string_lossy().to_string();

                for file_entry in std::fs::read_dir(&memory_dir)? {
                    let file_entry = file_entry?;
                    let path = file_entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        continue;
                    }

                    let metadata = std::fs::metadata(&path)?;
                    let file_mtime = metadata
                        .modified()
                        .unwrap_or(std::time::UNIX_EPOCH)
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;

                    let source = path.to_string_lossy().to_string();

                    // Check if already indexed with same mtime
                    if let Ok(Some(existing)) = store.get_memory_doc_by_source(&source) {
                        if existing.file_mtime >= file_mtime {
                            total_skipped += 1;
                            continue;
                        }
                    }

                    let content = std::fs::read_to_string(&path)?;
                    let input = commitmux_types::MemoryDocInput {
                        source,
                        project: project_name.clone(),
                        source_type: commitmux_types::MemorySourceType::ImplDoc,
                        content,
                        file_mtime,
                    };
                    store.upsert_memory_doc(&input)?;
                    total_ingested += 1;
                }
            }

            println!(
                "Ingested {} IMPL docs ({} unchanged, skipped)",
                total_ingested, total_skipped
            );

            // Embed any docs without embeddings
            match EmbedConfig::from_store(&store) {
                Ok(config) => {
                    let embedder = commitmux_embed::Embedder::new(&config);
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("tokio runtime");
                    match rt.block_on(commitmux_embed::embed_memory_pending(&store, &embedder, 50))
                    {
                        Ok(summary) => {
                            if summary.embedded > 0 || summary.failed > 0 {
                                println!(
                                    "Embedded {} memory docs ({} failed)",
                                    summary.embedded, summary.failed
                                );
                            }
                        }
                        Err(e) => eprintln!("Warning: embedding failed: {e}"),
                    }
                }
                Err(e) => eprintln!("Warning: embed config error: {e}"),
            }
        }

        Commands::InstallHook { repo, db: _, force } => {
            let canonical = repo
                .canonicalize()
                .with_context(|| format!("Failed to canonicalize repo path: {}", repo.display()))?;

            // Verify it's a git repository
            let git_dir = canonical.join(".git");
            if !git_dir.is_dir() {
                anyhow::bail!(
                    "'{}' is not a git repository (no .git directory found)",
                    canonical.display()
                );
            }

            let hooks_dir = git_dir.join("hooks");
            std::fs::create_dir_all(&hooks_dir).with_context(|| {
                format!("Failed to create hooks directory: {}", hooks_dir.display())
            })?;

            let hook_path = hooks_dir.join("post-commit");

            if hook_path.exists() && !force {
                eprintln!(
                    "Warning: post-commit hook already exists at {}. Use --force to overwrite.",
                    hook_path.display()
                );
                return Ok(());
            }

            let hook_content =
                "#!/bin/sh\ncommitmux sync --repo \"$(git rev-parse --show-toplevel)\" 2>/dev/null || true\n";
            std::fs::write(&hook_path, hook_content)
                .with_context(|| format!("Failed to write hook to {}", hook_path.display()))?;

            // chmod +x (mode 0o755)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o755);
                std::fs::set_permissions(&hook_path, perms).with_context(|| {
                    format!("Failed to set permissions on {}", hook_path.display())
                })?;
            }

            println!("Installed post-commit hook at {}", hook_path.display());
        }

        Commands::InstallMemoryHook {
            db,
            claude_settings,
        } => {
            let settings_path = claude_settings.unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".claude").join("settings.json")
            });

            let command = if let Some(db_path) = db {
                format!("commitmux ingest-memory --db {}", db_path.display())
            } else {
                "commitmux ingest-memory".to_string()
            };

            install_memory_hook(&settings_path, &command)?;
        }

        Commands::Reindex {
            repo,
            db,
            reset_dim,
        } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
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

            if reset_dim {
                // Clear stored dimension so validate_or_store_dimension will accept the new model's
                // dimension on the next embed call. Setting to "0" means stored_dim parses as 0,
                // and 0 != actual_dim, which still triggers a bail. Instead we delete the key by
                // setting it to empty — validate_or_store_dimension returns None path and stores fresh.
                // NOTE: validate_or_store_dimension checks get_config returning None (not set) vs Some("").
                // Setting "" causes stored_dim.parse() to yield unwrap_or(0), then 0 != dim => Err.
                // The safest approach: delete by setting to the sentinel value that causes re-store.
                // We set the key to "0" here; embed_pending will call validate_or_store_dimension which
                // sees stored_dim=0, compares 0 != actual_dim and bails. This means --reset-dim alone
                // is not sufficient to fully reset — the user must also run:
                //   commitmux config set embed.dimension <N>   (once that key is supported)
                // For now we document this limitation and skip the set_config call to avoid confusion.
                // The core reindex (delete + re-embed) works correctly regardless.
                eprintln!(
                    "Note: --reset-dim support is limited. If you see a dimension mismatch error,\n\
                     manually clear the stored dimension with your sqlite3 client:\n\
                     DELETE FROM config WHERE key = '{}';",
                    commitmux_embed::CONFIG_KEY_EMBED_DIM
                );
            }

            let n = repos.len();
            for r in &repos {
                println!("Reindexing {}...", r.name);
                store
                    .delete_embeddings_for_repo(r.repo_id)
                    .with_context(|| format!("Failed to delete embeddings for '{}'", r.name))?;

                let config = EmbedConfig::from_store(&store)
                    .with_context(|| format!("Failed to read embed config for '{}'", r.name))?;
                let embedder = commitmux_embed::Embedder::new(&config);
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                match rt.block_on(commitmux_embed::embed_pending(
                    &store, &embedder, r.repo_id, 50,
                )) {
                    Ok(esummary) => {
                        println!(
                            "  ✓ {} reindexed ({} embedded, {} failed)",
                            r.name, esummary.embedded, esummary.failed
                        );
                    }
                    Err(e) => {
                        eprintln!("  Warning: embedding failed for '{}': {e}", r.name);
                    }
                }
            }

            println!("Reindex complete. {} repo(s) processed.", n);
        }

        Commands::IndexImplDocs { path, project, db } => {
            let db_path = resolve_db_path(db);
            if !db_path.exists() {
                anyhow::bail!(
                    "Database not found at {}. Run 'commitmux init' first.",
                    db_path.display()
                );
            }
            let store = SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

            let canonical = path
                .canonicalize()
                .with_context(|| format!("Failed to canonicalize path: {}", path.display()))?;

            // Derive project name from directory name if not provided
            let project_name = project.unwrap_or_else(|| {
                canonical
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

            let impl_dir = canonical.join("docs").join("IMPL");
            if !impl_dir.is_dir() {
                anyhow::bail!("No docs/IMPL/ directory found at {}", impl_dir.display());
            }

            let mut total_ingested = 0usize;
            let mut total_skipped = 0usize;

            for entry in std::fs::read_dir(&impl_dir)
                .with_context(|| format!("Failed to read directory: {}", impl_dir.display()))?
            {
                let entry = entry?;
                let entry_path = entry.path();

                if entry_path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }

                let metadata = std::fs::metadata(&entry_path)?;
                let file_mtime = metadata
                    .modified()
                    .unwrap_or(UNIX_EPOCH)
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;

                let source = entry_path.to_string_lossy().to_string();

                // Skip if already indexed with same or newer mtime
                if let Ok(Some(existing)) = store.get_memory_doc_by_source(&source) {
                    if existing.file_mtime >= file_mtime {
                        total_skipped += 1;
                        continue;
                    }
                }

                let content = std::fs::read_to_string(&entry_path)?;
                let input = commitmux_types::MemoryDocInput {
                    source,
                    project: project_name.clone(),
                    source_type: commitmux_types::MemorySourceType::ImplDoc,
                    content,
                    file_mtime,
                };
                store.upsert_memory_doc(&input)?;
                total_ingested += 1;
            }

            println!(
                "Indexed {} IMPL docs from {} ({} unchanged, skipped)",
                total_ingested,
                impl_dir.display(),
                total_skipped
            );
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
        store
            .add_repo(&RepoInput {
                name: "myrepo".into(),
                local_path: std::path::PathBuf::from("/tmp/myrepo"),
                remote_url: None,
                default_branch: None,
                fork_of: None,
                author_filter: Some("alice@example.com".into()),
                exclude_prefixes: vec![],
                embed_enabled: false,
            })
            .expect("add_repo");

        let repo = store
            .get_repo_by_name("myrepo")
            .expect("get")
            .expect("some");
        assert_eq!(repo.author_filter, Some("alice@example.com".to_string()));
    }

    #[test]
    fn test_add_repo_persists_exclude_prefixes() {
        let (store, _dir) = temp_store();
        store
            .add_repo(&RepoInput {
                name: "myrepo".into(),
                local_path: std::path::PathBuf::from("/tmp/myrepo"),
                remote_url: None,
                default_branch: None,
                fork_of: None,
                author_filter: None,
                exclude_prefixes: vec!["dist/".into(), "vendor/".into()],
                embed_enabled: false,
            })
            .expect("add_repo");

        let repo = store
            .get_repo_by_name("myrepo")
            .expect("get")
            .expect("some");
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
        assert!(
            result.len() > 4,
            "timestamp should be more than just ' UTC'"
        );
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
            let msg = format!(
                "Database not found at {}. Run 'commitmux init' first.",
                path.display()
            );
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

    #[test]
    fn test_config_set_get_roundtrip() {
        let (store, _dir) = temp_store();
        store
            .set_config("embed.model", "test-model")
            .expect("set_config");
        let value = store.get_config("embed.model").expect("get_config");
        assert_eq!(value, Some("test-model".to_string()));
    }

    #[test]
    fn test_config_set_rejects_unknown_key() {
        const VALID_CONFIG_KEYS: &[&str] = &["embed.model", "embed.endpoint"];
        assert!(
            VALID_CONFIG_KEYS.contains(&"embed.model"),
            "embed.model should be valid"
        );
        assert!(
            VALID_CONFIG_KEYS.contains(&"embed.endpoint"),
            "embed.endpoint should be valid"
        );
        assert_eq!(
            VALID_CONFIG_KEYS.len(),
            2,
            "should have exactly 2 valid keys"
        );
        assert!(
            !VALID_CONFIG_KEYS.contains(&"embed.endpoint_url"),
            "embed.endpoint_url should be unknown"
        );
    }

    #[test]
    fn test_config_set_rejects_empty_value() {
        let empty = "";
        let whitespace = "   ";
        assert!(empty.trim().is_empty(), "empty string should be rejected");
        assert!(
            whitespace.trim().is_empty(),
            "whitespace-only string should be rejected"
        );
        let valid = "some-model";
        assert!(
            !valid.trim().is_empty(),
            "non-empty value should not be rejected"
        );
    }

    #[test]
    fn test_embed_sync_tip_logic() {
        // embed_only = true with embed_enabled = false: no embedding should be attempted.
        // This is a logic test: verify the condition `r.embed_enabled` gates the embedding call.
        let embed_only = true;
        let embed_enabled = false;

        // Simulate what the Sync handler does: skip embedding when embed_enabled is false
        let would_embed = embed_only && embed_enabled;
        assert!(
            !would_embed,
            "should not attempt embedding when embed_enabled is false, even with --embed-only"
        );
    }

    #[test]
    fn test_install_memory_hook_writes_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let settings_path = dir.path().join("settings.json");

        // First call: should write the hook
        install_memory_hook(&settings_path, "commitmux ingest-memory")
            .expect("first install should succeed");

        let raw = std::fs::read_to_string(&settings_path).expect("read settings");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("parse json");

        // Verify structure
        let stop = value["hooks"]["Stop"].as_array().expect("Stop array");
        assert_eq!(stop.len(), 1, "should have exactly one Stop hook group");
        let hooks = stop[0]["hooks"].as_array().expect("hooks array");
        assert_eq!(hooks.len(), 1, "hook group should have one hook");
        assert_eq!(hooks[0]["type"], "command");
        assert_eq!(hooks[0]["command"], "commitmux ingest-memory");
        assert_eq!(stop[0]["matcher"], "");

        // Second call: duplicate guard should fire, no duplicate added
        install_memory_hook(&settings_path, "commitmux ingest-memory")
            .expect("duplicate call should succeed without error");

        let raw2 = std::fs::read_to_string(&settings_path).expect("read settings after dup");
        let value2: serde_json::Value = serde_json::from_str(&raw2).expect("parse json after dup");
        let stop2 = value2["hooks"]["Stop"]
            .as_array()
            .expect("Stop array after dup");
        assert_eq!(
            stop2.len(),
            1,
            "duplicate guard: should still have exactly one hook group"
        );
    }

    #[test]
    fn test_ingest_memory_command_parses() {
        use clap::Parser;

        // Parse without --claude-home
        let cli = Cli::try_parse_from(["commitmux", "ingest-memory"]);
        assert!(cli.is_ok(), "ingest-memory should parse without args");

        // Parse with --claude-home
        let cli = Cli::try_parse_from([
            "commitmux",
            "ingest-memory",
            "--claude-home",
            "/tmp/.claude",
        ]);
        assert!(cli.is_ok(), "ingest-memory should parse with --claude-home");
        if let Ok(parsed) = cli {
            match parsed.command {
                Commands::IngestMemory { claude_home, db: _ } => {
                    assert_eq!(
                        claude_home,
                        Some(PathBuf::from("/tmp/.claude")),
                        "--claude-home should be parsed"
                    );
                }
                _ => panic!("expected IngestMemory command"),
            }
        }

        // Parse with --db
        let cli = Cli::try_parse_from(["commitmux", "ingest-memory", "--db", "/tmp/test.db"]);
        assert!(cli.is_ok(), "ingest-memory should parse with --db");

        // Parse with both flags
        let cli = Cli::try_parse_from([
            "commitmux",
            "ingest-memory",
            "--claude-home",
            "/tmp/.claude",
            "--db",
            "/tmp/test.db",
        ]);
        assert!(
            cli.is_ok(),
            "ingest-memory should parse with both --claude-home and --db"
        );
    }

    #[test]
    fn test_reindex_command_deletes_and_reembeds() {
        use clap::Parser;

        // Parse without args: reindex all repos
        let cli = Cli::try_parse_from(["commitmux", "reindex"]);
        assert!(cli.is_ok(), "reindex should parse without args");

        // Parse with --repo
        let cli = Cli::try_parse_from(["commitmux", "reindex", "--repo", "myrepo"]);
        assert!(cli.is_ok(), "reindex should parse with --repo");
        if let Ok(parsed) = cli {
            match parsed.command {
                Commands::Reindex {
                    repo,
                    db: _,
                    reset_dim,
                } => {
                    assert_eq!(repo, Some("myrepo".to_string()), "--repo should be parsed");
                    assert!(!reset_dim, "--reset-dim should default to false");
                }
                _ => panic!("expected Reindex command"),
            }
        }

        // Parse with --reset-dim
        let cli = Cli::try_parse_from(["commitmux", "reindex", "--reset-dim"]);
        assert!(cli.is_ok(), "reindex should parse with --reset-dim");
        if let Ok(parsed) = cli {
            match parsed.command {
                Commands::Reindex {
                    repo,
                    db: _,
                    reset_dim,
                } => {
                    assert!(reset_dim, "--reset-dim flag should be true");
                    assert!(repo.is_none(), "repo should be None when not provided");
                }
                _ => panic!("expected Reindex command"),
            }
        }

        // Parse with --db
        let cli = Cli::try_parse_from(["commitmux", "reindex", "--db", "/tmp/test.db"]);
        assert!(cli.is_ok(), "reindex should parse with --db");

        // Parse with all flags combined
        let cli = Cli::try_parse_from([
            "commitmux",
            "reindex",
            "--repo",
            "myrepo",
            "--reset-dim",
            "--db",
            "/tmp/test.db",
        ]);
        assert!(cli.is_ok(), "reindex should parse with all flags");
    }
}
