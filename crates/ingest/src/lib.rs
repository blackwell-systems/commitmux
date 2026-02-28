mod patch;
mod walker;

pub use walker::Git2Ingester;

#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_types::{
        CommitDetail, CommitFile, CommitPatch, EmbedCommit, IgnoreConfig, IngestState,
        Ingester, PatchResult, Repo, RepoInput, RepoListEntry, RepoStats, RepoUpdate,
        Result, SearchOpts, SearchResult, SemanticSearchOpts, Store, TouchOpts, TouchResult,
    };
    use std::sync::Mutex;

    // ── MockStore ────────────────────────────────────────────────────────────

    struct MockStore {
        commits: Mutex<Vec<commitmux_types::Commit>>,
        files: Mutex<Vec<CommitFile>>,
        patches: Mutex<Vec<CommitPatch>>,
        ingest_state: Mutex<Option<IngestState>>,
    }

    impl MockStore {
        fn new() -> Self {
            MockStore {
                commits: Mutex::new(Vec::new()),
                files: Mutex::new(Vec::new()),
                patches: Mutex::new(Vec::new()),
                ingest_state: Mutex::new(None),
            }
        }
    }

    impl Store for MockStore {
        fn add_repo(&self, _input: &RepoInput) -> Result<Repo> {
            unimplemented!()
        }

        fn list_repos(&self) -> Result<Vec<Repo>> {
            unimplemented!()
        }

        fn get_repo_by_name(&self, _name: &str) -> Result<Option<Repo>> {
            unimplemented!()
        }

        fn remove_repo(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn update_repo(&self, _repo_id: i64, _update: &RepoUpdate) -> Result<Repo> {
            unimplemented!()
        }

        fn list_repos_with_stats(&self) -> Result<Vec<RepoListEntry>> {
            unimplemented!()
        }

        fn upsert_commit(&self, commit: &commitmux_types::Commit) -> Result<()> {
            self.commits.lock().unwrap().push(commit.clone());
            Ok(())
        }

        fn upsert_commit_files(&self, files: &[CommitFile]) -> Result<()> {
            self.files.lock().unwrap().extend_from_slice(files);
            Ok(())
        }

        fn upsert_patch(&self, patch: &CommitPatch) -> Result<()> {
            self.patches.lock().unwrap().push(patch.clone());
            Ok(())
        }

        fn get_ingest_state(&self, _repo_id: i64) -> Result<Option<IngestState>> {
            Ok(self.ingest_state.lock().unwrap().clone())
        }

        fn update_ingest_state(&self, state: &IngestState) -> Result<()> {
            *self.ingest_state.lock().unwrap() = Some(state.clone());
            Ok(())
        }

        fn commit_exists(&self, _repo_id: i64, sha: &str) -> Result<bool> {
            // Real impl for test support:
            Ok(self.commits.lock().unwrap().iter().any(|c| c.sha == sha))
        }

        fn search(&self, _query: &str, _opts: &SearchOpts) -> Result<Vec<SearchResult>> {
            unimplemented!()
        }

        fn touches(&self, _path_glob: &str, _opts: &TouchOpts) -> Result<Vec<TouchResult>> {
            unimplemented!()
        }

        fn get_commit(
            &self,
            _repo_name: &str,
            _sha_prefix: &str,
        ) -> Result<Option<CommitDetail>> {
            unimplemented!()
        }

        fn get_patch(
            &self,
            _repo_name: &str,
            _sha: &str,
            _max_bytes: Option<usize>,
        ) -> Result<Option<PatchResult>> {
            unimplemented!()
        }

        fn repo_stats(&self, _repo_id: i64) -> Result<RepoStats> {
            unimplemented!()
        }

        fn count_commits_for_repo(&self, _repo_id: i64) -> Result<usize> {
            Ok(0)
        }

        fn get_config(&self, _key: &str) -> Result<Option<String>> { Ok(None) }
        fn set_config(&self, _key: &str, _value: &str) -> Result<()> { Ok(()) }
        fn get_commits_without_embeddings(&self, _repo_id: i64, _limit: usize) -> Result<Vec<EmbedCommit>> { Ok(vec![]) }
        #[allow(clippy::too_many_arguments)]
        fn store_embedding(&self, _repo_id: i64, _sha: &str, _subject: &str, _author_name: &str, _repo_name: &str, _author_time: i64, _patch_preview: Option<&str>, _embedding: &[f32]) -> Result<()> { Ok(()) }
        fn search_semantic(&self, _embedding: &[f32], _opts: &SemanticSearchOpts) -> Result<Vec<SearchResult>> { Ok(vec![]) }
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn make_repo(path: &std::path::Path) -> Repo {
        Repo {
            repo_id: 1,
            name: "test-repo".into(),
            local_path: path.to_path_buf(),
            remote_url: None,
            default_branch: None,
            fork_of: None,
            author_filter: None,
            exclude_prefixes: vec![],
            embed_enabled: false,
        }
    }

    fn default_config() -> IgnoreConfig {
        IgnoreConfig {
            path_prefixes: vec!["node_modules/".into()],
            max_patch_bytes: 1_048_576,
        }
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[test]
    fn test_sync_empty_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _git_repo = git2::Repository::init(dir.path()).expect("git init");
        // An empty repo has no commits, no HEAD — sync_repo should handle gracefully.
        // Actually, with no commits, head() will fail. The ingester should return
        // an error or empty summary. Let's check what happens.
        let store = MockStore::new();
        let repo = make_repo(dir.path());
        let config = default_config();

        // An empty repo has no HEAD. The ingester will fail to resolve the tip.
        // We treat this as 0 commits indexed with an error.
        let result = Git2Ingester::new().sync_repo(&repo, &store, &config);
        match result {
            Ok(summary) => {
                assert_eq!(summary.commits_indexed, 0, "empty repo: no commits indexed");
            }
            Err(_) => {
                // Also acceptable: return an error for completely empty repos
            }
        }
        let commits = store.commits.lock().unwrap();
        assert_eq!(commits.len(), 0, "no commits in store for empty repo");
    }

    #[test]
    fn test_sync_single_commit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Create a file and commit it
        let file_path = dir.path().join("README.md");
        std::fs::write(&file_path, "# Hello\n").expect("write file");

        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("README.md")).expect("add path");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");

        let sig = git2::Signature::now("Test Author", "test@example.com").expect("sig");
        git_repo
            .commit(Some("HEAD"), &sig, &sig, "Initial commit\n\nThis is the body.", &tree, &[])
            .expect("commit");

        let store = MockStore::new();
        let repo = make_repo(dir.path());
        let config = default_config();

        let summary = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("sync_repo");

        assert_eq!(summary.commits_indexed, 1, "should index 1 commit");
        assert_eq!(summary.commits_already_indexed, 0, "should have 0 already-indexed commits");
        assert_eq!(summary.commits_filtered, 0, "should have 0 filtered commits");
        assert!(summary.errors.is_empty(), "no errors: {:?}", summary.errors);

        let commits = store.commits.lock().unwrap();
        assert_eq!(commits.len(), 1, "store should have 1 commit");
        assert_eq!(commits[0].subject, "Initial commit");
        assert_eq!(commits[0].body.as_deref(), Some("This is the body."));
        assert_eq!(commits[0].parent_count, 0);
    }

    #[test]
    fn test_ignore_rules() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Create files in both node_modules and src
        let nm_dir = dir.path().join("node_modules");
        std::fs::create_dir_all(&nm_dir).expect("create node_modules");
        std::fs::write(nm_dir.join("foo.js"), "console.log('hi');\n").expect("write foo.js");

        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src");
        std::fs::write(src_dir.join("main.rs"), "fn main() {}\n").expect("write main.rs");

        let mut index = git_repo.index().expect("get index");
        index
            .add_path(std::path::Path::new("node_modules/foo.js"))
            .expect("add node_modules/foo.js");
        index
            .add_path(std::path::Path::new("src/main.rs"))
            .expect("add src/main.rs");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");

        let sig = git2::Signature::now("Test Author", "test@example.com").expect("sig");
        git_repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "Add files",
                &tree,
                &[],
            )
            .expect("commit");

        let store = MockStore::new();
        let repo = make_repo(dir.path());
        let config = IgnoreConfig {
            path_prefixes: vec!["node_modules/".into()],
            max_patch_bytes: 1_048_576,
        };

        let summary = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("sync_repo");

        assert_eq!(summary.commits_indexed, 1, "should index 1 commit");

        let files = store.files.lock().unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();

        assert!(
            paths.contains(&"src/main.rs"),
            "src/main.rs should be present, got: {:?}",
            paths
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("node_modules/")),
            "node_modules/ paths should be ignored, got: {:?}",
            paths
        );
    }

    #[test]
    fn test_author_filter_skips_non_matching() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Commit 1: by Alice
        let file1 = dir.path().join("alice.txt");
        std::fs::write(&file1, "alice\n").expect("write alice.txt");
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("alice.txt")).expect("add alice.txt");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let time = git2::Time::new(1_000_000, 0);
        let alice_sig = git2::Signature::new("Alice", "alice@example.com", &time).expect("alice sig");
        let c1 = git_repo
            .commit(Some("HEAD"), &alice_sig, &alice_sig, "Alice commit", &tree, &[])
            .expect("alice commit");

        // Commit 2: by Bob
        let file2 = dir.path().join("bob.txt");
        std::fs::write(&file2, "bob\n").expect("write bob.txt");
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("bob.txt")).expect("add bob.txt");
        index.write().expect("write index");
        let tree_oid2 = index.write_tree().expect("write tree");
        let tree2 = git_repo.find_tree(tree_oid2).expect("find tree");
        let time2 = git2::Time::new(1_000_001, 0);
        let bob_sig = git2::Signature::new("Bob", "bob@example.com", &time2).expect("bob sig");
        let parent1 = git_repo.find_commit(c1).expect("find alice commit");
        git_repo
            .commit(Some("HEAD"), &bob_sig, &bob_sig, "Bob commit", &tree2, &[&parent1])
            .expect("bob commit");

        let store = MockStore::new();
        let mut repo = make_repo(dir.path());
        repo.author_filter = Some("alice@example.com".into());
        let config = default_config();

        let summary = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("sync_repo");

        assert_eq!(summary.commits_indexed, 1, "should index 1 commit (Alice only)");
        assert_eq!(summary.commits_filtered, 1, "should filter 1 commit (Bob)");

        let commits = store.commits.lock().unwrap();
        assert_eq!(commits.len(), 1, "store should have 1 commit");
        assert_eq!(commits[0].author_email, "alice@example.com");
    }

    #[test]
    fn test_exclude_prefixes_from_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Create files in src/ and generated/
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src");
        std::fs::write(src_dir.join("main.rs"), "fn main() {}\n").expect("write main.rs");

        let gen_dir = dir.path().join("generated");
        std::fs::create_dir_all(&gen_dir).expect("create generated");
        std::fs::write(gen_dir.join("api.rs"), "// generated\n").expect("write api.rs");

        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("src/main.rs")).expect("add src/main.rs");
        index.add_path(std::path::Path::new("generated/api.rs")).expect("add generated/api.rs");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");

        let sig = git2::Signature::now("Test Author", "test@example.com").expect("sig");
        git_repo
            .commit(Some("HEAD"), &sig, &sig, "Add files", &tree, &[])
            .expect("commit");

        let store = MockStore::new();
        let mut repo = make_repo(dir.path());
        repo.exclude_prefixes = vec!["generated/".into()];
        let config = default_config();

        let summary = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("sync_repo");

        assert_eq!(summary.commits_indexed, 1, "should index 1 commit");

        let files = store.files.lock().unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();

        assert!(
            paths.contains(&"src/main.rs"),
            "src/main.rs should be present, got: {:?}",
            paths
        );
        assert!(
            !paths.iter().any(|p| p.starts_with("generated/")),
            "generated/ paths should be excluded, got: {:?}",
            paths
        );
    }

    #[test]
    fn test_incremental_skip_already_indexed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Commit 1
        let file1 = dir.path().join("file1.txt");
        std::fs::write(&file1, "first\n").expect("write file1");
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("file1.txt")).expect("add file1");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let sig = git2::Signature::now("Test", "test@example.com").expect("sig");
        let c1 = git_repo
            .commit(Some("HEAD"), &sig, &sig, "First commit", &tree, &[])
            .expect("first commit");

        // Commit 2
        let file2 = dir.path().join("file2.txt");
        std::fs::write(&file2, "second\n").expect("write file2");
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("file2.txt")).expect("add file2");
        index.write().expect("write index");
        let tree_oid2 = index.write_tree().expect("write tree");
        let tree2 = git_repo.find_tree(tree_oid2).expect("find tree");
        let parent1 = git_repo.find_commit(c1).expect("find c1");
        git_repo
            .commit(Some("HEAD"), &sig, &sig, "Second commit", &tree2, &[&parent1])
            .expect("second commit");

        let store = MockStore::new();
        let repo = make_repo(dir.path());
        let config = default_config();

        // First run: both commits indexed
        let summary1 = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("sync_repo first run");
        assert_eq!(summary1.commits_indexed, 2, "first run: 2 commits indexed");
        assert_eq!(summary1.commits_already_indexed, 0, "first run: 0 already-indexed");

        // Second run: both commits already in store, both skipped
        let summary2 = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("sync_repo second run");
        assert_eq!(summary2.commits_indexed, 0, "second run: 0 indexed");
        assert_eq!(summary2.commits_already_indexed, 2, "second run: 2 already-indexed");
    }

    #[test]
    fn test_sync_summary_already_indexed_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Create two commits
        let file1 = dir.path().join("a.txt");
        std::fs::write(&file1, "a\n").expect("write a.txt");
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("a.txt")).expect("add a.txt");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let sig = git2::Signature::now("Dev", "dev@example.com").expect("sig");
        let c1 = git_repo
            .commit(Some("HEAD"), &sig, &sig, "Commit A", &tree, &[])
            .expect("commit A");

        let file2 = dir.path().join("b.txt");
        std::fs::write(&file2, "b\n").expect("write b.txt");
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("b.txt")).expect("add b.txt");
        index.write().expect("write index");
        let tree_oid2 = index.write_tree().expect("write tree");
        let tree2 = git_repo.find_tree(tree_oid2).expect("find tree");
        let parent1 = git_repo.find_commit(c1).expect("find c1");
        git_repo
            .commit(Some("HEAD"), &sig, &sig, "Commit B", &tree2, &[&parent1])
            .expect("commit B");

        let store = MockStore::new();
        let repo = make_repo(dir.path());
        let config = default_config();

        // First sync: indexes all commits
        let summary1 = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("first sync");
        assert_eq!(summary1.commits_indexed, 2, "first sync: 2 indexed");
        assert_eq!(summary1.commits_already_indexed, 0, "first sync: 0 already-indexed");
        assert_eq!(summary1.commits_filtered, 0, "first sync: 0 filtered");

        // Second sync: all commits already indexed
        let summary2 = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("second sync");
        assert!(summary2.commits_already_indexed > 0, "second sync: some already-indexed");
        assert_eq!(summary2.commits_filtered, 0, "second sync: 0 filtered");
    }

    #[test]
    fn test_sync_summary_filtered_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let git_repo = git2::Repository::init(dir.path()).expect("git init");

        // Commit by one author
        let file1 = dir.path().join("x.txt");
        std::fs::write(&file1, "x\n").expect("write x.txt");
        let mut index = git_repo.index().expect("get index");
        index.add_path(std::path::Path::new("x.txt")).expect("add x.txt");
        index.write().expect("write index");
        let tree_oid = index.write_tree().expect("write tree");
        let tree = git_repo.find_tree(tree_oid).expect("find tree");
        let sig = git2::Signature::now("Other Author", "other@example.com").expect("sig");
        git_repo
            .commit(Some("HEAD"), &sig, &sig, "Other commit", &tree, &[])
            .expect("commit");

        let store = MockStore::new();
        let mut repo = make_repo(dir.path());
        // Filter for a non-matching email — no commits will match
        repo.author_filter = Some("nobody@example.com".into());
        let config = default_config();

        let summary = Git2Ingester::new()
            .sync_repo(&repo, &store, &config)
            .expect("sync_repo");

        assert!(summary.commits_filtered > 0, "should have filtered commits");
        assert_eq!(summary.commits_already_indexed, 0, "should have 0 already-indexed");
        assert_eq!(summary.commits_indexed, 0, "should have 0 indexed");
    }
}
