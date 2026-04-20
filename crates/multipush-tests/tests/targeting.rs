use multipush_core::config::{
    EnsureFileConfig, EnsureFileMode, FilterConfig, PolicyConfig, RuleDefinition, TargetConfig,
};
use multipush_core::engine::evaluate;
use multipush_core::model::{Severity, Visibility};
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

fn dummy_rule() -> RuleDefinition {
    // README always exists in every test repo so we focus the assertions on
    // which repos got evaluated, not whether the rule passed.
    RuleDefinition::EnsureFile(EnsureFileConfig {
        path: "README.md".to_string(),
        content: None,
        mode: EnsureFileMode::CreateIfMissing,
    })
}

fn policy_with_filters(filters: Vec<FilterConfig>) -> PolicyConfig {
    PolicyConfig {
        name: "filtered".to_string(),
        description: None,
        severity: Severity::Error,
        targets: TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters,
        },
        rules: vec![dummy_rule()],
    }
}

fn evaluated_repos(report: &multipush_core::formatter::Report) -> Vec<String> {
    let mut names: Vec<String> = report.results[0]
        .repo_results
        .iter()
        .map(|rr| rr.repo_name.clone())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn targeting_has_file() {
    let repos = vec![make_repo("org/has"), make_repo("org/missing")];
    let provider = MockProvider::new(repos)
        .with_file("org/has:README.md", "# Has")
        .with_file("org/has:Dockerfile", "FROM scratch")
        .with_file("org/missing:README.md", "# Missing");

    let config = test_config(vec![policy_with_filters(vec![FilterConfig::HasFile(
        "Dockerfile".to_string(),
    )])]);

    let report = evaluate(&config, &provider, rules_factory, 4).await.unwrap();
    assert_eq!(evaluated_repos(&report), vec!["org/has"]);
}

#[tokio::test]
async fn targeting_topic() {
    let mut tagged = make_repo("org/tagged");
    tagged.topics = vec!["security".into(), "infra".into()];
    let mut other = make_repo("org/other");
    other.topics = vec!["docs".into()];

    let provider = MockProvider::new(vec![tagged, other])
        .with_file("org/tagged:README.md", "# Tagged")
        .with_file("org/other:README.md", "# Other");

    let config = test_config(vec![policy_with_filters(vec![FilterConfig::Topic(
        "security".to_string(),
    )])]);

    let report = evaluate(&config, &provider, rules_factory, 4).await.unwrap();
    assert_eq!(evaluated_repos(&report), vec!["org/tagged"]);
}

#[tokio::test]
async fn targeting_visibility() {
    let mut public_repo = make_repo("org/public");
    public_repo.visibility = Visibility::Public;
    let private_repo = make_repo("org/private"); // default Private

    let provider = MockProvider::new(vec![public_repo, private_repo])
        .with_file("org/public:README.md", "# Public")
        .with_file("org/private:README.md", "# Private");

    let config = test_config(vec![policy_with_filters(vec![FilterConfig::Visibility(
        Visibility::Public,
    )])]);

    let report = evaluate(&config, &provider, rules_factory, 4).await.unwrap();
    assert_eq!(evaluated_repos(&report), vec!["org/public"]);
}

#[tokio::test]
async fn targeting_combined_and_semantics() {
    // Only `org/match` satisfies all three: public, has Dockerfile, topic=infra.
    let mut match_repo = make_repo("org/match");
    match_repo.visibility = Visibility::Public;
    match_repo.topics = vec!["infra".into()];

    let mut wrong_topic = make_repo("org/wrong-topic");
    wrong_topic.visibility = Visibility::Public;
    wrong_topic.topics = vec!["docs".into()];

    let mut wrong_visibility = make_repo("org/wrong-visibility");
    wrong_visibility.visibility = Visibility::Private;
    wrong_visibility.topics = vec!["infra".into()];

    let mut missing_file = make_repo("org/missing-file");
    missing_file.visibility = Visibility::Public;
    missing_file.topics = vec!["infra".into()];

    let provider = MockProvider::new(vec![
        match_repo,
        wrong_topic,
        wrong_visibility,
        missing_file,
    ])
    .with_file("org/match:README.md", "# Match")
    .with_file("org/match:Dockerfile", "FROM scratch")
    .with_file("org/wrong-topic:README.md", "# WT")
    .with_file("org/wrong-topic:Dockerfile", "FROM scratch")
    .with_file("org/wrong-visibility:README.md", "# WV")
    .with_file("org/wrong-visibility:Dockerfile", "FROM scratch")
    .with_file("org/missing-file:README.md", "# MF");

    let config = test_config(vec![policy_with_filters(vec![
        FilterConfig::HasFile("Dockerfile".to_string()),
        FilterConfig::Topic("infra".to_string()),
        FilterConfig::Visibility(Visibility::Public),
    ])]);

    let report = evaluate(&config, &provider, rules_factory, 4).await.unwrap();
    assert_eq!(evaluated_repos(&report), vec!["org/match"]);
}
