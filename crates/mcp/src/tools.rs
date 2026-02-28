use serde::Deserialize;

/// Input type for the `commitmux_search` tool.
#[derive(Debug, Deserialize)]
pub struct SearchInput {
    pub query: String,
    pub since: Option<i64>,
    pub repos: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    pub limit: Option<usize>,
}

/// Input type for the `commitmux_touches` tool.
#[derive(Debug, Deserialize)]
pub struct TouchesInput {
    pub path_glob: String,
    pub since: Option<i64>,
    pub repos: Option<Vec<String>>,
    pub limit: Option<usize>,
}

/// Input type for the `commitmux_get_commit` tool.
#[derive(Debug, Deserialize)]
pub struct GetCommitInput {
    pub repo: String,
    pub sha: String,
}

/// Input type for the `commitmux_get_patch` tool.
#[derive(Debug, Deserialize)]
pub struct GetPatchInput {
    pub repo: String,
    pub sha: String,
    pub max_bytes: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_input_deserialize() {
        let json = r#"{"query":"foo","limit":5}"#;
        let input: SearchInput = serde_json::from_str(json).expect("deserialize SearchInput");
        assert_eq!(input.query, "foo");
        assert_eq!(input.limit, Some(5));
        assert!(input.since.is_none());
        assert!(input.repos.is_none());
        assert!(input.paths.is_none());
    }

    #[test]
    fn test_touches_input_deserialize() {
        let json = r#"{"path_glob":"src/"}"#;
        let input: TouchesInput = serde_json::from_str(json).expect("deserialize TouchesInput");
        assert_eq!(input.path_glob, "src/");
        assert!(input.since.is_none());
        assert!(input.repos.is_none());
        assert!(input.limit.is_none());
    }
}
