use std::sync::atomic::Ordering;

use multipush_core::config::{
    BranchProtectionConfig, PolicyConfig, RequiredPullRequestReviewsConfig, RuleDefinition,
    TargetConfig,
};
use multipush_core::engine::executor::{execute, SettingsActionKind};
use multipush_core::engine::{evaluate, ApplyReport};
use multipush_core::formatter::{RepoOutcome, Report};
use multipush_core::model::{BranchProtection, RequiredPullRequestReviews, Severity};
use multipush_core::rule::Rule;
use multipush_core::testing::{make_repo, test_config, MockProvider};
use multipush_core::Result;

fn rules_factory(policy: &PolicyConfig) -> Result<Vec<Box<dyn Rule>>> {
    policy
        .rules
        .iter()
        .map(multipush_rules::create_rule)
        .collect()
}

fn protection(reviews: u32, enforce_admins: bool) -> BranchProtection {
    BranchProtection {
        required_status_checks: None,
        required_pull_request_reviews: Some(RequiredPullRequestReviews {
            required_approving_review_count: reviews,
            dismiss_stale_reviews: false,
            require_code_owner_reviews: false,
        }),
        enforce_admins,
        required_linear_history: false,
        allow_force_pushes: false,
        allow_deletions: false,
    }
}

fn standard_policy() -> PolicyConfig {
    PolicyConfig {
        name: "require-protection".to_string(),
        description: None,
        severity: Severity::Error,
        targets: TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![],
        },
        rules: vec![RuleDefinition::BranchProtection(BranchProtectionConfig {
            required_pull_request_reviews: Some(RequiredPullRequestReviewsConfig {
                required_approving_review_count: 1,
                ..Default::default()
            }),
            enforce_admins: Some(true),
            ..Default::default()
        })],
    }
}

async fn run_check(config_policies: Vec<PolicyConfig>, provider: &MockProvider) -> Report {
    let config = test_config(config_policies);
    evaluate(&config, provider, rules_factory, 4).await.unwrap()
}

async fn run_apply(report: &Report, provider: &MockProvider, dry_run: bool) -> ApplyReport {
    let config = test_config(vec![]);
    execute(report, &config, provider, dry_run, 10)
        .await
        .unwrap()
}

#[tokio::test]
async fn branch_protection_check_pass() {
    let repo = make_repo("org/alpha");
    let provider =
        MockProvider::new(vec![repo]).with_branch_protection("org/alpha:main", protection(1, true));

    let report = run_check(vec![standard_policy()], &provider).await;

    assert_eq!(report.summary.passing, 1);
    assert_eq!(report.summary.failing, 0);
}

#[tokio::test]
async fn branch_protection_check_fail_generates_minimal_patch() {
    // actual matches required_pull_request_reviews but enforce_admins differs
    let repo = make_repo("org/alpha");
    let provider = MockProvider::new(vec![repo])
        .with_branch_protection("org/alpha:main", protection(1, false));

    let report = run_check(vec![standard_policy()], &provider).await;

    assert_eq!(report.summary.failing, 1);
    let policy_report = &report.results[0];
    let repo_result = &policy_report.repo_results[0];
    let remediations = match &repo_result.outcome {
        RepoOutcome::Fail { remediations, .. } => remediations,
        other => panic!("expected Fail, got {other:?}"),
    };
    assert_eq!(remediations.len(), 1);
    match &remediations[0] {
        multipush_core::rule::Remediation::BranchProtection { branch, patch, .. } => {
            assert_eq!(branch, "main");
            assert_eq!(patch.enforce_admins, Some(true));
            // reviews matched, so should NOT be in patch
            assert!(patch.required_pull_request_reviews.is_none());
            // un-declared fields should stay None
            assert!(patch.required_status_checks.is_none());
            assert!(patch.required_linear_history.is_none());
        }
        other => panic!("expected BranchProtection remediation, got {other:?}"),
    }
}

#[tokio::test]
async fn branch_protection_apply_sends_put() {
    let repo = make_repo("org/alpha");
    let provider = MockProvider::new(vec![repo])
        .with_branch_protection("org/alpha:main", protection(0, false));

    let report = run_check(vec![standard_policy()], &provider).await;
    let apply_report = run_apply(&report, &provider, false).await;

    assert_eq!(
        provider
            .update_branch_protection_calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(apply_report.branch_protection_applied.len(), 1);
    let action = &apply_report.branch_protection_applied[0];
    assert_eq!(action.action, SettingsActionKind::Applied);
    assert_eq!(action.repo_name, "org/alpha");
    assert_eq!(action.branch, "main");

    let history = provider.update_branch_protection_history.lock().unwrap();
    assert_eq!(history.len(), 1);
    let (sent_repo, sent_branch, sent_patch) = &history[0];
    assert_eq!(sent_repo, "org/alpha");
    assert_eq!(sent_branch, "main");
    assert_eq!(sent_patch.enforce_admins, Some(true));
    assert_eq!(
        sent_patch
            .required_pull_request_reviews
            .as_ref()
            .unwrap()
            .required_approving_review_count,
        1
    );
}

#[tokio::test]
async fn branch_protection_apply_dry_run_no_api() {
    let repo = make_repo("org/alpha");
    let provider = MockProvider::new(vec![repo])
        .with_branch_protection("org/alpha:main", protection(0, false));

    let report = run_check(vec![standard_policy()], &provider).await;
    let apply_report = run_apply(&report, &provider, true).await;

    assert_eq!(
        provider
            .update_branch_protection_calls
            .load(Ordering::SeqCst),
        0
    );
    assert_eq!(apply_report.branch_protection_applied.len(), 1);
    assert_eq!(
        apply_report.branch_protection_applied[0].action,
        SettingsActionKind::DryRun
    );
    assert!(apply_report.branch_protection_errored.is_empty());
}
