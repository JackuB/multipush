use std::collections::HashMap;

use tracing::{debug, info, warn};

use crate::config::{ExistingPrStrategy, RootConfig};
use crate::formatter::{Report, RepoOutcome};
use crate::model::{FileChange, PullRequest, Repo, Visibility};
use crate::provider::Provider;
use crate::rule::Remediation;
use crate::Result;

#[derive(Debug)]
pub struct ApplyReport {
    pub report: Report,
    pub prs_created: Vec<PrAction>,
    pub prs_updated: Vec<PrAction>,
    pub prs_skipped: Vec<PrAction>,
    pub prs_limited: usize,
}

#[derive(Debug)]
pub struct PrAction {
    pub repo_name: String,
    pub policy_name: String,
    pub branch: String,
    pub pr: Option<PullRequest>,
    pub action: PrActionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrActionKind {
    Created,
    Updated,
    Skipped,
    DryRun,
}

pub async fn execute(
    report: &Report,
    config: &RootConfig,
    provider: &dyn Provider,
    dry_run: bool,
    max_prs: usize,
) -> Result<ApplyReport> {
    let apply_config = config
        .defaults
        .as_ref()
        .and_then(|d| d.apply.as_ref());

    let pr_prefix = apply_config
        .map(|a| a.pr_prefix.as_str())
        .unwrap_or("multipush");

    let existing_pr_strategy = apply_config
        .map(|a| a.existing_pr)
        .unwrap_or_default();

    let mut prs_created = Vec::new();
    let mut prs_updated = Vec::new();
    let mut prs_skipped = Vec::new();
    let mut pr_counter: usize = 0;
    let mut prs_limited: usize = 0;

    for policy_report in &report.results {
        let policy_name = &policy_report.policy_name;

        for repo_result in &policy_report.repo_results {
            let remediations = match &repo_result.outcome {
                RepoOutcome::Fail { remediations, .. } if !remediations.is_empty() => {
                    remediations
                }
                _ => continue,
            };

            let branch = format!("{pr_prefix}/{policy_name}");
            let repo = build_repo(&repo_result.repo_name, &repo_result.default_branch);

            debug!(
                repo = %repo.full_name,
                policy = %policy_name,
                branch = %branch,
                remediations = remediations.len(),
                "processing failing repo"
            );

            let existing = provider.find_open_pr(&repo, &branch).await?;

            if let Some(ref pr) = existing {
                match existing_pr_strategy {
                    ExistingPrStrategy::Skip => {
                        info!(
                            repo = %repo.full_name,
                            pr = pr.number,
                            "skipping — PR already exists"
                        );
                        prs_skipped.push(PrAction {
                            repo_name: repo.full_name.clone(),
                            policy_name: policy_name.clone(),
                            branch: branch.clone(),
                            pr: Some(pr.clone()),
                            action: PrActionKind::Skipped,
                        });
                        continue;
                    }
                    ExistingPrStrategy::Update => {
                        if dry_run {
                            info!(
                                repo = %repo.full_name,
                                pr = pr.number,
                                "[dry-run] would update existing PR"
                            );
                            prs_updated.push(PrAction {
                                repo_name: repo.full_name.clone(),
                                policy_name: policy_name.clone(),
                                branch: branch.clone(),
                                pr: Some(pr.clone()),
                                action: PrActionKind::DryRun,
                            });
                        } else {
                            let changes = collect_changes(remediations);
                            let updated_pr = provider.update_pr(&repo, pr, changes).await?;
                            info!(
                                repo = %repo.full_name,
                                pr = updated_pr.number,
                                "updated existing PR"
                            );
                            prs_updated.push(PrAction {
                                repo_name: repo.full_name.clone(),
                                policy_name: policy_name.clone(),
                                branch: branch.clone(),
                                pr: Some(updated_pr),
                                action: PrActionKind::Updated,
                            });
                        }
                        continue;
                    }
                    ExistingPrStrategy::Recreate => {
                        // Treat as if no PR exists — fall through to create
                    }
                }
            }

            // Create new PR
            if pr_counter >= max_prs {
                warn!(
                    repo = %repo.full_name,
                    policy = %policy_name,
                    "PR limit reached, skipping"
                );
                prs_limited += 1;
                continue;
            }

            let title = format!("policy({policy_name}): apply remediations");
            let body = generate_pr_body(
                policy_name,
                policy_report.description.as_deref(),
                &policy_report.severity,
                &repo.full_name,
                remediations,
            );

            if dry_run {
                info!(
                    repo = %repo.full_name,
                    branch = %branch,
                    "[dry-run] would create PR: {title}"
                );
                prs_created.push(PrAction {
                    repo_name: repo.full_name.clone(),
                    policy_name: policy_name.clone(),
                    branch: branch.clone(),
                    pr: None,
                    action: PrActionKind::DryRun,
                });
            } else {
                let changes = collect_changes(remediations);
                let pr = provider
                    .create_pr(
                        &repo,
                        &branch,
                        &repo_result.default_branch,
                        &title,
                        &body,
                        changes,
                    )
                    .await?;
                info!(
                    repo = %repo.full_name,
                    pr = pr.number,
                    url = %pr.url,
                    "created PR"
                );
                prs_created.push(PrAction {
                    repo_name: repo.full_name.clone(),
                    policy_name: policy_name.clone(),
                    branch: branch.clone(),
                    pr: Some(pr),
                    action: PrActionKind::Created,
                });
            }

            pr_counter += 1;
        }
    }

    Ok(ApplyReport {
        report: report.clone(),
        prs_created,
        prs_updated,
        prs_skipped,
        prs_limited,
    })
}

fn build_repo(full_name: &str, default_branch: &str) -> Repo {
    let parts: Vec<&str> = full_name.splitn(2, '/').collect();
    Repo {
        owner: parts[0].to_string(),
        name: parts.get(1).unwrap_or(&"").to_string(),
        full_name: full_name.to_string(),
        default_branch: default_branch.to_string(),
        archived: false,
        visibility: Visibility::Private,
        topics: vec![],
        language: None,
        custom_properties: HashMap::new(),
    }
}

fn collect_changes(remediations: &[Remediation]) -> Vec<FileChange> {
    remediations
        .iter()
        .flat_map(|r| r.changes.clone())
        .collect()
}

fn generate_pr_body(
    policy_name: &str,
    description: Option<&str>,
    severity: &crate::model::Severity,
    repo_name: &str,
    remediations: &[Remediation],
) -> String {
    let mut body = String::new();

    body.push_str(&format!("## Policy: {policy_name}\n\n"));

    if let Some(desc) = description {
        body.push_str(&format!("{desc}\n\n"));
    }

    body.push_str(&format!("**Severity:** {severity}\n"));
    body.push_str(&format!("**Repository:** {repo_name}\n\n"));

    body.push_str("### Changes\n\n");
    for remediation in remediations {
        body.push_str(&format!("- {}\n", remediation.description));
        for change in &remediation.changes {
            let action = if change.content.is_some() {
                "create/update"
            } else {
                "delete"
            };
            body.push_str(&format!("  - `{}` ({action})\n", change.path));
        }
    }

    body.push_str("\n---\n*This PR was automatically created by [multipush](https://github.com/multipush/multipush).*\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::{PolicyReport, RepoResult, Summary};
    use crate::model::{FileContent, PullRequest, PrState, RepoSettings, Severity};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockProvider {
        repos: Vec<Repo>,
        open_prs: Mutex<HashMap<String, PullRequest>>,
        create_pr_calls: AtomicUsize,
        update_pr_calls: AtomicUsize,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                repos: vec![],
                open_prs: Mutex::new(HashMap::new()),
                create_pr_calls: AtomicUsize::new(0),
                update_pr_calls: AtomicUsize::new(0),
            }
        }

        fn with_open_pr(self, repo_branch_key: &str, pr: PullRequest) -> Self {
            self.open_prs
                .lock()
                .unwrap()
                .insert(repo_branch_key.to_string(), pr);
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
            _repo: &Repo,
            _path: &str,
            _git_ref: &str,
        ) -> Result<Option<FileContent>> {
            Ok(None)
        }

        async fn get_repo_settings(&self, _repo: &Repo) -> Result<RepoSettings> {
            unimplemented!()
        }

        async fn find_open_pr(
            &self,
            repo: &Repo,
            head: &str,
        ) -> Result<Option<PullRequest>> {
            let key = format!("{}:{}", repo.full_name, head);
            Ok(self.open_prs.lock().unwrap().get(&key).cloned())
        }

        async fn create_pr(
            &self,
            repo: &Repo,
            branch: &str,
            _base: &str,
            title: &str,
            _body: &str,
            _changes: Vec<FileChange>,
        ) -> Result<PullRequest> {
            let n = self.create_pr_calls.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            Ok(PullRequest {
                number: n,
                title: title.to_string(),
                head_branch: branch.to_string(),
                url: format!("https://github.com/{}/pull/{n}", repo.full_name),
                state: PrState::Open,
            })
        }

        async fn update_pr(
            &self,
            _repo: &Repo,
            pr: &PullRequest,
            _changes: Vec<FileChange>,
        ) -> Result<PullRequest> {
            self.update_pr_calls.fetch_add(1, Ordering::SeqCst);
            Ok(pr.clone())
        }
    }

    fn make_report_with_failures(repo_names: &[&str], with_remediations: bool) -> Report {
        let remediations = if with_remediations {
            vec![Remediation {
                description: "Create LICENSE file".to_string(),
                changes: vec![FileChange {
                    path: "LICENSE".to_string(),
                    content: Some("MIT License".to_string()),
                    message: "Add LICENSE".to_string(),
                }],
            }]
        } else {
            vec![]
        };

        let repo_results = repo_names
            .iter()
            .map(|name| RepoResult {
                repo_name: name.to_string(),
                default_branch: "main".to_string(),
                outcome: RepoOutcome::Fail {
                    detail: "Missing LICENSE".to_string(),
                    remediations: remediations.clone(),
                },
            })
            .collect();

        Report {
            results: vec![PolicyReport {
                policy_name: "require-license".to_string(),
                description: Some("All repos must have LICENSE".to_string()),
                severity: Severity::Error,
                repo_results,
            }],
            summary: Summary {
                total_repos: repo_names.len(),
                passing: 0,
                failing: repo_names.len(),
                skipped: 0,
                errors: 0,
            },
        }
    }

    fn default_config() -> RootConfig {
        use crate::config::{ProviderConfig, ProviderType};
        RootConfig {
            provider: ProviderConfig {
                provider_type: ProviderType::Github,
                org: "org".to_string(),
                token: "ghp_test".to_string(),
                base_url: None,
            },
            defaults: None,
            policies: vec![],
        }
    }

    #[tokio::test]
    async fn dry_run_records_actions_without_mutations() {
        let provider = MockProvider::new();
        let report = make_report_with_failures(&["org/alpha", "org/beta"], true);
        let config = default_config();

        let result = execute(&report, &config, &provider, true, 10).await.unwrap();

        assert_eq!(result.prs_created.len(), 2);
        assert!(result.prs_created.iter().all(|a| a.action == PrActionKind::DryRun));
        assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.update_pr_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn max_prs_limits_creation() {
        let provider = MockProvider::new();
        let report = make_report_with_failures(
            &["org/a", "org/b", "org/c", "org/d", "org/e"],
            true,
        );
        let config = default_config();

        let result = execute(&report, &config, &provider, false, 2).await.unwrap();

        assert_eq!(result.prs_created.len(), 2);
        assert_eq!(result.prs_limited, 3);
        assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn existing_pr_skip_strategy() {
        use crate::config::{ApplyConfig, DefaultsConfig, ProviderConfig, ProviderType};

        let existing_pr = PullRequest {
            number: 42,
            title: "existing".to_string(),
            head_branch: "multipush/require-license".to_string(),
            url: "https://github.com/org/alpha/pull/42".to_string(),
            state: PrState::Open,
        };

        let provider = MockProvider::new()
            .with_open_pr("org/alpha:multipush/require-license", existing_pr);

        let report = make_report_with_failures(&["org/alpha"], true);

        let config = RootConfig {
            provider: ProviderConfig {
                provider_type: ProviderType::Github,
                org: "org".to_string(),
                token: "ghp_test".to_string(),
                base_url: None,
            },
            defaults: Some(DefaultsConfig {
                targets: None,
                apply: Some(ApplyConfig {
                    pr_prefix: "multipush".to_string(),
                    commit_author: None,
                    pr_labels: vec![],
                    pr_draft: false,
                    existing_pr: ExistingPrStrategy::Skip,
                }),
            }),
            policies: vec![],
        };

        let result = execute(&report, &config, &provider, false, 10).await.unwrap();

        assert_eq!(result.prs_skipped.len(), 1);
        assert_eq!(result.prs_created.len(), 0);
        assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn existing_pr_update_strategy() {
        let existing_pr = PullRequest {
            number: 42,
            title: "existing".to_string(),
            head_branch: "multipush/require-license".to_string(),
            url: "https://github.com/org/alpha/pull/42".to_string(),
            state: PrState::Open,
        };

        let provider = MockProvider::new()
            .with_open_pr("org/alpha:multipush/require-license", existing_pr);

        let report = make_report_with_failures(&["org/alpha"], true);
        let config = default_config(); // default strategy is Update

        let result = execute(&report, &config, &provider, false, 10).await.unwrap();

        assert_eq!(result.prs_updated.len(), 1);
        assert_eq!(result.prs_created.len(), 0);
        assert_eq!(provider.update_pr_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn no_remediation_skipped() {
        let provider = MockProvider::new();
        let report = make_report_with_failures(&["org/alpha"], false);
        let config = default_config();

        let result = execute(&report, &config, &provider, false, 10).await.unwrap();

        assert_eq!(result.prs_created.len(), 0);
        assert_eq!(result.prs_limited, 0);
        assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn generate_pr_body_format() {
        let remediations = vec![Remediation {
            description: "Create LICENSE file".to_string(),
            changes: vec![FileChange {
                path: "LICENSE".to_string(),
                content: Some("MIT".to_string()),
                message: "Add LICENSE".to_string(),
            }],
        }];

        let body = generate_pr_body(
            "require-license",
            Some("All repos need a license"),
            &Severity::Error,
            "org/alpha",
            &remediations,
        );

        assert!(body.contains("## Policy: require-license"));
        assert!(body.contains("All repos need a license"));
        assert!(body.contains("**Severity:** error"));
        assert!(body.contains("**Repository:** org/alpha"));
        assert!(body.contains("Create LICENSE file"));
        assert!(body.contains("`LICENSE` (create/update)"));
        assert!(body.contains("multipush"));
    }
}
