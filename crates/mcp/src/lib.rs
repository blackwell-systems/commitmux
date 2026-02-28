//! MCP server implementation for commitmux using manual stdio JSON-RPC transport.
//!
//! This implements the MCP protocol (2024-11-05) over stdio as newline-delimited JSON.
//! We use a manual implementation rather than the `rmcp` crate because rmcp requires
//! tokio/async which adds significant complexity. The protocol is straightforward enough
//! that a manual implementation is simpler and more reliable.

pub mod tools;

use std::io::{BufRead, Write};
use std::sync::Arc;

use commitmux_types::{SearchOpts, Store, TouchOpts};
use serde_json::{json, Value};
use tools::{GetCommitInput, GetPatchInput, SearchInput, TouchesInput};

/// Run the MCP server, blocking until stdin is closed.
///
/// Reads newline-delimited JSON-RPC messages from stdin, dispatches tool calls
/// to the provided store, and writes JSON-RPC responses to stdout.
pub fn run_mcp_server(store: Arc<dyn Store + 'static>) -> anyhow::Result<()> {
    let server = McpServer::new(store);
    server.run_stdio()
}

struct McpServer {
    store: Arc<dyn Store + 'static>,
}

impl McpServer {
    fn new(store: Arc<dyn Store + 'static>) -> Self {
        Self { store }
    }

    fn run_stdio(&self) -> anyhow::Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());

        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match self.handle_message(&line) {
                Some(response) => {
                    writeln!(out, "{response}")?;
                    out.flush()?;
                }
                None => {
                    // Notification — no response required
                }
            }
        }
        Ok(())
    }

    /// Handle a single JSON-RPC message. Returns `Some(response_json)` for requests,
    /// or `None` for notifications (which require no response).
    fn handle_message(&self, raw: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("commitmux-mcp: failed to parse message: {e}");
                return None;
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");

        // Notifications have no "id" field — do not respond.
        let id = id?;

        let response = match method {
            "initialize" => self.handle_initialize(&id),
            "tools/list" => self.handle_tools_list(&id),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                self.handle_tools_call(&id, &params)
            }
            other => {
                eprintln!("commitmux-mcp: unknown method: {other}");
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {other}")
                    }
                })
            }
        };

        Some(response.to_string())
    }

    fn handle_initialize(&self, id: &Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "commitmux", "version": "0.1.0" }
            }
        })
    }

    fn handle_tools_list(&self, id: &Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    {
                        "name": "commitmux_search",
                        "description": "Search commit messages and diffs across all indexed repos",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "Search query" },
                                "since": { "type": "integer", "description": "Unix timestamp lower bound" },
                                "repos": { "type": "array", "items": { "type": "string" }, "description": "Filter by repo names" },
                                "paths": { "type": "array", "items": { "type": "string" }, "description": "Filter by path substrings" },
                                "limit": { "type": "integer", "description": "Max results (default 20)" }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "commitmux_touches",
                        "description": "Find commits that touched a given path pattern",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path_glob": { "type": "string", "description": "Path substring to match" },
                                "since": { "type": "integer" },
                                "repos": { "type": "array", "items": { "type": "string" } },
                                "limit": { "type": "integer" }
                            },
                            "required": ["path_glob"]
                        }
                    },
                    {
                        "name": "commitmux_get_commit",
                        "description": "Get full details for a specific commit",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "repo": { "type": "string" },
                                "sha": { "type": "string" }
                            },
                            "required": ["repo", "sha"]
                        }
                    },
                    {
                        "name": "commitmux_get_patch",
                        "description": "Get the patch (diff) for a specific commit",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "repo": { "type": "string" },
                                "sha": { "type": "string" },
                                "max_bytes": { "type": "integer", "description": "Truncate patch to this many bytes" }
                            },
                            "required": ["repo", "sha"]
                        }
                    }
                ]
            }
        })
    }

    fn handle_tools_call(&self, id: &Value, params: &Value) -> Value {
        let name = params
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match name {
            "commitmux_search" => self.call_search(&arguments),
            "commitmux_touches" => self.call_touches(&arguments),
            "commitmux_get_commit" => self.call_get_commit(&arguments),
            "commitmux_get_patch" => self.call_get_patch(&arguments),
            other => Err(format!("Unknown tool: {other}")),
        };

        match result {
            Ok(text) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": false
                }
            }),
            Err(msg) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": msg }],
                    "isError": true
                }
            }),
        }
    }

    fn call_search(&self, arguments: &Value) -> Result<String, String> {
        let input: SearchInput = serde_json::from_value(arguments.clone())
            .map_err(|e| format!("Invalid arguments for commitmux_search: {e}"))?;

        let opts = SearchOpts {
            since: input.since,
            repos: input.repos,
            paths: input.paths,
            limit: input.limit,
        };

        self.store
            .search(&input.query, &opts)
            .map_err(|e| e.to_string())
            .and_then(|results| {
                serde_json::to_string(&results).map_err(|e| e.to_string())
            })
    }

    fn call_touches(&self, arguments: &Value) -> Result<String, String> {
        let input: TouchesInput = serde_json::from_value(arguments.clone())
            .map_err(|e| format!("Invalid arguments for commitmux_touches: {e}"))?;

        let opts = TouchOpts {
            since: input.since,
            repos: input.repos,
            limit: input.limit,
        };

        self.store
            .touches(&input.path_glob, &opts)
            .map_err(|e| e.to_string())
            .and_then(|results| {
                serde_json::to_string(&results).map_err(|e| e.to_string())
            })
    }

    fn call_get_commit(&self, arguments: &Value) -> Result<String, String> {
        let input: GetCommitInput = serde_json::from_value(arguments.clone())
            .map_err(|e| format!("Invalid arguments for commitmux_get_commit: {e}"))?;

        self.store
            .get_commit(&input.repo, &input.sha)
            .map_err(|e| e.to_string())
            .and_then(|opt| {
                let result = opt.ok_or_else(|| {
                    format!("Commit {}:{} not found", input.repo, input.sha)
                })?;
                serde_json::to_string(&result).map_err(|e| e.to_string())
            })
    }

    fn call_get_patch(&self, arguments: &Value) -> Result<String, String> {
        let input: GetPatchInput = serde_json::from_value(arguments.clone())
            .map_err(|e| format!("Invalid arguments for commitmux_get_patch: {e}"))?;

        self.store
            .get_patch(&input.repo, &input.sha, input.max_bytes)
            .map_err(|e| e.to_string())
            .and_then(|opt| {
                let result = opt.ok_or_else(|| {
                    format!("Patch {}:{} not found", input.repo, input.sha)
                })?;
                serde_json::to_string(&result).map_err(|e| e.to_string())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commitmux_types::{
        CommitDetail, PatchResult, Result as StoreResult, SearchResult, Store, TouchResult,
    };
    use commitmux_types::{
        Commit, CommitFile, CommitPatch, IngestState, Repo, RepoInput, RepoListEntry,
        RepoStats, RepoUpdate, SearchOpts, TouchOpts,
    };

    /// A minimal in-memory stub store for testing.
    struct StubStore;

    impl Store for StubStore {
        fn add_repo(&self, _input: &RepoInput) -> StoreResult<Repo> {
            unimplemented!()
        }
        fn list_repos(&self) -> StoreResult<Vec<Repo>> {
            unimplemented!()
        }
        fn get_repo_by_name(&self, _name: &str) -> StoreResult<Option<Repo>> {
            unimplemented!()
        }
        fn remove_repo(&self, _name: &str) -> StoreResult<()> {
            unimplemented!()
        }
        fn update_repo(&self, _repo_id: i64, _update: &RepoUpdate) -> StoreResult<Repo> {
            unimplemented!()
        }
        fn list_repos_with_stats(&self) -> StoreResult<Vec<RepoListEntry>> {
            unimplemented!()
        }
        fn upsert_commit(&self, _commit: &Commit) -> StoreResult<()> {
            unimplemented!()
        }
        fn upsert_commit_files(&self, _files: &[CommitFile]) -> StoreResult<()> {
            unimplemented!()
        }
        fn upsert_patch(&self, _patch: &CommitPatch) -> StoreResult<()> {
            unimplemented!()
        }
        fn get_ingest_state(&self, _repo_id: i64) -> StoreResult<Option<IngestState>> {
            unimplemented!()
        }
        fn update_ingest_state(&self, _state: &IngestState) -> StoreResult<()> {
            unimplemented!()
        }
        fn commit_exists(&self, _repo_id: i64, _sha: &str) -> StoreResult<bool> {
            unimplemented!()
        }

        fn search(&self, query: &str, _opts: &SearchOpts) -> StoreResult<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                repo: "testrepo".into(),
                sha: "abc123".into(),
                subject: format!("commit matching {query}"),
                author: "Alice".into(),
                date: 1700000000,
                matched_paths: vec!["src/lib.rs".into()],
                patch_excerpt: String::new(),
            }])
        }

        fn touches(
            &self,
            path_glob: &str,
            _opts: &TouchOpts,
        ) -> StoreResult<Vec<TouchResult>> {
            Ok(vec![TouchResult {
                repo: "testrepo".into(),
                sha: "def456".into(),
                subject: "touch commit".into(),
                date: 1700000001,
                path: path_glob.to_string(),
                status: "M".into(),
            }])
        }

        fn get_commit(
            &self,
            repo_name: &str,
            sha_prefix: &str,
        ) -> StoreResult<Option<CommitDetail>> {
            if repo_name == "testrepo" && sha_prefix == "abc123" {
                Ok(Some(CommitDetail {
                    repo: repo_name.into(),
                    sha: sha_prefix.into(),
                    subject: "test commit".into(),
                    body: None,
                    author: "Alice".into(),
                    date: 1700000000,
                    changed_files: vec![],
                }))
            } else {
                Ok(None)
            }
        }

        fn get_patch(
            &self,
            repo_name: &str,
            sha: &str,
            _max_bytes: Option<usize>,
        ) -> StoreResult<Option<PatchResult>> {
            if repo_name == "testrepo" && sha == "abc123" {
                Ok(Some(PatchResult {
                    repo: repo_name.into(),
                    sha: sha.into(),
                    patch_text: "diff --git a/src/lib.rs b/src/lib.rs\n".into(),
                }))
            } else {
                Ok(None)
            }
        }

        fn repo_stats(&self, _repo_id: i64) -> StoreResult<RepoStats> {
            unimplemented!()
        }
    }

    fn make_server() -> McpServer {
        McpServer::new(Arc::new(StubStore))
    }

    #[test]
    fn test_tools_list_response() {
        let server = make_server();
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let response_str = server
            .handle_message(request)
            .expect("tools/list must produce a response");
        let response: Value =
            serde_json::from_str(&response_str).expect("response must be valid JSON");

        let tools = response["result"]["tools"]
            .as_array()
            .expect("result.tools must be an array");

        let tool_names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();

        assert!(
            tool_names.contains(&"commitmux_search"),
            "missing commitmux_search"
        );
        assert!(
            tool_names.contains(&"commitmux_touches"),
            "missing commitmux_touches"
        );
        assert!(
            tool_names.contains(&"commitmux_get_commit"),
            "missing commitmux_get_commit"
        );
        assert!(
            tool_names.contains(&"commitmux_get_patch"),
            "missing commitmux_get_patch"
        );
        assert_eq!(tool_names.len(), 4, "must have exactly 4 tools");
    }

    #[test]
    fn test_initialize_response() {
        let server = make_server();
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let response_str = server
            .handle_message(request)
            .expect("initialize must produce a response");
        let response: Value =
            serde_json::from_str(&response_str).expect("response must be valid JSON");

        assert_eq!(
            response["result"]["protocolVersion"].as_str().unwrap(),
            "2024-11-05"
        );
        assert_eq!(
            response["result"]["serverInfo"]["name"].as_str().unwrap(),
            "commitmux"
        );
    }

    #[test]
    fn test_notification_no_response() {
        let server = make_server();
        // Notifications have no "id" field
        let notif = r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#;
        let result = server.handle_message(notif);
        assert!(result.is_none(), "notifications must not produce a response");
    }

    #[test]
    fn test_tools_call_search() {
        let server = make_server();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "commitmux_search",
                "arguments": { "query": "test", "limit": 10 }
            }
        })
        .to_string();

        let response_str = server
            .handle_message(&request)
            .expect("tools/call must produce a response");
        let response: Value =
            serde_json::from_str(&response_str).expect("valid JSON");

        assert_eq!(response["result"]["isError"], false);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("text field");
        let results: Value = serde_json::from_str(text).expect("results must be JSON");
        assert!(results.as_array().is_some());
    }

    #[test]
    fn test_tools_call_get_commit_not_found() {
        let server = make_server();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "commitmux_get_commit",
                "arguments": { "repo": "nonexistent", "sha": "000000" }
            }
        })
        .to_string();

        let response_str = server
            .handle_message(&request)
            .expect("tools/call must produce a response");
        let response: Value = serde_json::from_str(&response_str).expect("valid JSON");

        // Not found => isError: true
        assert_eq!(response["result"]["isError"], true);
    }
}
