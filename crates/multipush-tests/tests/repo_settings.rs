use std::sync::atomic::Ordering;

use multipush_core::config::{PolicyConfig, RepoSettingsConfig, RuleDefinition, TargetConfig};
use multipush_core::engine::executor::{execute, SettingsActionKind};
use multipush_core::engine::{evaluate, ApplyReport};
use multipush_core::formatter::{RepoOutcome, Report};
use multipush_core::model::{RepoSettings, Severity};
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

fn settings(has_wiki: bool, has_projects: bool, delete_branch_on_merge: bool) -> RepoSettings {
    RepoSettings {
        has_issues: true,
        has_wiki,
        has_projects,
        allow_merge_commit: true,
        allow_squash_merge: true,
        allow_rebase_merge: false,
        delete_branch_on_merge,
        default_branch: "main".to_string(),
        allow_auto_merge: false,
    }
}

fn standard_policy() -> PolicyConfig {
    PolicyConfig {
        name: "standardize".to_string(),
        description: None,
        severity: Severity::Warning,
        targets: TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![],
        },
        rules: vec![RuleDefinition::RepoSettings(RepoSettingsConfig {
            has_wiki: Some(false),
            has_projects: Some(false),
            delete_branch_on_merge: Some(true),
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
async fn repo_settings_check_pass() {
    let repo = make_repo("org/alpha");
    let provider =
        MockProvider::new(vec![repo]).with_repo_settings("org/alpha", settings(false, false, true));

    let report = run_check(vec![standard_policy()], &provider).await;

    assert_eq!(report.summary.passing, 1);
    assert_eq!(report.summary.failing, 0);
}

#[tokio::test]
async fn repo_settings_check_fail_generates_minimal_patch() {
    // actual differs in two of three declared fields (has_wiki, has_projects
    // are wrong; delete_branch_on_merge matches)
    let repo = make_repo("org/alpha");
    let provider =
        MockProvider::new(vec![repo]).with_repo_settings("org/alpha", settings(true, true, true));

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
        multipush_core::rule::Remediation::RepoSettings { patch, .. } => {
            assert_eq!(patch.has_wiki, Some(false));
            assert_eq!(patch.has_projects, Some(false));
            assert_eq!(patch.delete_branch_on_merge, None);
            // Fields we never declared must stay None.
            assert_eq!(patch.has_issues, None);
            assert_eq!(patch.allow_squash_merge, None);
        }
        other => panic!("expected RepoSettings remediation, got {other:?}"),
    }
}

#[tokio::test]
async fn repo_settings_apply_sends_patch() {
    let repo = make_repo("org/alpha");
    let provider =
        MockProvider::new(vec![repo]).with_repo_settings("org/alpha", settings(true, true, false));

    let report = run_check(vec![standard_policy()], &provider).await;
    let apply_report = run_apply(&report, &provider, false).await;

    assert_eq!(
        provider.update_repo_settings_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(apply_report.settings_applied.len(), 1);
    let action = &apply_report.settings_applied[0];
    assert_eq!(action.action, SettingsActionKind::Applied);
    assert_eq!(action.repo_name, "org/alpha");

    let history = provider.update_repo_settings_history.lock().unwrap();
    assert_eq!(history.len(), 1);
    let (sent_repo, sent_patch) = &history[0];
    assert_eq!(sent_repo, "org/alpha");
    assert_eq!(sent_patch.has_wiki, Some(false));
    assert_eq!(sent_patch.has_projects, Some(false));
    assert_eq!(sent_patch.delete_branch_on_merge, Some(true));
}

#[tokio::test]
async fn repo_settings_apply_dry_run_no_api() {
    let repo = make_repo("org/alpha");
    let provider =
        MockProvider::new(vec![repo]).with_repo_settings("org/alpha", settings(true, true, false));

    let report = run_check(vec![standard_policy()], &provider).await;
    let apply_report = run_apply(&report, &provider, true).await;

    assert_eq!(
        provider.update_repo_settings_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(apply_report.settings_applied.len(), 1);
    assert_eq!(
        apply_report.settings_applied[0].action,
        SettingsActionKind::DryRun
    );
    assert!(apply_report.settings_errored.is_empty());
}

#[tokio::test]
async fn repo_settings_merged_from_multiple_rules() {
    let repo = make_repo("org/alpha");
    let provider =
        MockProvider::new(vec![repo]).with_repo_settings("org/alpha", settings(true, true, false));

    let policy_a = PolicyConfig {
        name: "policy-a".to_string(),
        description: None,
        severity: Severity::Warning,
        targets: TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![],
        },
        rules: vec![RuleDefinition::RepoSettings(RepoSettingsConfig {
            has_wiki: Some(false),
            ..Default::default()
        })],
    };

    let policy_b = PolicyConfig {
        name: "policy-b".to_string(),
        description: None,
        severity: Severity::Warning,
        targets: TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![],
        },
        rules: vec![RuleDefinition::RepoSettings(RepoSettingsConfig {
            has_projects: Some(false),
            delete_branch_on_merge: Some(true),
            ..Default::default()
        })],
    };

    let report = run_check(vec![policy_a, policy_b], &provider).await;
    let apply_report = run_apply(&report, &provider, false).await;

    // Both policies contribute, but there should be exactly one API call per
    // repo with the merged patch.
    assert_eq!(
        provider.update_repo_settings_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(apply_report.settings_applied.len(), 1);

    let history = provider.update_repo_settings_history.lock().unwrap();
    let (_, patch) = &history[0];
    assert_eq!(patch.has_wiki, Some(false));
    assert_eq!(patch.has_projects, Some(false));
    assert_eq!(patch.delete_branch_on_merge, Some(true));

    let action = &apply_report.settings_applied[0];
    assert!(action.policy_names.contains(&"policy-a".to_string()));
    assert!(action.policy_names.contains(&"policy-b".to_string()));
}
