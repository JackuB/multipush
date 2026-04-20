use std::sync::atomic::Ordering;

use multipush_core::config::{ApplyConfig, DefaultsConfig, ExistingPrStrategy};
use multipush_core::engine::executor::{execute, PrActionKind};
use multipush_core::model::{PrState, PullRequest};
use multipush_core::testing::{default_config, make_report_with_failures, MockProvider};

#[tokio::test]
async fn apply_mode_dry_run() {
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
}

#[tokio::test]
async fn apply_mode_create_pr() {
    let provider = MockProvider::new(vec![]);
    let report = make_report_with_failures(&["org/alpha"], true);
    let config = default_config();

    let result = execute(&report, &config, &provider, false, 10)
        .await
        .unwrap();

    assert_eq!(result.prs_created.len(), 1);
    assert_eq!(result.prs_created[0].action, PrActionKind::Created);
    assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn apply_mode_existing_pr_skip() {
    let existing_pr = PullRequest {
        number: 42,
        title: "existing".to_string(),
        head_branch: "multipush/require-license".to_string(),
        url: "https://github.com/org/alpha/pull/42".to_string(),
        state: PrState::Open,
    };

    let provider =
        MockProvider::new(vec![]).with_open_pr("org/alpha:multipush/require-license", existing_pr);

    let report = make_report_with_failures(&["org/alpha"], true);

    let mut config = default_config();
    config.defaults = Some(DefaultsConfig {
        targets: None,
        apply: Some(ApplyConfig {
            pr_prefix: "multipush".to_string(),
            commit_author: None,
            pr_labels: vec![],
            pr_draft: false,
            existing_pr: ExistingPrStrategy::Skip,
        }),
    });

    let result = execute(&report, &config, &provider, false, 10)
        .await
        .unwrap();

    assert_eq!(result.prs_skipped.len(), 1);
    assert_eq!(result.prs_created.len(), 0);
    assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn apply_mode_existing_pr_update() {
    let existing_pr = PullRequest {
        number: 42,
        title: "existing".to_string(),
        head_branch: "multipush/require-license".to_string(),
        url: "https://github.com/org/alpha/pull/42".to_string(),
        state: PrState::Open,
    };

    let provider =
        MockProvider::new(vec![]).with_open_pr("org/alpha:multipush/require-license", existing_pr);

    let report = make_report_with_failures(&["org/alpha"], true);
    let config = default_config();

    let result = execute(&report, &config, &provider, false, 10)
        .await
        .unwrap();

    assert_eq!(result.prs_updated.len(), 1);
    assert_eq!(result.prs_created.len(), 0);
    assert_eq!(provider.update_pr_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn apply_mode_max_prs() {
    let provider = MockProvider::new(vec![]);
    let report = make_report_with_failures(&["org/a", "org/b", "org/c", "org/d", "org/e"], true);
    let config = default_config();

    let result = execute(&report, &config, &provider, false, 2)
        .await
        .unwrap();

    assert_eq!(result.prs_created.len(), 2);
    assert_eq!(result.prs_limited, 3);
    assert_eq!(provider.create_pr_calls.load(Ordering::SeqCst), 2);
}
