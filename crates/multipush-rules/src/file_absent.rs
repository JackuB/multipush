use async_trait::async_trait;
use tracing::debug;

use multipush_core::config::FileAbsentConfig;
use multipush_core::model::FileChange;
use multipush_core::rule::{Remediation, Rule, RuleContext, RuleResult};

pub struct FileAbsentRule {
    config: FileAbsentConfig,
}

impl FileAbsentRule {
    pub fn new(config: FileAbsentConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Rule for FileAbsentRule {
    fn rule_type(&self) -> &str {
        "file_absent"
    }

    fn description(&self) -> String {
        format!("Ensure file {} does not exist", self.config.path)
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        let path = &self.config.path;
        debug!(path = path.as_str(), repo = %ctx.repo.full_name, "evaluating file_absent rule");

        let file = ctx
            .provider
            .get_file(ctx.repo, path, &ctx.repo.default_branch)
            .await?;

        match file {
            None => Ok(RuleResult::Pass {
                detail: format!("File {path} does not exist"),
            }),
            Some(_) => Ok(RuleResult::Fail {
                detail: format!("File {path} exists but should be absent"),
                remediation: Some(Remediation::FileChanges {
                    description: format!("Delete file {path}"),
                    changes: vec![FileChange {
                        path: path.clone(),
                        content: None,
                        message: format!("Delete file {path}"),
                    }],
                }),
            }),
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

        async fn list_autolinks(
            &self,
            _repo: &Repo,
        ) -> multipush_core::Result<Vec<multipush_core::model::Autolink>> {
            unimplemented!()
        }

        async fn create_autolink(
            &self,
            _repo: &Repo,
            _spec: &multipush_core::model::AutolinkSpec,
        ) -> multipush_core::Result<()> {
            unimplemented!()
        }

        async fn delete_autolink(&self, _repo: &Repo, _id: u64) -> multipush_core::Result<()> {
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
    async fn file_absent_passes_when_missing() {
        let provider = TestProvider::new();
        let repo = test_repo();
        let rule = FileAbsentRule::new(FileAbsentConfig {
            path: ".env".to_string(),
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();

        match result {
            RuleResult::Pass { detail } => {
                assert!(detail.contains("does not exist"));
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn file_absent_fails_when_present() {
        let provider = TestProvider::new().with_file(".env", "SECRET=abc");
        let repo = test_repo();
        let rule = FileAbsentRule::new(FileAbsentConfig {
            path: ".env".to_string(),
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
                assert!(detail.contains("exists but should be absent"));
                let rem = remediation.unwrap();
                match rem {
                    Remediation::FileChanges {
                        changes,
                        description,
                    } => {
                        assert!(description.contains("Delete"));
                        assert_eq!(changes.len(), 1);
                        assert_eq!(changes[0].path, ".env");
                        assert!(changes[0].content.is_none());
                    }
                    other => panic!("expected FileChanges remediation, got {other:?}"),
                }
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
