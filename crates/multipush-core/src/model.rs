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

/// Partial update to repository-level settings. Only set fields are sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_issues: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_wiki: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_projects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_merge_commit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_squash_merge: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_rebase_merge: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_branch_on_merge: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_auto_merge: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
}

/// Required status checks for a protected branch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredStatusChecks {
    pub strict: bool,
    #[serde(default)]
    pub contexts: Vec<String>,
}

/// Required pull-request review settings for a protected branch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredPullRequestReviews {
    #[serde(default)]
    pub required_approving_review_count: u32,
    #[serde(default)]
    pub dismiss_stale_reviews: bool,
    #[serde(default)]
    pub require_code_owner_reviews: bool,
}

/// Branch protection settings for a single branch.
#[derive(Debug, Clone, Default)]
pub struct BranchProtection {
    pub required_status_checks: Option<RequiredStatusChecks>,
    pub required_pull_request_reviews: Option<RequiredPullRequestReviews>,
    pub enforce_admins: bool,
    pub required_linear_history: bool,
    pub allow_force_pushes: bool,
    pub allow_deletions: bool,
}

/// Partial update to branch protection. Only set fields are sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchProtectionPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_status_checks: Option<RequiredStatusChecks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_pull_request_reviews: Option<RequiredPullRequestReviews>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforce_admins: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_linear_history: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_force_pushes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_deletions: Option<bool>,
}

impl BranchProtectionPatch {
    /// Merge `other` into `self`. Fields set in `other` override `self`.
    pub fn merge(&mut self, other: BranchProtectionPatch) {
        if other.required_status_checks.is_some() {
            self.required_status_checks = other.required_status_checks;
        }
        if other.required_pull_request_reviews.is_some() {
            self.required_pull_request_reviews = other.required_pull_request_reviews;
        }
        if other.enforce_admins.is_some() {
            self.enforce_admins = other.enforce_admins;
        }
        if other.required_linear_history.is_some() {
            self.required_linear_history = other.required_linear_history;
        }
        if other.allow_force_pushes.is_some() {
            self.allow_force_pushes = other.allow_force_pushes;
        }
        if other.allow_deletions.is_some() {
            self.allow_deletions = other.allow_deletions;
        }
    }

    /// Returns true if no fields are set.
    pub fn is_empty(&self) -> bool {
        self.required_status_checks.is_none()
            && self.required_pull_request_reviews.is_none()
            && self.enforce_admins.is_none()
            && self.required_linear_history.is_none()
            && self.allow_force_pushes.is_none()
            && self.allow_deletions.is_none()
    }
}

impl RepoSettingsPatch {
    /// Merge `other` into `self`. Fields set in `other` override `self`.
    pub fn merge(&mut self, other: RepoSettingsPatch) {
        if other.has_issues.is_some() {
            self.has_issues = other.has_issues;
        }
        if other.has_wiki.is_some() {
            self.has_wiki = other.has_wiki;
        }
        if other.has_projects.is_some() {
            self.has_projects = other.has_projects;
        }
        if other.allow_merge_commit.is_some() {
            self.allow_merge_commit = other.allow_merge_commit;
        }
        if other.allow_squash_merge.is_some() {
            self.allow_squash_merge = other.allow_squash_merge;
        }
        if other.allow_rebase_merge.is_some() {
            self.allow_rebase_merge = other.allow_rebase_merge;
        }
        if other.delete_branch_on_merge.is_some() {
            self.delete_branch_on_merge = other.delete_branch_on_merge;
        }
        if other.allow_auto_merge.is_some() {
            self.allow_auto_merge = other.allow_auto_merge;
        }
        if other.default_branch.is_some() {
            self.default_branch = other.default_branch;
        }
    }

    /// Returns true if no fields are set.
    pub fn is_empty(&self) -> bool {
        self.has_issues.is_none()
            && self.has_wiki.is_none()
            && self.has_projects.is_none()
            && self.allow_merge_commit.is_none()
            && self.allow_squash_merge.is_none()
            && self.allow_rebase_merge.is_none()
            && self.delete_branch_on_merge.is_none()
            && self.allow_auto_merge.is_none()
            && self.default_branch.is_none()
    }
}
