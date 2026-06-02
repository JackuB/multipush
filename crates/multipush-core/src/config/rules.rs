use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

/// Treat an empty or whitespace-only string as `None`. Recipes substitute
/// missing/blank params as empty strings (e.g. `must_contain: ""`), and we
/// don't want that to mean "the file must contain the empty string" — which
/// is trivially true and silently disables the predicate. Same logic for
/// `default_content`: an empty body is never a meaningful canonical file.
fn empty_string_as_none<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(de)?;
    Ok(opt.filter(|s| !s.trim().is_empty()))
}

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
    ///
    /// Empty or whitespace-only strings are normalized to `None` so recipes
    /// can leave the field unsubstituted without accidentally requesting
    /// creation of a blank file.
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub default_content: Option<String>,
    /// Predicate: an existing file must contain this substring.
    ///
    /// Empty or whitespace-only strings are normalized to `None` (any file
    /// trivially "contains" the empty string, so substituting an unset recipe
    /// param into this field must not silently turn the check off).
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub must_contain: Option<String>,
    /// Predicate: an existing file must match this regular expression.
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub must_match: Option<String>,
    /// Predicate: an existing file must equal this exactly; drift is overwritten
    /// as a remediation. Also used as the creation body for a missing file.
    #[serde(default, deserialize_with = "empty_string_as_none")]
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

    /// The content to write when creating a missing file. `None` means the
    /// file is required to exist but the rule does not know what to put in it.
    ///
    /// Source precedence:
    /// - `must_equal` is authoritative — returned verbatim. Mutating it would
    ///   break the equality predicate (`exists && content == must_equal`),
    ///   causing an infinite create/overwrite loop on subsequent runs.
    /// - `default_content` is normalized to end with exactly one `\n`. Two
    ///   reasons this matters:
    ///   1. Recipe template substitution inlines a user-supplied `\n` into
    ///      the YAML source as a raw newline. Inside a double-quoted YAML
    ///      string, YAML folds that into a space, so `"* @team\n"` becomes
    ///      `"* @team "` after roundtrip. Trimming + appending fixes that.
    ///   2. Humans often forget the trailing newline in YAML; text files
    ///      should always have one (POSIX convention, git happiness).
    pub fn creation_body(&self) -> Option<Cow<'_, str>> {
        if let Some(eq) = self.must_equal.as_deref() {
            return Some(Cow::Borrowed(eq));
        }
        self.default_content.as_deref().map(|s| {
            let trimmed = s.trim_end_matches(['\n', '\r', ' ', '\t']);
            Cow::Owned(format!("{trimmed}\n"))
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Recipe expansion leaves unsubstituted params as empty strings in the
    /// rendered YAML (e.g. `must_contain: ""`). Those must round-trip into
    /// `Option<String>::None`, not `Some("")` — otherwise predicates like
    /// `must_contain: ""` silently pass (every file "contains" empty) and
    /// `default_content: ""` would write blank files.
    #[test]
    fn empty_strings_normalize_to_none_on_ensure_file_config() {
        let yaml = r#"
path: CODEOWNERS
default_content: ""
must_contain: ""
must_match: "   "
"#;
        let cfg: EnsureFileConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert!(cfg.default_content.is_none());
        assert!(cfg.must_contain.is_none());
        assert!(cfg.must_match.is_none());
    }

    #[test]
    fn non_empty_strings_are_preserved() {
        let yaml = r#"
path: CODEOWNERS
default_content: "* @team\n"
must_contain: "@team"
"#;
        let cfg: EnsureFileConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(cfg.default_content.as_deref(), Some("* @team\n"));
        assert_eq!(cfg.must_contain.as_deref(), Some("@team"));
    }

    #[test]
    fn creation_body_appends_newline_when_missing() {
        let cfg = EnsureFileConfig {
            path: Some("CODEOWNERS".into()),
            paths: vec![],
            default_content: Some("* @team".to_string()),
            must_contain: None,
            must_match: None,
            must_equal: None,
        };
        assert_eq!(cfg.creation_body().as_deref(), Some("* @team\n"));
    }

    #[test]
    fn creation_body_collapses_trailing_whitespace_to_single_newline() {
        // Recipe substitution can fold the user's literal newline into a
        // space inside a double-quoted YAML string ("* @team\n" → "* @team ").
        // Also covers double-newlines and mixed trailing whitespace.
        let cases = [
            ("* @team ", "* @team\n"),
            ("* @team\n", "* @team\n"),
            ("* @team\n\n", "* @team\n"),
            ("* @team \t\n", "* @team\n"),
        ];
        for (input, expected) in cases {
            let cfg = EnsureFileConfig {
                path: Some("CODEOWNERS".into()),
                paths: vec![],
                default_content: Some(input.to_string()),
                must_contain: None,
                must_match: None,
                must_equal: None,
            };
            assert_eq!(
                cfg.creation_body().as_deref(),
                Some(expected),
                "input was {input:?}"
            );
        }
    }

    #[test]
    fn creation_body_uses_must_equal_verbatim() {
        // `must_equal` participates in the equality predicate; mutating the
        // creation body would cause subsequent runs to detect drift and
        // overwrite forever.
        let cfg = EnsureFileConfig {
            path: Some("foo".into()),
            paths: vec![],
            default_content: None,
            must_contain: None,
            must_match: None,
            must_equal: Some("exactly this, no trailing newline".to_string()),
        };
        assert_eq!(
            cfg.creation_body().as_deref(),
            Some("exactly this, no trailing newline")
        );
    }
}
