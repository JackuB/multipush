use multipush_core::config::{
    EnsureFileConfig, EnsureFileMode, PolicyConfig, RuleDefinition, TargetConfig,
};
use multipush_core::engine::evaluate;
use multipush_core::model::Severity;
use multipush_core::rule::Rule;
use multipush_core::testing::{make_repo, make_repo_archived, test_config, MockProvider};
use multipush_core::Result;

fn rules_factory(policy: &PolicyConfig) -> Result<Vec<Box<dyn Rule>>> {
    policy
        .rules
        .iter()
        .map(multipush_rules::create_rule)
        .collect()
}

#[tokio::test]
async fn check_mode_basic() {
    let repos = vec![make_repo("org/has-readme"), make_repo("org/no-readme")];
    let provider = MockProvider::new(repos).with_file("org/has-readme:README.md", "# Has Readme");

    let config = test_config(vec![PolicyConfig {
        name: "require-readme".to_string(),
        description: None,
        severity: Severity::Error,
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

    let report = evaluate(&config, &provider, rules_factory, 10)
        .await
        .unwrap();

    assert_eq!(report.summary.total_repos, 2);
    assert_eq!(report.summary.passing, 1);
    assert_eq!(report.summary.failing, 1);
}

#[tokio::test]
async fn check_mode_targeting() {
    let repos = vec![
        make_repo("org/active"),
        make_repo_archived("org/archived"),
        make_repo("org/excluded-repo"),
        make_repo("other/outside"),
    ];
    let provider = MockProvider::new(repos).with_file("org/active:README.md", "# Active");

    let config = test_config(vec![PolicyConfig {
        name: "require-readme".to_string(),
        description: None,
        severity: Severity::Error,
        targets: TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec!["org/excluded-*".to_string()],
            exclude_archived: true,
            filters: vec![],
        },
        rules: vec![RuleDefinition::EnsureFile(EnsureFileConfig {
            path: "README.md".to_string(),
            content: None,
            mode: EnsureFileMode::CreateIfMissing,
        })],
    }]);

    let report = evaluate(&config, &provider, rules_factory, 10)
        .await
        .unwrap();

    // Only org/active should be evaluated (archived excluded, excluded-repo excluded, other/outside not matched)
    assert_eq!(report.summary.total_repos, 1);
    assert_eq!(report.summary.passing, 1);
}

#[tokio::test]
async fn check_mode_multiple_rules() {
    let repos = vec![make_repo("org/alpha")];
    let provider = MockProvider::new(repos).with_file("org/alpha:README.md", "# Alpha");

    let config = test_config(vec![PolicyConfig {
        name: "require-files".to_string(),
        description: None,
        severity: Severity::Error,
        targets: TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![],
        },
        rules: vec![
            RuleDefinition::EnsureFile(EnsureFileConfig {
                path: "README.md".to_string(),
                content: None,
                mode: EnsureFileMode::CreateIfMissing,
            }),
            RuleDefinition::EnsureFile(EnsureFileConfig {
                path: "LICENSE".to_string(),
                content: None,
                mode: EnsureFileMode::CreateIfMissing,
            }),
        ],
    }]);

    let report = evaluate(&config, &provider, rules_factory, 10)
        .await
        .unwrap();

    // README passes but LICENSE fails -> overall fail
    assert_eq!(report.summary.total_repos, 1);
    assert_eq!(report.summary.failing, 1);

    let detail = match &report.results[0].repo_results[0].outcome {
        multipush_core::formatter::RepoOutcome::Fail { detail, .. } => detail.clone(),
        other => panic!("expected Fail, got {other:?}"),
    };
    assert!(
        detail.contains("LICENSE"),
        "detail should mention LICENSE: {detail}"
    );
}
