use commitmux_ingest::Git2Ingester;
use commitmux_store::SqliteStore;
use commitmux_types::{IgnoreConfig, Ingester, RepoInput, SearchOpts, Store, TouchOpts};

#[test]
fn test_end_to_end() {
    // 1. Create a temp dir for the DB
    let db_dir = tempfile::tempdir().unwrap();
    let db_path = db_dir.path().join("test.sqlite3");

    // 2. Create a real git repo in a temp dir with 2 commits
    let repo_dir = tempfile::tempdir().unwrap();
    let git_repo = git2::Repository::init(repo_dir.path()).unwrap();

    // Configure signature
    let sig = git2::Signature::now("Test User", "test@example.com").unwrap();

    // First commit
    let mut index = git_repo.index().unwrap();
    let readme_path = repo_dir.path().join("README.md");
    std::fs::write(&readme_path, "# Test Repo\n").unwrap();
    index.add_path(std::path::Path::new("README.md")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = git_repo.find_tree(tree_oid).unwrap();
    git_repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            "initial commit: add README",
            &tree,
            &[],
        )
        .unwrap();

    // Second commit
    let src_path = repo_dir.path().join("src");
    std::fs::create_dir_all(&src_path).unwrap();
    std::fs::write(src_path.join("main.rs"), "fn main() {}\n").unwrap();
    let mut index = git_repo.index().unwrap();
    index.add_path(std::path::Path::new("src/main.rs")).unwrap();
    index.write().unwrap();
    let tree_oid = index.write_tree().unwrap();
    let tree = git_repo.find_tree(tree_oid).unwrap();
    let parent = git_repo.head().unwrap().peel_to_commit().unwrap();
    git_repo
        .commit(Some("HEAD"), &sig, &sig, "add main.rs", &tree, &[&parent])
        .unwrap();

    // 3. Open store, add repo, sync
    let store = SqliteStore::open(&db_path).unwrap();
    let repo_input = RepoInput {
        name: "test-repo".to_string(),
        local_path: repo_dir.path().to_path_buf(),
        remote_url: None,
        default_branch: None,
        fork_of: None,
        author_filter: None,
        exclude_prefixes: vec![],
        embed_enabled: false,
    };
    let repo = store.add_repo(&repo_input).unwrap();

    let ingester = Git2Ingester::new();
    let config = IgnoreConfig::default();
    let summary = ingester.sync_repo(&repo, &store, &config).unwrap();

    assert_eq!(summary.commits_indexed, 2, "Expected 2 commits indexed");
    assert!(
        summary.errors.is_empty(),
        "Expected no errors: {:?}",
        summary.errors
    );

    // 4. Search for "initial commit"
    let opts = SearchOpts {
        since: None,
        repos: None,
        paths: None,
        limit: Some(10),
    };
    let results = store.search("initial commit", &opts).unwrap();
    assert!(!results.is_empty(), "Expected search to return results");
    assert_eq!(results[0].repo, "test-repo");
    assert!(results[0].subject.contains("initial commit"));

    // 5. Test touches
    let touch_opts = TouchOpts::default();
    let touches = store.touches("src/", &touch_opts).unwrap();
    assert!(
        !touches.is_empty(),
        "Expected touches for src/ to return results"
    );

    // 6. Test get_commit
    let sha = &results[0].sha;
    let detail = store.get_commit("test-repo", sha).unwrap();
    assert!(detail.is_some());
    assert_eq!(detail.unwrap().repo, "test-repo");
}
