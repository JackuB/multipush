use async_trait::async_trait;
use regex::Regex;
use tracing::debug;

use multipush_core::config::FileMatchesConfig;
use multipush_core::rule::{Rule, RuleContext, RuleResult};

pub struct FileMatchesRule {
    config: FileMatchesConfig,
    regex: Regex,
}

impl FileMatchesRule {
    pub fn new(config: FileMatchesConfig) -> multipush_core::Result<Self> {
        let regex = Regex::new(&config.pattern).map_err(|e| {
            multipush_core::CoreError::Config(format!(
                "Invalid regex pattern `{}`: {e}",
                config.pattern
            ))
        })?;
        Ok(Self { config, regex })
    }
}

#[async_trait]
impl Rule for FileMatchesRule {
    fn rule_type(&self) -> &str {
        "file_matches"
    }

    fn description(&self) -> String {
        format!(
            "Ensure {} matches pattern `{}`",
            self.config.path, self.config.pattern
        )
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        let path = &self.config.path;
        debug!(path = path.as_str(), pattern = self.config.pattern.as_str(), repo = %ctx.repo.full_name, "evaluating file_matches rule");

        let file = ctx
            .provider
            .get_file(ctx.repo, path, &ctx.repo.default_branch)
            .await?;

        let file = match file {
            Some(f) => f,
            None => {
                return Ok(RuleResult::Fail {
                    detail: format!("File {path} does not exist"),
                    remediation: None,
                });
            }
        };

        if self.regex.is_match(&file.content) {
            Ok(RuleResult::Pass {
                detail: format!("File {path} matches pattern `{}`", self.config.pattern),
            })
        } else {
            Ok(RuleResult::Fail {
                detail: format!(
                    "File {path} does not match pattern `{}`",
                    self.config.pattern
                ),
                remediation: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::model::{
        FileChange, FileContent, PullRequest, Repo, RepoSettings, RepoSettingsPatch, Visibility,
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
    async fn file_missing() {
        let rule = FileMatchesRule::new(FileMatchesConfig {
            path: "README.md".to_string(),
            pattern: "MIT".to_string(),
        })
        .unwrap();

        let provider = TestProvider::new();
        let repo = test_repo();
        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Fail { .. }));
    }

    #[tokio::test]
    async fn pattern_matches() {
        let rule = FileMatchesRule::new(FileMatchesConfig {
            path: "LICENSE".to_string(),
            pattern: r"MIT\s+License".to_string(),
        })
        .unwrap();

        let provider = TestProvider::new().with_file("LICENSE", "MIT License\nCopyright 2024");
        let repo = test_repo();
        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Pass { .. }));
    }

    #[tokio::test]
    async fn pattern_no_match() {
        let rule = FileMatchesRule::new(FileMatchesConfig {
            path: "LICENSE".to_string(),
            pattern: r"Apache".to_string(),
        })
        .unwrap();

        let provider = TestProvider::new().with_file("LICENSE", "MIT License");
        let repo = test_repo();
        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(
            result,
            RuleResult::Fail {
                remediation: None,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn multiline_match() {
        let rule = FileMatchesRule::new(FileMatchesConfig {
            path: "Cargo.toml".to_string(),
            pattern: r"(?s)edition.*2021".to_string(),
        })
        .unwrap();

        let provider = TestProvider::new().with_file(
            "Cargo.toml",
            "[package]\nname = \"foo\"\nedition = \"2021\"\n",
        );
        let repo = test_repo();
        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Pass { .. }));
    }

    #[test]
    fn invalid_regex() {
        let result = FileMatchesRule::new(FileMatchesConfig {
            path: "file.txt".to_string(),
            pattern: r"[invalid".to_string(),
        });
        assert!(result.is_err());
    }
}
