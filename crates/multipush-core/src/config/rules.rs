use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleDefinition {
    EnsureFile(EnsureFileConfig),
    EnsureJsonKey(EnsureJsonKeyConfig),
    EnsureYamlKey(EnsureYamlKeyConfig),
    FileMatches(FileMatchesConfig),
    RepoSettings(RepoSettingsConfig),
    BranchProtection(BranchProtectionConfig),
    FileAbsent(FileAbsentConfig),
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnsureFileConfig {
    /// Single file path to check. Complementary with `paths`; when `paths` is
    /// set it takes precedence.
    #[serde(default)]
    pub path: Option<String>,
    /// Candidate paths to check, in priority order (any-of semantics): the rule
    /// passes if a file exists at any of them. The first entry is the canonical
    /// location used when creating the file. Useful for files GitHub accepts in
    /// multiple locations, e.g. CODEOWNERS in the repo root, `.github/`, or
    /// `docs/`.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Body to write when the file is missing. This only seeds creation — it is
    /// never enforced on existing files. When `must_equal` is set, that value
    /// governs creation instead and `default_content` must be omitted.
    #[serde(default)]
    pub default_content: Option<String>,
    /// Predicate: an existing file must contain this substring.
    #[serde(default)]
    pub must_contain: Option<String>,
    /// Predicate: an existing file must match this regular expression.
    #[serde(default)]
    pub must_match: Option<String>,
    /// Predicate: an existing file must equal this exactly; drift is overwritten
    /// as a remediation. Also used as the creation body for a missing file.
    #[serde(default)]
    pub must_equal: Option<String>,
}

impl EnsureFileConfig {
    /// The paths to check, in priority order. `paths` takes precedence over the
    /// single `path`. The first entry is the canonical location for creation.
    pub fn candidate_paths(&self) -> Vec<String> {
        if !self.paths.is_empty() {
            self.paths.clone()
        } else {
            self.path.iter().cloned().collect()
        }
    }

    /// The content to write when creating a missing file: `must_equal` if set
    /// (it is authoritative), otherwise `default_content`. `None` means the file
    /// is required to exist but the rule does not know what to put in it.
    pub fn creation_body(&self) -> Option<&str> {
        self.must_equal
            .as_deref()
            .or(self.default_content.as_deref())
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnsureJsonKeyConfig {
    pub path: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub mode: JsonKeyMode,
}

#[derive(Debug, Default, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JsonKeyMode {
    #[default]
    CheckOnly,
    Enforce,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnsureYamlKeyConfig {
    pub path: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub mode: JsonKeyMode,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileMatchesConfig {
    pub path: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequiredStatusChecksConfig {
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub contexts: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequiredPullRequestReviewsConfig {
    #[serde(default)]
    pub required_approving_review_count: u32,
    #[serde(default)]
    pub dismiss_stale_reviews: bool,
    #[serde(default)]
    pub require_code_owner_reviews: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileAbsentConfig {
    pub path: String,
}
