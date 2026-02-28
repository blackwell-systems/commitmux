use commitmux_types::{EmbedCommit, Store};

// ── EmbedConfig ────────────────────────────────────────────────────────────

pub struct EmbedConfig {
    pub model: String,
    pub endpoint: String,
}

impl EmbedConfig {
    /// Reads embed.model and embed.endpoint from store config.
    /// Falls back to "nomic-embed-text" and "http://localhost:11434/v1".
    pub fn from_store(store: &dyn Store) -> anyhow::Result<Self> {
        let model = store
            .get_config("embed.model")
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .unwrap_or_else(|| "nomic-embed-text".into());
        let endpoint = store
            .get_config("embed.endpoint")
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .unwrap_or_else(|| "http://localhost:11434/v1".into());
        Ok(Self { model, endpoint })
    }
}

// ── Embedder ───────────────────────────────────────────────────────────────

pub struct Embedder {
    client: async_openai::Client<async_openai::config::OpenAIConfig>,
    pub model: String,
}

impl Embedder {
    /// Constructs an Embedder pointing at the given endpoint with the given model.
    pub fn new(config: &EmbedConfig) -> Self {
        let openai_config = async_openai::config::OpenAIConfig::new()
            .with_api_base(&config.endpoint)
            .with_api_key("ollama"); // ignored by Ollama; required by client
        Self {
            client: async_openai::Client::with_config(openai_config),
            model: config.model.clone(),
        }
    }

    /// Calls the embedding API and returns a float vector.
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        use async_openai::types::embeddings::CreateEmbeddingRequestArgs;
        let request = CreateEmbeddingRequestArgs::default()
            .model(&self.model)
            .input(text)
            .build()?;
        let response = self.client.embeddings().create(request).await?;
        let embedding = response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))?
            .embedding;
        Ok(embedding)
    }
}

// ── EmbedSummary ───────────────────────────────────────────────────────────

pub struct EmbedSummary {
    pub embedded: usize,
    pub skipped: usize,
    pub failed: usize,
}

// ── build_embed_doc ────────────────────────────────────────────────────────

/// Builds the embedding document for a commit.
/// Format: "{subject}\n\n{body}\n\nFiles changed: {files}\n\n{patch_preview truncated}"
/// Pure function — no I/O.
pub fn build_embed_doc(commit: &EmbedCommit) -> String {
    let mut doc = commit.subject.clone();
    if let Some(ref body) = commit.body {
        if !body.trim().is_empty() {
            doc.push_str("\n\n");
            doc.push_str(body.trim());
        }
    }
    if !commit.files_changed.is_empty() {
        doc.push_str("\n\nFiles changed: ");
        doc.push_str(&commit.files_changed.join(", "));
    }
    if let Some(ref preview) = commit.patch_preview {
        if !preview.trim().is_empty() {
            // Truncate to ~400 tokens (approx 1600 chars)
            let truncated = if preview.len() > 1600 {
                &preview[..1600]
            } else {
                preview.as_str()
            };
            doc.push_str("\n\n");
            doc.push_str(truncated);
        }
    }
    doc
}

// ── embed_pending ──────────────────────────────────────────────────────────

/// Embeds all commits without embeddings for a repo.
/// Fetches in batches of `batch_size`. On per-commit failures: increments `failed`, continues.
/// Returns Err only if the store query itself fails.
pub async fn embed_pending(
    store: &dyn Store,
    embedder: &Embedder,
    repo_id: i64,
    batch_size: usize,
) -> anyhow::Result<EmbedSummary> {
    let mut summary = EmbedSummary {
        embedded: 0,
        skipped: 0,
        failed: 0,
    };

    loop {
        let batch = store
            .get_commits_without_embeddings(repo_id, batch_size)
            .map_err(|e| anyhow::anyhow!("Failed to fetch pending commits: {e}"))?;

        if batch.is_empty() {
            break;
        }

        for commit in &batch {
            let doc = build_embed_doc(commit);
            match embedder.embed(&doc).await {
                Ok(embedding) => {
                    match store.store_embedding(
                        commit.repo_id,
                        &commit.sha,
                        &commit.subject,
                        &commit.author_name,
                        &commit.repo_name,
                        commit.author_time,
                        commit.patch_preview.as_deref(),
                        &embedding,
                    ) {
                        Ok(()) => summary.embedded += 1,
                        Err(e) => {
                            eprintln!(
                                "embed: failed to store embedding for {}: {e}",
                                commit.sha
                            );
                            summary.failed += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("embed: failed to embed {}: {e}", commit.sha);
                    summary.failed += 1;
                }
            }
        }
    }

    Ok(summary)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_types::{
        EmbedCommit, Result, SearchResult, SemanticSearchOpts, Store,
    };

    // ── NullStore mock ──────────────────────────────────────────────────────
    // NOTE: This will fail to compile because Wave 1A's new methods don't exist
    // in this worktree yet. That is expected — documented as out_of_scope_build_blockers.

    struct NullStore;

    impl Store for NullStore {
        fn get_config(&self, _key: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn set_config(&self, _key: &str, _value: &str) -> Result<()> {
            Ok(())
        }
        fn get_commits_without_embeddings(
            &self,
            _repo_id: i64,
            _limit: usize,
        ) -> Result<Vec<EmbedCommit>> {
            Ok(vec![])
        }
        fn store_embedding(
            &self,
            _repo_id: i64,
            _sha: &str,
            _subject: &str,
            _author_name: &str,
            _repo_name: &str,
            _author_time: i64,
            _patch_preview: Option<&str>,
            _embedding: &[f32],
        ) -> Result<()> {
            Ok(())
        }
        fn search_semantic(
            &self,
            _embedding: &[f32],
            _opts: &SemanticSearchOpts,
        ) -> Result<Vec<SearchResult>> {
            Ok(vec![])
        }

        // All other Store trait methods — unimplemented
        fn add_repo(&self, _input: &commitmux_types::RepoInput) -> Result<commitmux_types::Repo> {
            unimplemented!()
        }
        fn list_repos(&self) -> Result<Vec<commitmux_types::Repo>> {
            unimplemented!()
        }
        fn get_repo_by_name(&self, _name: &str) -> Result<Option<commitmux_types::Repo>> {
            unimplemented!()
        }
        fn remove_repo(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }
        fn update_repo(
            &self,
            _repo_id: i64,
            _update: &commitmux_types::RepoUpdate,
        ) -> Result<commitmux_types::Repo> {
            unimplemented!()
        }
        fn list_repos_with_stats(&self) -> Result<Vec<commitmux_types::RepoListEntry>> {
            unimplemented!()
        }
        fn upsert_commit(&self, _commit: &commitmux_types::Commit) -> Result<()> {
            unimplemented!()
        }
        fn upsert_commit_files(&self, _files: &[commitmux_types::CommitFile]) -> Result<()> {
            unimplemented!()
        }
        fn upsert_patch(&self, _patch: &commitmux_types::CommitPatch) -> Result<()> {
            unimplemented!()
        }
        fn get_ingest_state(&self, _repo_id: i64) -> Result<Option<commitmux_types::IngestState>> {
            unimplemented!()
        }
        fn update_ingest_state(&self, _state: &commitmux_types::IngestState) -> Result<()> {
            unimplemented!()
        }
        fn commit_exists(&self, _repo_id: i64, _sha: &str) -> Result<bool> {
            unimplemented!()
        }
        fn search(
            &self,
            _query: &str,
            _opts: &commitmux_types::SearchOpts,
        ) -> Result<Vec<commitmux_types::SearchResult>> {
            unimplemented!()
        }
        fn touches(
            &self,
            _path_glob: &str,
            _opts: &commitmux_types::TouchOpts,
        ) -> Result<Vec<commitmux_types::TouchResult>> {
            unimplemented!()
        }
        fn get_commit(
            &self,
            _repo_name: &str,
            _sha_prefix: &str,
        ) -> Result<Option<commitmux_types::CommitDetail>> {
            unimplemented!()
        }
        fn get_patch(
            &self,
            _repo_name: &str,
            _sha: &str,
            _max_bytes: Option<usize>,
        ) -> Result<Option<commitmux_types::PatchResult>> {
            unimplemented!()
        }
        fn repo_stats(&self, _repo_id: i64) -> Result<commitmux_types::RepoStats> {
            unimplemented!()
        }
        fn count_commits_for_repo(&self, _repo_id: i64) -> Result<usize> {
            unimplemented!()
        }
    }

    // Helper to construct a minimal EmbedCommit
    fn make_embed_commit(
        subject: &str,
        body: Option<&str>,
        files: Vec<&str>,
        patch: Option<&str>,
    ) -> EmbedCommit {
        EmbedCommit {
            repo_id: 1,
            sha: "abc123".into(),
            subject: subject.into(),
            body: body.map(|s| s.into()),
            files_changed: files.into_iter().map(|s| s.into()).collect(),
            patch_preview: patch.map(|s| s.into()),
            author_name: "Test Author".into(),
            repo_name: "test-repo".into(),
            author_time: 1700000000,
        }
    }

    #[test]
    fn test_build_embed_doc_subject_only() {
        let commit = make_embed_commit("Fix the thing", None, vec![], None);
        let doc = build_embed_doc(&commit);
        assert_eq!(doc, "Fix the thing");
    }

    #[test]
    fn test_build_embed_doc_full() {
        let commit = make_embed_commit(
            "Add new feature",
            Some("This commit adds a great new feature\nthat spans multiple lines."),
            vec!["src/main.rs", "src/lib.rs"],
            Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,5 @@"),
        );
        let doc = build_embed_doc(&commit);
        assert!(doc.contains("Add new feature"), "should contain subject");
        assert!(
            doc.contains("This commit adds a great new feature"),
            "should contain body"
        );
        assert!(doc.contains("Files changed:"), "should contain files header");
        assert!(doc.contains("src/main.rs"), "should contain file path");
        assert!(doc.contains("src/lib.rs"), "should contain file path");
        assert!(
            doc.contains("--- a/src/main.rs"),
            "should contain patch content"
        );
    }

    #[test]
    fn test_build_embed_doc_truncates_patch() {
        // Create a patch_preview of 2000 chars
        let long_patch: String = "x".repeat(2000);
        let commit = make_embed_commit("Subject", None, vec![], Some(&long_patch));
        let doc = build_embed_doc(&commit);

        // The doc is "Subject\n\n" + truncated patch (max 1600 chars)
        // Find where the patch portion starts
        let prefix = "Subject\n\n";
        assert!(doc.starts_with(prefix));
        let patch_portion = &doc[prefix.len()..];
        assert!(
            patch_portion.len() <= 1600,
            "patch portion should be at most 1600 chars, got {}",
            patch_portion.len()
        );
        assert_eq!(
            patch_portion.len(),
            1600,
            "patch portion should be exactly 1600 chars (truncated)"
        );
    }

    #[test]
    fn test_embed_config_defaults() {
        let store = NullStore;
        let config = EmbedConfig::from_store(&store)
            .expect("from_store should succeed with NullStore");
        assert_eq!(config.model, "nomic-embed-text");
        assert_eq!(config.endpoint, "http://localhost:11434/v1");
    }
}
