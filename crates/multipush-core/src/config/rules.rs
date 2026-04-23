use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleDefinition {
    EnsureFile(EnsureFileConfig),
    EnsureJsonKey(EnsureJsonKeyConfig),
    EnsureYamlKey(EnsureYamlKeyConfig),
    FileMatches(FileMatchesConfig),
    RepoSettings(RepoSettingsConfig),
    BranchProtection(BranchProtectionConfig),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsureFileConfig {
    pub path: String,
    pub content: Option<String>,
    #[serde(default)]
    pub mode: EnsureFileMode,
}

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnsureFileMode {
    #[default]
    CreateIfMissing,
    ExactMatch,
    Contains,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsureJsonKeyConfig {
    pub path: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub mode: JsonKeyMode,
}

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JsonKeyMode {
    #[default]
    CheckOnly,
    Enforce,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnsureYamlKeyConfig {
    pub path: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub mode: JsonKeyMode,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileMatchesConfig {
    pub path: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredStatusChecksConfig {
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub contexts: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredPullRequestReviewsConfig {
    #[serde(default)]
    pub required_approving_review_count: u32,
    #[serde(default)]
    pub dismiss_stale_reviews: bool,
    #[serde(default)]
    pub require_code_owner_reviews: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchProtectionConfig {
    /// Branch to apply protection to. If omitted, the repo's default branch is used.
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub required_status_checks: Option<RequiredStatusChecksConfig>,
    #[serde(default)]
    pub required_pull_request_reviews: Option<RequiredPullRequestReviewsConfig>,
    #[serde(default)]
    pub enforce_admins: Option<bool>,
    #[serde(default)]
    pub required_linear_history: Option<bool>,
    #[serde(default)]
    pub allow_force_pushes: Option<bool>,
    #[serde(default)]
    pub allow_deletions: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoSettingsConfig {
    #[serde(default)]
    pub has_issues: Option<bool>,
    #[serde(default)]
    pub has_wiki: Option<bool>,
    #[serde(default)]
    pub has_projects: Option<bool>,
    #[serde(default)]
    pub allow_merge_commit: Option<bool>,
    #[serde(default)]
    pub allow_squash_merge: Option<bool>,
    #[serde(default)]
    pub allow_rebase_merge: Option<bool>,
    #[serde(default)]
    pub delete_branch_on_merge: Option<bool>,
    #[serde(default)]
    pub allow_auto_merge: Option<bool>,
    #[serde(default)]
    pub default_branch: Option<String>,
}
