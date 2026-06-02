use async_trait::async_trait;
use regex::Regex;
use tracing::debug;

use multipush_core::config::EnsureFileConfig;
use multipush_core::model::FileChange;
use multipush_core::rule::{Remediation, Rule, RuleContext, RuleResult};
use multipush_core::CoreError;

pub struct EnsureFileRule {
    config: EnsureFileConfig,
}

impl EnsureFileRule {
    pub fn new(config: EnsureFileConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Rule for EnsureFileRule {
    fn rule_type(&self) -> &str {
        "ensure_file"
    }

    fn description(&self) -> String {
        let paths = self.config.candidate_paths();
        if paths.len() == 1 {
            format!("Ensure file {} exists", paths[0])
        } else {
            format!("Ensure a file exists at one of: {}", paths.join(", "))
        }
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        let paths = self.config.candidate_paths();
        debug!(paths = ?paths, repo = %ctx.repo.full_name, "evaluating ensure_file rule");

        // Fetch every candidate path, preserving priority order.
        let mut fetched: Vec<(String, Option<String>)> = Vec::with_capacity(paths.len());
        for path in &paths {
            let file = ctx
                .provider
                .get_file(ctx.repo, path, &ctx.repo.default_branch)
                .await?;
            fetched.push((path.clone(), file.map(|f| f.content)));
        }

        // The canonical location used when creating the file as a remediation.
        let canonical = paths
            .first()
            .cloned()
            .unwrap_or_else(|| "CODEOWNERS".to_string());
        let existing: Vec<(&str, &str)> = fetched
            .iter()
            .filter_map(|(p, c)| c.as_deref().map(|content| (p.as_str(), content)))
            .collect();

        // No file at any candidate path: same outcome regardless of mode —
        // fail, and offer to create the canonical one only when content is
        // known. Without content (no `default_content` / `must_equal`), the
        // rule deliberately emits `remediation: None` — discovery without a
        // fix — so the executor reports it as non-compliant but does not try
        // to open an empty PR.
        if existing.is_empty() {
            let detail = if paths.len() == 1 {
                format!("File {} does not exist", paths[0])
            } else {
                format!("No file found at any of: {}", paths.join(", "))
            };
            let remediation = self
                .config
                .creation_body()
                .map(|body| Remediation::FileChanges {
                    description: format!("Create file {canonical}"),
                    changes: vec![FileChange {
                        path: canonical.clone(),
                        content: Some(body.to_string()),
                        message: format!("Create file {canonical}"),
                    }],
                });
            return Ok(RuleResult::Fail {
                detail,
                remediation,
            });
        }

        // At least one file exists. Apply the predicate (at most one is set;
        // enforced by config validation). With no predicate the file only has
        // to exist.
        if let Some(expected) = self.config.must_equal.as_deref() {
            if let Some((path, _)) = existing.iter().find(|(_, c)| *c == expected) {
                Ok(RuleResult::Pass {
                    detail: format!("File {path} matches required content"),
                })
            } else {
                // Exists but drifted: overwrite the highest-priority file.
                let target = existing[0].0.to_string();
                Ok(RuleResult::Fail {
                    detail: format!("File {target} content does not match required content"),
                    remediation: Some(Remediation::FileChanges {
                        description: format!("Update file {target} to required content"),
                        changes: vec![FileChange {
                            path: target.clone(),
                            content: Some(expected.to_string()),
                            message: format!("Update file {target} to match policy"),
                        }],
                    }),
                })
            }
        } else if let Some(needle) = self.config.must_contain.as_deref() {
            if let Some((path, _)) = existing.iter().find(|(_, c)| c.contains(needle)) {
                Ok(RuleResult::Pass {
                    detail: format!("File {path} contains required content"),
                })
            } else {
                // Cannot safely splice a substring into a hand-written file.
                Ok(RuleResult::Fail {
                    detail: format!(
                        "File {} does not contain required content: {needle:?}",
                        existing[0].0
                    ),
                    remediation: None,
                })
            }
        } else if let Some(pattern) = self.config.must_match.as_deref() {
            let re = Regex::new(pattern)
                .map_err(|e| CoreError::RuleEvaluation(format!("invalid must_match regex: {e}")))?;
            if let Some((path, _)) = existing.iter().find(|(_, c)| re.is_match(c)) {
                Ok(RuleResult::Pass {
                    detail: format!("File {path} matches required pattern"),
                })
            } else {
                Ok(RuleResult::Fail {
                    detail: format!(
                        "File {} does not match required pattern: /{pattern}/",
                        existing[0].0
                    ),
                    remediation: None,
                })
            }
        } else {
            Ok(RuleResult::Pass {
                detail: format!("File {} exists", existing[0].0),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::model::{
        FileContent, PullRequest, Repo, RepoSettings, RepoSettingsPatch, Visibility,
    };
    use multipush_core::provider::Provider;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct TestProvider {
        files: Mutex<HashMap<String, FileContent>>,
    }

    impl TestProvider {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }

        fn with_file(self, path: &str, content: &str) -> Self {
            self.files.lock().unwrap().insert(
                path.to_string(),
                FileContent {
                    path: path.to_string(),
                    content: content.to_string(),
                    sha: "abc123".to_string(),
                },
            );
            self
        }
    }

    #[async_trait]
    impl Provider for TestProvider {
        fn name(&self) -> &str {
            "test"
        }

        async fn list_repos(&self, _org: &str) -> multipush_core::Result<Vec<Repo>> {
            unimplemented!()
        }

        async fn get_file(
            &self,
            _repo: &Repo,
            path: &str,
            _git_ref: &str,
        ) -> multipush_core::Result<Option<FileContent>> {
            Ok(self.files.lock().unwrap().get(path).cloned())
        }

        async fn get_repo_settings(&self, _repo: &Repo) -> multipush_core::Result<RepoSettings> {
            unimplemented!()
        }

        async fn find_open_pr(
            &self,
            _repo: &Repo,
            _head_branch: &str,
        ) -> multipush_core::Result<Option<PullRequest>> {
            unimplemented!()
        }

        async fn create_pr(
            &self,
            _repo: &Repo,
            _branch: &str,
            _base: &str,
            _title: &str,
            _body: &str,
            _changes: Vec<FileChange>,
        ) -> multipush_core::Result<PullRequest> {
            unimplemented!()
        }

        async fn update_pr(
            &self,
            _repo: &Repo,
            _pr: &PullRequest,
            _changes: Vec<FileChange>,
        ) -> multipush_core::Result<PullRequest> {
            unimplemented!()
        }

        async fn update_repo_settings(
            &self,
            _repo: &Repo,
            _patch: &RepoSettingsPatch,
        ) -> multipush_core::Result<()> {
            unimplemented!()
        }

        async fn enable_auto_merge(
            &self,
            _repo: &Repo,
            _pr: &multipush_core::model::PullRequest,
        ) -> multipush_core::Result<()> {
            unimplemented!()
        }

        async fn get_branch_protection(
            &self,
            _repo: &Repo,
            _branch: &str,
        ) -> multipush_core::Result<Option<multipush_core::model::BranchProtection>> {
            unimplemented!()
        }

        async fn update_branch_protection(
            &self,
            _repo: &Repo,
            _branch: &str,
            _patch: &multipush_core::model::BranchProtectionPatch,
        ) -> multipush_core::Result<()> {
            unimplemented!()
        }
    }

    fn test_repo() -> Repo {
        Repo {
            owner: "org".to_string(),
            name: "repo".to_string(),
            full_name: "org/repo".to_string(),
            default_branch: "main".to_string(),
            archived: false,
            visibility: Visibility::Private,
            topics: vec![],
            language: None,
            custom_properties: HashMap::new(),
        }
    }

    /// Build an EnsureFileConfig with everything defaulted; tests set only the
    /// fields they exercise.
    fn cfg() -> EnsureFileConfig {
        EnsureFileConfig {
            path: None,
            paths: vec![],
            default_content: None,
            must_contain: None,
            must_match: None,
            must_equal: None,
        }
    }

    #[tokio::test]
    async fn missing_no_default_content_flags_only() {
        // Discovery-only mode: rule reports FAIL but emits no remediation
        // because we have no content to write. Executor should treat this as
        // "report only" — no branch, no PR, no max-prs budget consumed.
        let provider = TestProvider::new();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some("README.md".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("does not exist"));
                assert!(
                    remediation.is_none(),
                    "expected no remediation, got {remediation:?}"
                );
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_with_default_content_creates_it() {
        let provider = TestProvider::new();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some("LICENSE".to_string()),
            default_content: Some("MIT License".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Fail { remediation, .. } => match remediation.unwrap() {
                Remediation::FileChanges { changes, .. } => {
                    assert_eq!(changes.len(), 1);
                    assert_eq!(changes[0].path, "LICENSE");
                    // creation_body() appends a trailing newline so authors
                    // don't have to remember it in every config / recipe param.
                    assert_eq!(changes[0].content.as_deref(), Some("MIT License\n"));
                }
                other => panic!("expected FileChanges remediation, got {other:?}"),
            },
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn exists_no_predicate_passes() {
        let provider = TestProvider::new().with_file("README.md", "# Hello");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some("README.md".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Pass { detail } => assert!(detail.contains("exists")),
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn must_equal_matches_passes() {
        let provider = TestProvider::new().with_file("LICENSE", "MIT License");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some("LICENSE".to_string()),
            must_equal: Some("MIT License".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        assert!(matches!(result, RuleResult::Pass { .. }), "got {result:?}");
    }

    #[tokio::test]
    async fn must_equal_differs_remediates() {
        let provider = TestProvider::new().with_file("LICENSE", "Apache 2.0");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some("LICENSE".to_string()),
            must_equal: Some("MIT License".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("does not match"));
                match remediation.unwrap() {
                    Remediation::FileChanges { changes, .. } => {
                        assert_eq!(changes.len(), 1);
                        assert_eq!(changes[0].content.as_deref(), Some("MIT License"));
                    }
                    other => panic!("expected FileChanges remediation, got {other:?}"),
                }
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn must_contain_present_passes() {
        let provider = TestProvider::new().with_file(".gitignore", "target/\nnode_modules/\n");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some(".gitignore".to_string()),
            must_contain: Some("target/".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Pass { detail } => assert!(detail.contains("contains required")),
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn must_contain_absent_fails_without_remediation() {
        let provider = TestProvider::new().with_file(".gitignore", "node_modules/\n");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some(".gitignore".to_string()),
            must_contain: Some("target/".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("does not contain"));
                assert!(remediation.is_none());
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn must_match_regex() {
        let provider = TestProvider::new().with_file("CODEOWNERS", "/infra/ @waratek/ops\n");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: Some("CODEOWNERS".to_string()),
            must_match: Some(r"@waratek/ops\b".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        assert!(matches!(result, RuleResult::Pass { .. }), "got {result:?}");
    }

    #[tokio::test]
    async fn multi_path_passes_when_present_in_alt_location() {
        // File lives in .github/, not the repo root — should still pass.
        let provider = TestProvider::new().with_file(".github/CODEOWNERS", "* @team");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            paths: vec![
                "CODEOWNERS".to_string(),
                ".github/CODEOWNERS".to_string(),
                "docs/CODEOWNERS".to_string(),
            ],
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Pass { detail } => {
                assert!(detail.contains(".github/CODEOWNERS"), "got: {detail}");
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn multi_path_contains_passes_when_any_candidate_matches() {
        // CODEOWNERS lives in .github/ and mentions the team — must_contain
        // should pass against the alternate location.
        let provider =
            TestProvider::new().with_file(".github/CODEOWNERS", "* @waratek/portal @waratek/ops\n");
        let rule = EnsureFileRule::new(EnsureFileConfig {
            paths: vec![
                "CODEOWNERS".to_string(),
                ".github/CODEOWNERS".to_string(),
                "docs/CODEOWNERS".to_string(),
            ],
            default_content: Some("* @waratek/ops\n".to_string()),
            must_contain: Some("@waratek/ops".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        assert!(matches!(result, RuleResult::Pass { .. }), "got {result:?}");
    }

    #[tokio::test]
    async fn multi_path_missing_creates_canonical_from_default_content() {
        let provider = TestProvider::new();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            paths: vec![
                "CODEOWNERS".to_string(),
                ".github/CODEOWNERS".to_string(),
                "docs/CODEOWNERS".to_string(),
            ],
            default_content: Some("* @waratek/ops\n".to_string()),
            must_contain: Some("@waratek/ops".to_string()),
            ..cfg()
        });

        let result = rule
            .evaluate(&RuleContext {
                provider: &provider,
                repo: &test_repo(),
            })
            .await
            .unwrap();

        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("No file found at any of"), "got: {detail}");
                match remediation.unwrap() {
                    Remediation::FileChanges { changes, .. } => {
                        assert_eq!(changes.len(), 1);
                        // Created at the first (canonical) path, with a valid body.
                        assert_eq!(changes[0].path, "CODEOWNERS");
                        assert_eq!(changes[0].content.as_deref(), Some("* @waratek/ops\n"));
                    }
                    other => panic!("expected FileChanges remediation, got {other:?}"),
                }
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
