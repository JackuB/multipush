use futures::stream::{self, StreamExt};
use tracing::{error, info, info_span, Instrument};

use crate::config::{PolicyConfig, RootConfig};
use crate::formatter::{PolicyReport, Report, RepoOutcome, RepoResult, Summary};
use crate::model::Repo;
use crate::provider::Provider;
use crate::rule::{Rule, RuleContext, RuleResult};
use crate::Result;

use super::targeting::filter_repos;

pub async fn evaluate<F>(
    config: &RootConfig,
    provider: &dyn Provider,
    rules_factory: F,
    concurrency: usize,
) -> Result<Report>
where
    F: Fn(&PolicyConfig) -> Result<Vec<Box<dyn Rule>>>,
{
    let all_repos = provider.list_repos(&config.provider.org).await?;
    info!(count = all_repos.len(), "fetched repos");

    let mut policy_reports = Vec::new();
    let mut summary = Summary::default();

    for policy in &config.policies {
        let _policy_span = info_span!("policy", name = %policy.name).entered();
        let rules = rules_factory(policy)?;
        let targeted = filter_repos(&all_repos, &policy.targets)?;
        info!(
            policy = %policy.name,
            targeted = targeted.len(),
            rules = rules.len(),
            "evaluating policy"
        );

        let repo_results: Vec<RepoResult> = stream::iter(targeted.iter().map(|repo| {
            let rules = &rules;
            let span = info_span!("repo", name = %repo.full_name);
            async move {
                evaluate_repo(provider, repo, rules).await
            }
            .instrument(span)
        }))
        .buffer_unordered(concurrency)
        .collect()
        .await;

        for rr in &repo_results {
            summary.total_repos += 1;
            match &rr.outcome {
                RepoOutcome::Pass { .. } => summary.passing += 1,
                RepoOutcome::Fail { .. } => summary.failing += 1,
                RepoOutcome::Skip { .. } => summary.skipped += 1,
                RepoOutcome::Error { .. } => summary.errors += 1,
            }
        }

        policy_reports.push(PolicyReport {
            policy_name: policy.name.clone(),
            description: policy.description.clone(),
            severity: policy.severity,
            repo_results,
        });
    }

    Ok(Report {
        results: policy_reports,
        summary,
    })
}

async fn evaluate_repo(
    provider: &dyn Provider,
    repo: &Repo,
    rules: &[Box<dyn Rule>],
) -> RepoResult {
    let mut outcomes: Vec<RuleResult> = Vec::new();

    for rule in rules {
        let ctx = RuleContext { provider, repo };
        match rule.evaluate(&ctx).await {
            Ok(result) => outcomes.push(result),
            Err(e) => {
                error!(
                    repo = %repo.full_name,
                    rule = rule.rule_type(),
                    error = %e,
                    "rule evaluation error"
                );
                outcomes.push(RuleResult::Error {
                    message: e.to_string(),
                });
            }
        }
    }

    let outcome = aggregate_outcomes(&outcomes);
    RepoResult {
        repo_name: repo.full_name.clone(),
        default_branch: repo.default_branch.clone(),
        outcome,
    }
}

fn aggregate_outcomes(outcomes: &[RuleResult]) -> RepoOutcome {
    // Priority: errors > failures > skips > passes
    let mut has_error = false;
    let mut has_fail = false;
    let mut has_skip = false;
    let mut details = Vec::new();
    let mut remediations = Vec::new();

    for o in outcomes {
        match o {
            RuleResult::Error { message } => {
                has_error = true;
                details.push(message.clone());
            }
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                has_fail = true;
                details.push(detail.clone());
                if let Some(r) = remediation {
                    remediations.push(r.clone());
                }
            }
            RuleResult::Skip { reason } => {
                has_skip = true;
                details.push(reason.clone());
            }
            RuleResult::Pass { detail } => {
                details.push(detail.clone());
            }
        }
    }

    let combined = details.join("; ");

    if has_error {
        RepoOutcome::Error { message: combined }
    } else if has_fail {
        RepoOutcome::Fail {
            detail: combined,
            remediations,
        }
    } else if has_skip {
        RepoOutcome::Skip { reason: combined }
    } else {
        RepoOutcome::Pass { detail: combined }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        EnsureFileConfig, EnsureFileMode, ProviderConfig, ProviderType, RuleDefinition,
        TargetConfig,
    };
    use crate::model::{FileChange, FileContent, PullRequest, Repo, RepoSettings, Visibility};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockProvider {
        repos: Vec<Repo>,
        files: Mutex<HashMap<String, FileContent>>,
    }

    impl MockProvider {
        fn new(repos: Vec<Repo>) -> Self {
            Self {
                repos,
                files: Mutex::new(HashMap::new()),
            }
        }

        fn with_file(self, repo_file_key: &str, content: &str) -> Self {
            self.files.lock().unwrap().insert(
                repo_file_key.to_string(),
                FileContent {
                    path: repo_file_key.to_string(),
                    content: content.to_string(),
                    sha: "abc".to_string(),
                },
            );
            self
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn list_repos(&self, _org: &str) -> Result<Vec<Repo>> {
            Ok(self.repos.clone())
        }

        async fn get_file(
            &self,
            repo: &Repo,
            path: &str,
            _git_ref: &str,
        ) -> Result<Option<FileContent>> {
            let key = format!("{}:{}", repo.full_name, path);
            Ok(self.files.lock().unwrap().get(&key).cloned())
        }

        async fn get_repo_settings(&self, _repo: &Repo) -> Result<RepoSettings> {
            unimplemented!()
        }

        async fn find_open_pr(
            &self,
            _repo: &Repo,
            _head: &str,
        ) -> Result<Option<PullRequest>> {
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
        ) -> Result<PullRequest> {
            unimplemented!()
        }

        async fn update_pr(
            &self,
            _repo: &Repo,
            _pr: &PullRequest,
            _changes: Vec<FileChange>,
        ) -> Result<PullRequest> {
            unimplemented!()
        }
    }

    fn make_repo(full_name: &str) -> Repo {
        let parts: Vec<&str> = full_name.splitn(2, '/').collect();
        Repo {
            owner: parts[0].to_string(),
            name: parts.get(1).unwrap_or(&"").to_string(),
            full_name: full_name.to_string(),
            default_branch: "main".to_string(),
            archived: false,
            visibility: Visibility::Private,
            topics: vec![],
            language: None,
            custom_properties: HashMap::new(),
        }
    }

    fn test_config(policies: Vec<PolicyConfig>) -> RootConfig {
        RootConfig {
            provider: ProviderConfig {
                provider_type: ProviderType::Github,
                org: "org".to_string(),
                token: "ghp_test".to_string(),
                base_url: None,
            },
            defaults: None,
            policies,
        }
    }

    fn simple_factory(policy: &PolicyConfig) -> Result<Vec<Box<dyn Rule>>> {
        let mut rules: Vec<Box<dyn Rule>> = Vec::new();
        for def in &policy.rules {
            match def {
                RuleDefinition::EnsureFile(cfg) => {
                    rules.push(Box::new(SimpleEnsureFileRule {
                        path: cfg.path.clone(),
                    }));
                }
                _ => {}
            }
        }
        Ok(rules)
    }

    struct SimpleEnsureFileRule {
        path: String,
    }

    #[async_trait]
    impl Rule for SimpleEnsureFileRule {
        fn rule_type(&self) -> &str {
            "ensure_file"
        }

        fn description(&self) -> String {
            format!("Ensure file {}", self.path)
        }

        async fn evaluate(&self, ctx: &RuleContext<'_>) -> Result<RuleResult> {
            let file = ctx
                .provider
                .get_file(ctx.repo, &self.path, &ctx.repo.default_branch)
                .await?;
            match file {
                Some(_) => Ok(RuleResult::Pass {
                    detail: format!("File {} exists", self.path),
                }),
                None => Ok(RuleResult::Fail {
                    detail: format!("File {} does not exist", self.path),
                    remediation: None,
                }),
            }
        }
    }

    #[tokio::test]
    async fn evaluate_single_policy_pass() {
        let repos = vec![make_repo("org/alpha")];
        let provider = MockProvider::new(repos).with_file("org/alpha:README.md", "# Alpha");

        let config = test_config(vec![PolicyConfig {
            name: "require-readme".to_string(),
            description: None,
            severity: crate::model::Severity::Error,
            targets: TargetConfig {
                repos: "org/*".to_string(),
                exclude: vec![],
                exclude_archived: true,
                filters: vec![],
            },
            rules: vec![RuleDefinition::EnsureFile(EnsureFileConfig {
                path: "README.md".to_string(),
                content: None,
                mode: EnsureFileMode::CreateIfMissing,
            })],
        }]);

        let report = evaluate(&config, &provider, simple_factory, 10).await.unwrap();
        assert_eq!(report.summary.passing, 1);
        assert_eq!(report.summary.failing, 0);
    }

    #[tokio::test]
    async fn evaluate_single_policy_fail() {
        let repos = vec![make_repo("org/alpha")];
        let provider = MockProvider::new(repos);

        let config = test_config(vec![PolicyConfig {
            name: "require-readme".to_string(),
            description: None,
            severity: crate::model::Severity::Error,
            targets: TargetConfig {
                repos: "org/*".to_string(),
                exclude: vec![],
                exclude_archived: true,
                filters: vec![],
            },
            rules: vec![RuleDefinition::EnsureFile(EnsureFileConfig {
                path: "README.md".to_string(),
                content: None,
                mode: EnsureFileMode::CreateIfMissing,
            })],
        }]);

        let report = evaluate(&config, &provider, simple_factory, 10).await.unwrap();
        assert_eq!(report.summary.passing, 0);
        assert_eq!(report.summary.failing, 1);
    }

    #[test]
    fn aggregate_all_pass() {
        let outcomes = vec![
            RuleResult::Pass {
                detail: "a".into(),
            },
            RuleResult::Pass {
                detail: "b".into(),
            },
        ];
        match aggregate_outcomes(&outcomes) {
            RepoOutcome::Pass { detail } => assert!(detail.contains("a") && detail.contains("b")),
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn aggregate_fail_takes_priority() {
        let outcomes = vec![
            RuleResult::Pass {
                detail: "ok".into(),
            },
            RuleResult::Fail {
                detail: "bad".into(),
                remediation: None,
            },
        ];
        match aggregate_outcomes(&outcomes) {
            RepoOutcome::Fail { .. } => {}
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn aggregate_error_takes_priority() {
        let outcomes = vec![
            RuleResult::Fail {
                detail: "bad".into(),
                remediation: None,
            },
            RuleResult::Error {
                message: "boom".into(),
            },
        ];
        match aggregate_outcomes(&outcomes) {
            RepoOutcome::Error { .. } => {}
            other => panic!("expected Error, got {other:?}"),
        }
    }
}
