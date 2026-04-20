use std::path::PathBuf;

use multipush_core::config::{load_config, ConfigSource, RuleDefinition};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

#[test]
fn config_single_file() {
    let path = fixtures_dir().join("valid_basic.yml");
    let config = load_config(&[ConfigSource::FilePath(path)]).unwrap();

    assert_eq!(config.provider.org, "test-org");
    assert_eq!(config.policies.len(), 1);
    assert_eq!(config.policies[0].name, "require-readme");
}

#[test]
fn config_multi_file() {
    let dir = fixtures_dir().join("multi_file");
    let config = load_config(&[ConfigSource::Directory(dir)]).unwrap();

    assert_eq!(config.provider.org, "test-org");
    assert_eq!(config.policies.len(), 1);
    assert_eq!(config.policies[0].name, "require-license");
}

#[test]
fn config_validation_unknown_rule_type() {
    let path = fixtures_dir().join("invalid_unknown_field.yml");
    let err = load_config(&[ConfigSource::FilePath(path)]).unwrap_err();
    let msg = err.to_string();
    // Should suggest the correct rule name
    assert!(
        msg.contains("ensure_file") || msg.contains("unknown variant"),
        "error should mention the typo: {msg}"
    );
}

#[test]
fn config_validation_empty_policies() {
    let path = fixtures_dir().join("invalid_empty_policies.yml");
    let err = load_config(&[ConfigSource::FilePath(path)]).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("at least one policy"),
        "error should mention empty policies: {msg}"
    );
}

#[test]
fn recipe_expansion() {
    let path = fixtures_dir().join("recipe_codeowners.yml");
    let config = load_config(&[ConfigSource::FilePath(path)]).unwrap();

    assert_eq!(config.policies.len(), 1);
    assert_eq!(config.policies[0].name, "codeowners");

    match &config.policies[0].rules[0] {
        RuleDefinition::EnsureFile(cfg) => {
            assert_eq!(cfg.path, "CODEOWNERS");
            assert!(cfg.content.as_ref().unwrap().contains("@platform-team"));
        }
        other => panic!("expected EnsureFile, got {other:?}"),
    }
}
