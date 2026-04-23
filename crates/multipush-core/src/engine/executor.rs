use std::collections::HashMap;

use tracing::{debug, error, info, info_span, warn};

use crate::config::{ExistingPrStrategy, RootConfig};
use crate::formatter::{RepoOutcome, Report};
use crate::model::{
    BranchProtectionPatch, FileChange, PullRequest, Repo, RepoSettingsPatch, Visibility,
};
use crate::provider::Provider;
use crate::rule::Remediation;
use crate::Result;

/// Result of the apply phase: the original check report plus all PR actions taken.
#[derive(Debug)]
pub struct ApplyReport {
    pub report: Report,
    pub prs_created: Vec<PrAction>,
    pub prs_updated: Vec<PrAction>,
    pub prs_skipped: Vec<PrAction>,
    pub prs_errored: Vec<PrAction>,
    /// Number of PRs not created because the `max_prs` limit was reached.
    pub prs_limited: usize,
    pub settings_applied: Vec<SettingsAction>,
    pub settings_errored: Vec<SettingsAction>,
    pub branch_protection_applied: Vec<BranchProtectionAction>,
    pub branch_protection_errored: Vec<BranchProtectionAction>,
}

/// A single repo-settings update taken (or skipped) during apply.
#[derive(Debug)]
pub struct SettingsAction {
    pub repo_name: String,
    pub policy_names: Vec<String>,
    pub patch: RepoSettingsPatch,
    pub action: SettingsActionKind,
    pub error: Option<String>,
}

/// A single branch-protection update taken (or skipped) during apply.
#[derive(Debug)]
pub struct BranchProtectionAction {
    pub repo_name: String,
    pub branch: String,
    pub policy_names: Vec<String>,
    pub patch: BranchProtectionPatch,
    pub action: SettingsActionKind,
    pub error: Option<String>,
}

/// What happened to a repo-settings update for a given repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsActionKind {
    Applied,
    DryRun,
    Error,
}

/// A single PR-related action taken (or skipped) during apply.
#[derive(Debug)]
pub struct PrAction {
    pub repo_name: String,
    pub policy_name: String,
    pub branch: String,
    pub pr: Option<PullRequest>,
    pub action: PrActionKind,
    pub error: Option<String>,
}

/// What happened to a remediation PR for a given repo + policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrActionKind {
    Created,
    Updated,
    Skipped,
    DryRun,
    Error,
}

/// Execute the apply phase: open or update PRs for failing repositories.
pub async fn execute(
    report: &Report,
    config: &RootConfig,
    provider: &dyn Provider,
    dry_run: bool,
    max_prs: usize,
) -> Result<ApplyReport> {
    let apply_config = config.defaults.as_ref().and_then(|d| d.apply.as_ref());

    let pr_prefix = apply_config
        .map(|a| a.pr_prefix.as_str())
        .unwrap_or("multipush");

    let existing_pr_strategy = apply_config.map(|a| a.existing_pr).unwrap_or_default();

    let mut prs_created = Vec::new();
    let mut prs_updated = Vec::new();
    let mut prs_skipped = Vec::new();
    let mut prs_errored = Vec::new();
    let mut pr_counter: usize = 0;
    let mut prs_limited: usize = 0;

    // Aggregate repo_settings patches per repo across all policies so we can
    // issue one update_repo_settings call per repo regardless of how many
    // policies contributed to it.
    let mut settings_by_repo: HashMap<String, (Repo, RepoSettingsPatch, Vec<String>)> =
        HashMap::new();

    // Aggregate branch_protection patches per (repo, branch).
    let mut protection_by_target: HashMap<
        (String, String),
        (Repo, BranchProtectionPatch, Vec<String>),
    > = HashMap::new();

    for policy_report in &report.results {
        let policy_name = &policy_report.policy_name;

        for repo_result in &policy_report.repo_results {
            let all_remediations = match &repo_result.outcome {
                RepoOutcome::Fail { remediations, .. } if !remediations.is_empty() => remediations,
                _ => continue,
            };

            let repo = build_repo(&repo_result.repo_name, &repo_result.default_branch);

            // Collect non-file patches separately; they don't flow through
            // the PR pipeline.
            for rem in all_remediations {
                match rem {
                    Remediation::RepoSettings { patch, .. } => {
                        let entry = settings_by_repo
                            .entry(repo.full_name.clone())
                            .or_insert_with(|| {
                                (repo.clone(), RepoSettingsPatch::default(), Vec::new())
                            });
                        entry.1.merge(patch.clone());
                        if !entry.2.contains(policy_name) {
                            entry.2.push(policy_name.clone());
                        }
                    }
                    Remediation::BranchProtection { branch, patch, .. } => {
                        let entry = protection_by_target
                            .entry((repo.full_name.clone(), branch.clone()))
                            .or_insert_with(|| {
                                (repo.clone(), BranchProtectionPatch::default(), Vec::new())
                            });
                        entry.1.merge(patch.clone());
                        if !entry.2.contains(policy_name) {
                            entry.2.push(policy_name.clone());
                        }
                    }
                    Remediation::FileChanges { .. } => {}
                }
            }

            // Only run the PR flow if this (repo, policy) has file changes.
            let has_file_changes = all_remediations
                .iter()
                .any(|r| matches!(r, Remediation::FileChanges { .. }));
            if !has_file_changes {
                continue;
            }
            let remediations = all_remediations;

            let branch = format!("{pr_prefix}/{policy_name}");
            let _span =
                info_span!("apply_repo", repo = %repo.full_name, policy = %policy_name).entered();

            debug!(
                repo = %repo.full_name,
                policy = %policy_name,
                branch = %branch,
                remediations = remediations.len(),
                "processing failing repo"
            );

            let existing = match provider.find_open_pr(&repo, &branch).await {
                Ok(pr) => pr,
                Err(e) => {
                    error!(
                        repo = %repo.full_name,
                        policy = %policy_name,
                        error = %e,
                        "failed to check for existing PR"
                    );
                    prs_errored.push(PrAction {
                        repo_name: repo.full_name.clone(),
                        policy_name: policy_name.clone(),
                        branch: branch.clone(),
                        pr: None,
                        action: PrActionKind::Error,
                        error: Some(e.to_string()),
                    });
                    continue;
                }
            };

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
                            error: None,
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
                                error: None,
                            });
                        } else {
                            let changes = collect_changes(remediations);
                            match provider.update_pr(&repo, pr, changes).await {
                                Ok(updated_pr) => {
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
                                        error: None,
                                    });
                                }
                                Err(e) => {
                                    error!(
                                        repo = %repo.full_name,
                                        policy = %policy_name,
                                        error = %e,
                                        "failed to update PR"
                                    );
                                    prs_errored.push(PrAction {
                                        repo_name: repo.full_name.clone(),
                                        policy_name: policy_name.clone(),
                                        branch: branch.clone(),
                                        pr: Some(pr.clone()),
                                        action: PrActionKind::Error,
                                        error: Some(e.to_string()),
                                    });
                                }
                            }
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
                    error: None,
                });
            } else {
                let changes = collect_changes(remediations);
                match provider
                    .create_pr(
                        &repo,
                        &branch,
                        &repo_result.default_branch,
                        &title,
                        &body,
                        changes,
                    )
                    .await
                {
                    Ok(pr) => {
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
                            error: None,
                        });
                    }
                    Err(e) => {
                        error!(
                            repo = %repo.full_name,
                            policy = %policy_name,
                            error = %e,
                            "failed to create PR"
                        );
                        prs_errored.push(PrAction {
                            repo_name: repo.full_name.clone(),
                            policy_name: policy_name.clone(),
                            branch: branch.clone(),
                            pr: None,
                            action: PrActionKind::Error,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }

            pr_counter += 1;
        }
    }

    // Apply merged repo-settings patches.
    let mut settings_applied = Vec::new();
    let mut settings_errored = Vec::new();
    for (_, (repo, patch, policy_names)) in settings_by_repo {
        if patch.is_empty() {
            continue;
        }
        let _span = info_span!("apply_settings", repo = %repo.full_name).entered();

        if dry_run {
            let json = serde_json::to_string_pretty(&patch)
                .unwrap_or_else(|_| "<unserializable>".to_string());
            info!(
                repo = %repo.full_name,
                "[dry-run] would update repo settings: {json}"
            );
            settings_applied.push(SettingsAction {
                repo_name: repo.full_name.clone(),
                policy_names,
                patch,
                action: SettingsActionKind::DryRun,
                error: None,
            });
            continue;
        }

        match provider.update_repo_settings(&repo, &patch).await {
            Ok(()) => {
                info!(repo = %repo.full_name, "updated repo settings");
                settings_applied.push(SettingsAction {
                    repo_name: repo.full_name.clone(),
                    policy_names,
                    patch,
                    action: SettingsActionKind::Applied,
                    error: None,
                });
            }
            Err(e) => {
                error!(
                    repo = %repo.full_name,
                    error = %e,
                    "failed to update repo settings"
                );
                settings_errored.push(SettingsAction {
                    repo_name: repo.full_name.clone(),
                    policy_names,
                    patch,
                    action: SettingsActionKind::Error,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    // Apply merged branch-protection patches.
    let mut branch_protection_applied = Vec::new();
    let mut branch_protection_errored = Vec::new();
    for ((_repo_full_name, branch), (repo, patch, policy_names)) in protection_by_target {
        if patch.is_empty() {
            continue;
        }
        let _span = info_span!("apply_branch_protection", repo = %repo.full_name, branch = %branch)
            .entered();

        if dry_run {
            let json = serde_json::to_string_pretty(&patch)
                .unwrap_or_else(|_| "<unserializable>".to_string());
            info!(
                repo = %repo.full_name,
                branch = %branch,
                "[dry-run] would update branch protection: {json}"
            );
            branch_protection_applied.push(BranchProtectionAction {
                repo_name: repo.full_name.clone(),
                branch: branch.clone(),
                policy_names,
                patch,
                action: SettingsActionKind::DryRun,
                error: None,
            });
            continue;
        }

        match provider
            .update_branch_protection(&repo, &branch, &patch)
            .await
        {
            Ok(()) => {
                info!(repo = %repo.full_name, branch = %branch, "updated branch protection");
                branch_protection_applied.push(BranchProtectionAction {
                    repo_name: repo.full_name.clone(),
                    branch: branch.clone(),
                    policy_names,
                    patch,
                    action: SettingsActionKind::Applied,
                    error: None,
                });
            }
            Err(e) => {
                error!(
                    repo = %repo.full_name,
                    branch = %branch,
                    error = %e,
                    "failed to update branch protection"
                );
                branch_protection_errored.push(BranchProtectionAction {
                    repo_name: repo.full_name.clone(),
                    branch: branch.clone(),
                    policy_names,
                    patch,
                    action: SettingsActionKind::Error,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(ApplyReport {
        report: report.clone(),
        prs_created,
        prs_updated,
        prs_skipped,
        prs_errored,
        prs_limited,
        settings_applied,
        settings_errored,
        branch_protection_applied,
        branch_protection_errored,
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
        .flat_map(|r| match r {
            Remediation::FileChanges { changes, .. } => changes.clone(),
            Remediation::RepoSettings { .. } | Remediation::BranchProtection { .. } => Vec::new(),
        })
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
        match remediation {
            Remediation::FileChanges {
                description,
                changes,
            } => {
                body.push_str(&format!("- {description}\n"));
                for change in changes {
                    let action = if change.content.is_some() {
                        "create/update"
                    } else {
                        "delete"
                    };
                    body.push_str(&format!("  - `{}` ({action})\n", change.path));
                }
            }
            Remediation::RepoSettings { description, .. } => {
                body.push_str(&format!("- {description}\n"));
            }
            Remediation::BranchProtection { description, .. } => {
                body.push_str(&format!("- {description}\n"));
            }
        }
    }

    body.push_str("\n---\n*This PR was automatically created by [multipush](https://github.com/multipush/multipush).*\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FileContent, PrState, RepoSettings, Severity};
    use crate::testing::{default_config, make_report_with_failures, MockProvider};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn dry_run_records_actions_without_mutations() {
        let provider = MockProvider::new(vec![]);
        let report = make_report_with_failures(&["org/alpha", "org/beta"], true);
        let config = default_config();

        let result = execute(&report, &config, &provider, true, 10)
            .await
            .unwrap();

        assert_eq!(result.prs_created.len(), 2);
        assert!(result
            .prs_created
            .iter()
            .all(|a| a.action == PrActionKind::DryRun));
        assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 0);
        assert_eq!(provider.update_pr_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn max_prs_limits_creation() {
        let provider = MockProvider::new(vec![]);
        let report =
            make_report_with_failures(&["org/a", "org/b", "org/c", "org/d", "org/e"], true);
        let config = default_config();

        let result = execute(&report, &config, &provider, false, 2)
            .await
            .unwrap();

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

        let provider = MockProvider::new(vec![])
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

        let result = execute(&report, &config, &provider, false, 10)
            .await
            .unwrap();

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

        let provider = MockProvider::new(vec![])
            .with_open_pr("org/alpha:multipush/require-license", existing_pr);

        let report = make_report_with_failures(&["org/alpha"], true);
        let config = default_config(); // default strategy is Update

        let result = execute(&report, &config, &provider, false, 10)
            .await
            .unwrap();

        assert_eq!(result.prs_updated.len(), 1);
        assert_eq!(result.prs_created.len(), 0);
        assert_eq!(provider.update_pr_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn no_remediation_skipped() {
        let provider = MockProvider::new(vec![]);
        let report = make_report_with_failures(&["org/alpha"], false);
        let config = default_config();

        let result = execute(&report, &config, &provider, false, 10)
            .await
            .unwrap();

        assert_eq!(result.prs_created.len(), 0);
        assert_eq!(result.prs_limited, 0);
        assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn create_pr_error_continues_execution() {
        use crate::error::CoreError;
        use crate::model::RepoSettingsPatch;

        struct FailingMockProvider {
            fail_repo: String,
            create_pr_calls: AtomicUsize,
        }

        #[async_trait]
        impl Provider for FailingMockProvider {
            fn name(&self) -> &str {
                "mock"
            }
            async fn list_repos(&self, _org: &str) -> Result<Vec<Repo>> {
                Ok(vec![])
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
            async fn find_open_pr(&self, _repo: &Repo, _head: &str) -> Result<Option<PullRequest>> {
                Ok(None)
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
                if repo.full_name == self.fail_repo {
                    return Err(CoreError::Provider("API error".into()));
                }
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
                Ok(pr.clone())
            }
            async fn update_repo_settings(
                &self,
                _repo: &Repo,
                _patch: &RepoSettingsPatch,
            ) -> Result<()> {
                unimplemented!()
            }
            async fn get_branch_protection(
                &self,
                _repo: &Repo,
                _branch: &str,
            ) -> Result<Option<crate::model::BranchProtection>> {
                unimplemented!()
            }
            async fn update_branch_protection(
                &self,
                _repo: &Repo,
                _branch: &str,
                _patch: &crate::model::BranchProtectionPatch,
            ) -> Result<()> {
                unimplemented!()
            }
        }

        let provider = FailingMockProvider {
            fail_repo: "org/alpha".to_string(),
            create_pr_calls: AtomicUsize::new(0),
        };
        let report = make_report_with_failures(&["org/alpha", "org/beta"], true);
        let config = default_config();

        let result = execute(&report, &config, &provider, false, 10)
            .await
            .unwrap();

        assert_eq!(result.prs_errored.len(), 1);
        assert_eq!(result.prs_errored[0].repo_name, "org/alpha");
        assert!(result.prs_errored[0].error.is_some());
        assert_eq!(result.prs_created.len(), 1);
        assert_eq!(result.prs_created[0].repo_name, "org/beta");
        assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn generate_pr_body_format() {
        let remediations = vec![Remediation::FileChanges {
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
