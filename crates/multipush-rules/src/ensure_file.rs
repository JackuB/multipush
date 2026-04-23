use async_trait::async_trait;
use tracing::debug;

use multipush_core::config::{EnsureFileConfig, EnsureFileMode};
use multipush_core::model::FileChange;
use multipush_core::rule::{Remediation, Rule, RuleContext, RuleResult};

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
        format!("Ensure file {} exists", self.config.path)
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        let path = &self.config.path;
        debug!(path = path.as_str(), repo = %ctx.repo.full_name, "evaluating ensure_file rule");

        let file = ctx
            .provider
            .get_file(ctx.repo, path, &ctx.repo.default_branch)
            .await?;

        match file {
            None => {
                let changes = match self.config.content.as_deref() {
                    Some(content) => vec![FileChange {
                        path: path.clone(),
                        content: Some(content.to_string()),
                        message: format!("Create file {path}"),
                    }],
                    None => vec![],
                };

                Ok(RuleResult::Fail {
                    detail: format!("File {path} does not exist"),
                    remediation: Some(Remediation::FileChanges {
                        description: format!("Create file {path}"),
                        changes,
                    }),
                })
            }
            Some(existing) => match self.config.mode {
                EnsureFileMode::CreateIfMissing => Ok(RuleResult::Pass {
                    detail: format!("File {path} exists"),
                }),
                EnsureFileMode::ExactMatch => match self.config.content.as_deref() {
                    None => Ok(RuleResult::Pass {
                        detail: format!("File {path} exists (no expected content to match)"),
                    }),
                    Some(expected) => {
                        if existing.content == expected {
                            Ok(RuleResult::Pass {
                                detail: format!("File {path} exists with expected content"),
                            })
                        } else {
                            Ok(RuleResult::Fail {
                                detail: format!("File {path} content does not match expected"),
                                remediation: Some(Remediation::FileChanges {
                                    description: format!(
                                        "Update file {path} to match expected content"
                                    ),
                                    changes: vec![FileChange {
                                        path: path.clone(),
                                        content: Some(expected.to_string()),
                                        message: format!("Update file {path} to match policy"),
                                    }],
                                }),
                            })
                        }
                    }
                },
                EnsureFileMode::Contains => match self.config.content.as_deref() {
                    None => Ok(RuleResult::Pass {
                        detail: format!("File {path} exists (no content to check for)"),
                    }),
                    Some(expected) => {
                        if existing.content.contains(expected) {
                            Ok(RuleResult::Pass {
                                detail: format!("File {path} contains expected content"),
                            })
                        } else {
                            Ok(RuleResult::Fail {
                                detail: format!("File {path} does not contain expected content"),
                                remediation: None,
                            })
                        }
                    }
                },
            },
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

    #[tokio::test]
    async fn file_missing_no_content() {
        let provider = TestProvider::new();
        let repo = test_repo();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: "README.md".to_string(),
            content: None,
            mode: EnsureFileMode::CreateIfMissing,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("does not exist"));
                let rem = remediation.unwrap();
                match rem {
                    Remediation::FileChanges { changes, .. } => assert!(changes.is_empty()),
                    other => panic!("expected FileChanges remediation, got {other:?}"),
                }
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_missing_with_content() {
        let provider = TestProvider::new();
        let repo = test_repo();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: "LICENSE".to_string(),
            content: Some("MIT License".to_string()),
            mode: EnsureFileMode::CreateIfMissing,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("does not exist"));
                let rem = remediation.unwrap();
                match rem {
                    Remediation::FileChanges { changes, .. } => {
                        assert_eq!(changes.len(), 1);
                        assert_eq!(changes[0].path, "LICENSE");
                        assert_eq!(changes[0].content.as_deref(), Some("MIT License"));
                    }
                    other => panic!("expected FileChanges remediation, got {other:?}"),
                }
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_exists_create_if_missing() {
        let provider = TestProvider::new().with_file("README.md", "# Hello");
        let repo = test_repo();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: "README.md".to_string(),
            content: Some("different content".to_string()),
            mode: EnsureFileMode::CreateIfMissing,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

        match result {
            RuleResult::Pass { detail } => {
                assert!(detail.contains("exists"));
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_exists_exact_match_matches() {
        let provider = TestProvider::new().with_file("LICENSE", "MIT License");
        let repo = test_repo();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: "LICENSE".to_string(),
            content: Some("MIT License".to_string()),
            mode: EnsureFileMode::ExactMatch,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

        match result {
            RuleResult::Pass { detail } => {
                assert!(detail.contains("expected content"));
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_exists_exact_match_differs() {
        let provider = TestProvider::new().with_file("LICENSE", "Apache 2.0");
        let repo = test_repo();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: "LICENSE".to_string(),
            content: Some("MIT License".to_string()),
            mode: EnsureFileMode::ExactMatch,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("does not match"));
                let rem = remediation.unwrap();
                match rem {
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
    async fn file_exists_contains_present() {
        let provider = TestProvider::new().with_file(".gitignore", "target/\nnode_modules/\n");
        let repo = test_repo();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: ".gitignore".to_string(),
            content: Some("target/".to_string()),
            mode: EnsureFileMode::Contains,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

        match result {
            RuleResult::Pass { detail } => {
                assert!(detail.contains("contains expected"));
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_exists_contains_absent() {
        let provider = TestProvider::new().with_file(".gitignore", "node_modules/\n");
        let repo = test_repo();
        let rule = EnsureFileRule::new(EnsureFileConfig {
            path: ".gitignore".to_string(),
            content: Some("target/".to_string()),
            mode: EnsureFileMode::Contains,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

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
}
