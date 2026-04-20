use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A repository on the hosting platform.
#[derive(Debug, Clone)]
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
    pub archived: bool,
    pub visibility: Visibility,
    pub topics: Vec<String>,
    pub language: Option<String>,
    pub custom_properties: HashMap<String, String>,
}

/// Repository visibility level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    Internal,
}

/// The contents and metadata of a single file retrieved from a repository.
#[derive(Debug, Clone)]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub sha: String,
}

/// A pull request on the hosting platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub head_branch: String,
    pub url: String,
    pub state: PrState,
}

/// Pull request lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

/// A file create/update/delete operation for a remediation PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    /// File content. `None` means delete.
    pub content: Option<String>,
    pub message: String,
}

/// Policy violation severity level.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    #[default]
    Error,
}

impl Ord for Severity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for Severity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Severity {
    fn rank(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Warning => 1,
            Self::Error => 2,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            other => Err(format!("unknown severity: {other}")),
        }
    }
}

/// Repository-level settings (merge strategies, enabled features, etc.).
#[derive(Debug, Clone)]
pub struct RepoSettings {
    pub has_issues: bool,
    pub has_wiki: bool,
    pub has_projects: bool,
    pub allow_merge_commit: bool,
    pub allow_squash_merge: bool,
    pub allow_rebase_merge: bool,
    pub delete_branch_on_merge: bool,
    pub default_branch: String,
    pub allow_auto_merge: bool,
}
