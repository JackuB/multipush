use std::path::Path;

use regex::Regex;

use crate::config::RootConfig;
use crate::error::CoreError;
use crate::Result;

pub fn load_config(path: &Path) -> Result<RootConfig> {
    let raw = std::fs::read_to_string(path)?;
    let resolved = resolve_env_vars(&raw)?;
    let config: RootConfig = serde_yaml_ng::from_str(&resolved)?;
    validate(&config)?;
    Ok(config)
}

fn resolve_env_vars(input: &str) -> Result<String> {
    let re = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
    let mut missing = Vec::new();

    let result = re.replace_all(input, |caps: &regex::Captures| {
        let var_name = &caps[1];
        match std::env::var(var_name) {
            Ok(val) => val,
            Err(_) => {
                missing.push(var_name.to_string());
                String::new()
            }
        }
    });

    if !missing.is_empty() {
        missing.sort();
        missing.dedup();
        return Err(CoreError::Config(format!(
            "missing environment variables: {}",
            missing.join(", ")
        )));
    }

    Ok(result.into_owned())
}

fn validate(config: &RootConfig) -> Result<()> {
    if config.provider.org.is_empty() {
        return Err(CoreError::Config("provider.org must not be empty".into()));
    }

    if config.policies.is_empty() {
        return Err(CoreError::Config("at least one policy is required".into()));
    }

    for policy in &config.policies {
        if policy.rules.is_empty() {
            return Err(CoreError::Config(format!(
                "policy '{}' must have at least one rule",
                policy.name
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_yaml(content: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join("multipush-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("test-config-{n}.yml"));
        std::fs::write(&path, content).unwrap();
        path
    }

    fn valid_yaml() -> &'static str {
        r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - name: require-readme
    severity: error
    targets:
      repos: "test-org/*"
    rules:
      - !ensure_file
        path: README.md
"#
    }

    #[test]
    fn load_valid_config() {
        let path = tmp_yaml(valid_yaml());
        let config = load_config(&path).unwrap();
        assert_eq!(config.provider.org, "test-org");
        assert_eq!(config.policies.len(), 1);
        assert_eq!(config.policies[0].name, "require-readme");
    }

    #[test]
    fn env_var_resolution() {
        let yaml = r#"
provider:
  type: github
  org: ${TEST_MP_ORG}
  token: ${TEST_MP_TOKEN}

policies:
  - name: p1
    targets:
      repos: "${TEST_MP_ORG}/*"
    rules:
      - !ensure_file
        path: README.md
"#;
        std::env::set_var("TEST_MP_ORG", "my-org");
        std::env::set_var("TEST_MP_TOKEN", "ghp_abc");

        let path = tmp_yaml(yaml);
        let config = load_config(&path).unwrap();
        assert_eq!(config.provider.org, "my-org");
        assert_eq!(config.provider.token, "ghp_abc");

        std::env::remove_var("TEST_MP_ORG");
        std::env::remove_var("TEST_MP_TOKEN");
    }

    #[test]
    fn missing_env_var_error() {
        let yaml = r#"
provider:
  type: github
  org: ${MISSING_VAR_A}
  token: ${MISSING_VAR_B}

policies:
  - name: p1
    targets:
      repos: "*"
    rules:
      - !ensure_file
        path: README.md
"#;
        std::env::remove_var("MISSING_VAR_A");
        std::env::remove_var("MISSING_VAR_B");

        let path = tmp_yaml(yaml);
        let err = load_config(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("MISSING_VAR_A"), "got: {msg}");
        assert!(msg.contains("MISSING_VAR_B"), "got: {msg}");
    }

    #[test]
    fn empty_org_validation() {
        let yaml = r#"
provider:
  type: github
  org: ""
  token: ghp_test

policies:
  - name: p1
    targets:
      repos: "*"
    rules:
      - !ensure_file
        path: README.md
"#;
        let path = tmp_yaml(yaml);
        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("org must not be empty"));
    }

    #[test]
    fn empty_policies_validation() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies: []
"#;
        // Note: serde_yaml_ng parses [] to empty vec, so validation catches it
        let path = tmp_yaml(yaml);
        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("at least one policy"));
    }

    #[test]
    fn policy_with_no_rules_validation() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - name: empty-policy
    targets:
      repos: "*"
    rules: []
"#;
        let path = tmp_yaml(yaml);
        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("empty-policy"));
        assert!(err.to_string().contains("at least one rule"));
    }
}
