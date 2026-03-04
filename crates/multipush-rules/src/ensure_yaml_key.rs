use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use multipush_core::config::{EnsureYamlKeyConfig, JsonKeyMode};
use multipush_core::model::FileChange;
use multipush_core::rule::{Remediation, Rule, RuleContext, RuleResult};

use crate::key_path;

pub struct EnsureYamlKeyRule {
    config: EnsureYamlKeyConfig,
}

impl EnsureYamlKeyRule {
    pub fn new(config: EnsureYamlKeyConfig) -> Self {
        Self { config }
    }

    fn build_remediation(&self, mut root: Value) -> Option<Remediation> {
        if self.config.mode != JsonKeyMode::Enforce {
            return None;
        }
        let value = self.config.value.as_ref()?;
        if !key_path::set_by_path(&mut root, &self.config.key, value.clone()) {
            return None;
        }
        let new_content = serde_yaml_ng::to_string(&root).ok()?;
        Some(Remediation {
            description: format!(
                "Set key `{}` in {} to expected value",
                self.config.key, self.config.path
            ),
            changes: vec![FileChange {
                path: self.config.path.clone(),
                content: Some(new_content),
                message: format!(
                    "Set key `{}` in {} to expected value",
                    self.config.key, self.config.path
                ),
            }],
        })
    }
}

#[async_trait]
impl Rule for EnsureYamlKeyRule {
    fn rule_type(&self) -> &str {
        "ensure_yaml_key"
    }

    fn description(&self) -> String {
        format!(
            "Ensure key `{}` in {}",
            self.config.key, self.config.path
        )
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        let path = &self.config.path;
        let key = &self.config.key;
        debug!(path = path.as_str(), key = key.as_str(), repo = %ctx.repo.full_name, "evaluating ensure_yaml_key rule");

        let file = ctx
            .provider
            .get_file(ctx.repo, path, &ctx.repo.default_branch)
            .await?;

        let file = match file {
            Some(f) => f,
            None => {
                let remediation = self.build_remediation(Value::Object(serde_json::Map::new()));
                return Ok(RuleResult::Fail {
                    detail: format!("File {path} does not exist"),
                    remediation,
                });
            }
        };

        let root: Value = match serde_yaml_ng::from_str(&file.content) {
            Ok(v) => v,
            Err(e) => {
                return Ok(RuleResult::Fail {
                    detail: format!("File {path} is not valid YAML: {e}"),
                    remediation: None,
                });
            }
        };

        match key_path::get_by_path(&root, key) {
            None => {
                let remediation = self.build_remediation(root);
                Ok(RuleResult::Fail {
                    detail: format!("Key `{key}` not found in {path}"),
                    remediation,
                })
            }
            Some(actual) => {
                match &self.config.value {
                    None => Ok(RuleResult::Pass {
                        detail: format!("Key `{key}` exists in {path}"),
                    }),
                    Some(expected) => {
                        if actual == expected {
                            Ok(RuleResult::Pass {
                                detail: format!("Key `{key}` in {path} has expected value"),
                            })
                        } else {
                            let remediation = self.build_remediation(root);
                            Ok(RuleResult::Fail {
                                detail: format!("Key `{key}` in {path} has unexpected value"),
                                remediation,
                            })
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::model::{FileContent, PullRequest, Repo, RepoSettings, Visibility};
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

        async fn get_repo_settings(
            &self,
            _repo: &Repo,
        ) -> multipush_core::Result<RepoSettings> {
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
    async fn file_missing_check_only() {
        let provider = TestProvider::new();
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "config.yml".to_string(),
            key: "version".to_string(),
            value: Some(serde_json::json!("1.0")),
            mode: JsonKeyMode::CheckOnly,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Fail { remediation: None, .. }));
    }

    #[tokio::test]
    async fn file_missing_enforce() {
        let provider = TestProvider::new();
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "config.yml".to_string(),
            key: "version".to_string(),
            value: Some(serde_json::json!("1.0")),
            mode: JsonKeyMode::Enforce,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { remediation, .. } => {
                let rem = remediation.unwrap();
                assert_eq!(rem.changes.len(), 1);
                let content = rem.changes[0].content.as_deref().unwrap();
                let parsed: Value = serde_yaml_ng::from_str(content).unwrap();
                assert_eq!(parsed["version"], serde_json::json!("1.0"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn key_present_value_matches() {
        let provider = TestProvider::new()
            .with_file("config.yml", "name: my-app\nversion: '1.0'\n");
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "config.yml".to_string(),
            key: "name".to_string(),
            value: Some(serde_json::json!("my-app")),
            mode: JsonKeyMode::CheckOnly,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Pass { .. }));
    }

    #[tokio::test]
    async fn key_present_value_mismatch_enforce() {
        let provider = TestProvider::new()
            .with_file("config.yml", "name: wrong\n");
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "config.yml".to_string(),
            key: "name".to_string(),
            value: Some(serde_json::json!("expected")),
            mode: JsonKeyMode::Enforce,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { remediation, .. } => {
                let rem = remediation.unwrap();
                let content = rem.changes[0].content.as_deref().unwrap();
                let parsed: Value = serde_yaml_ng::from_str(content).unwrap();
                assert_eq!(parsed["name"], serde_json::json!("expected"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn key_missing_enforce() {
        let provider = TestProvider::new()
            .with_file("config.yml", "existing: value\n");
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "config.yml".to_string(),
            key: "name".to_string(),
            value: Some(serde_json::json!("my-app")),
            mode: JsonKeyMode::Enforce,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { remediation, .. } => {
                let rem = remediation.unwrap();
                let content = rem.changes[0].content.as_deref().unwrap();
                let parsed: Value = serde_yaml_ng::from_str(content).unwrap();
                assert_eq!(parsed["name"], serde_json::json!("my-app"));
                assert_eq!(parsed["existing"], serde_json::json!("value"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn nested_key() {
        let provider = TestProvider::new()
            .with_file("config.yml", "a:\n  b:\n    c: 42\n");
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "config.yml".to_string(),
            key: "a.b.c".to_string(),
            value: Some(serde_json::json!(42)),
            mode: JsonKeyMode::CheckOnly,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Pass { .. }));
    }

    #[tokio::test]
    async fn invalid_yaml() {
        let provider = TestProvider::new()
            .with_file("bad.yml", ":\n  - :\n  bad: [");
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "bad.yml".to_string(),
            key: "name".to_string(),
            value: None,
            mode: JsonKeyMode::CheckOnly,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { detail, remediation } => {
                assert!(detail.contains("not valid YAML"));
                assert!(remediation.is_none());
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn key_exists_no_expected_value() {
        let provider = TestProvider::new()
            .with_file("config.yml", "name: anything\n");
        let repo = test_repo();
        let rule = EnsureYamlKeyRule::new(EnsureYamlKeyConfig {
            path: "config.yml".to_string(),
            key: "name".to_string(),
            value: None,
            mode: JsonKeyMode::CheckOnly,
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Pass { .. }));
    }
}
