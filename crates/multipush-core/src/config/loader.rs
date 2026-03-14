use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_yaml_ng::{Mapping, Value};

use crate::config::RootConfig;
use crate::error::CoreError;
use crate::recipe::builtin::builtin_recipes;
use crate::recipe::Recipe;
use crate::Result;

/// Where a config fragment comes from.
#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// A single YAML file.
    FilePath(PathBuf),
    /// A directory — load all *.yml / *.yaml files alphabetically.
    Directory(PathBuf),
}

/// A loaded but unmerged YAML fragment with provenance info.
struct ConfigLayer {
    #[allow(dead_code)]
    source: String,
    value: Value,
}

/// Load configuration from multiple sources with standard layering.
///
/// Loading order:
/// 1. `~/.config/multipush/config.yml` (optional)
/// 2. `.multipush/multipush.yml` in CWD (optional)
/// 3. `.multipush/policies/` directory (optional)
/// 4. Explicit sources from CLI args
pub fn load_config(sources: &[ConfigSource]) -> Result<RootConfig> {
    let layers = collect_layers(sources)?;
    if layers.is_empty() {
        return Err(CoreError::Config("no configuration found".into()));
    }
    let mut merged = merge_layers(layers);
    expand_recipes(&mut merged)?;
    let config: RootConfig = serde_yaml_ng::from_value(merged).map_err(|e| {
        let msg = e.to_string();
        let enhanced = enhance_deser_error(&msg);
        CoreError::Config(enhanced)
    })?;
    validate(&config)?;
    Ok(config)
}

// ---------------------------------------------------------------------------
// Layer collection
// ---------------------------------------------------------------------------

fn collect_layers(sources: &[ConfigSource]) -> Result<Vec<ConfigLayer>> {
    let mut layers = Vec::new();

    // 1. Global config
    if let Some(home) = home::home_dir() {
        let global = home.join(".config/multipush/config.yml");
        if global.is_file() {
            layers.push(load_layer(&global)?);
        }
    }

    // 2. Project-level config
    let project = PathBuf::from(".multipush/multipush.yml");
    if project.is_file() {
        layers.push(load_layer(&project)?);
    }

    // 3. Project-level policies directory
    let policies_dir = PathBuf::from(".multipush/policies");
    if policies_dir.is_dir() {
        layers.extend(load_directory(&policies_dir)?);
    }

    // 4. Explicit CLI sources
    for source in sources {
        match source {
            ConfigSource::FilePath(path) => {
                layers.push(load_layer(path)?);
            }
            ConfigSource::Directory(dir) => {
                layers.extend(load_directory(dir)?);
            }
        }
    }

    Ok(layers)
}

fn load_layer(path: &Path) -> Result<ConfigLayer> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| CoreError::Config(format!("cannot read {}: {e}", path.display())))?;
    let resolved = resolve_env_vars(&raw)?;
    let value: Value = serde_yaml_ng::from_str(&resolved)
        .map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))?;
    Ok(ConfigLayer {
        source: path.display().to_string(),
        value,
    })
}

fn load_directory(dir: &Path) -> Result<Vec<ConfigLayer>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| CoreError::Config(format!("cannot read directory {}: {e}", dir.display())))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            match path.extension().and_then(|e| e.to_str()) {
                Some("yml" | "yaml") => Some(path),
                _ => None,
            }
        })
        .collect();
    entries.sort();
    entries.iter().map(|p| load_layer(p)).collect()
}

// ---------------------------------------------------------------------------
// Environment variable interpolation
// ---------------------------------------------------------------------------

fn resolve_env_vars(input: &str) -> Result<String> {
    let re = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-(.*?))?\}").unwrap();
    let mut missing = Vec::new();

    let result = re.replace_all(input, |caps: &regex::Captures| {
        let var_name = &caps[1];
        match std::env::var(var_name) {
            Ok(val) => val,
            Err(_) => match caps.get(2) {
                Some(default_match) => default_match.as_str().to_string(),
                None => {
                    missing.push(var_name.to_string());
                    String::new()
                }
            },
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

// ---------------------------------------------------------------------------
// Deep merge
// ---------------------------------------------------------------------------

fn merge_layers(layers: Vec<ConfigLayer>) -> Value {
    let mut result = Value::Mapping(Mapping::new());
    for layer in layers {
        result = deep_merge(result, layer.value);
    }
    dedup_policies(&mut result);
    result
}

fn deep_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Mapping(mut base_map), Value::Mapping(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                let is_policies = matches!(&key, Value::String(s) if s == "policies");
                if let Some(base_val) = base_map.remove(&key) {
                    if is_policies {
                        base_map.insert(key, concat_sequences(base_val, overlay_val));
                    } else {
                        base_map.insert(key, deep_merge(base_val, overlay_val));
                    }
                } else {
                    base_map.insert(key, overlay_val);
                }
            }
            Value::Mapping(base_map)
        }
        (_base, overlay) => overlay,
    }
}

fn concat_sequences(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Sequence(mut base_seq), Value::Sequence(overlay_seq)) => {
            base_seq.extend(overlay_seq);
            Value::Sequence(base_seq)
        }
        (_, overlay) => overlay,
    }
}

fn dedup_policies(merged: &mut Value) {
    let mapping = match merged {
        Value::Mapping(m) => m,
        _ => return,
    };

    let policies_key = Value::String("policies".to_string());
    let policies = match mapping.get_mut(&policies_key) {
        Some(Value::Sequence(seq)) => seq,
        _ => return,
    };

    // Track name -> last index, warn on duplicates.
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut has_duplicates = false;
    for (i, policy) in policies.iter().enumerate() {
        if let Value::Mapping(m) = policy {
            if let Some(Value::String(name)) = m.get(Value::String("name".to_string())) {
                if let Some(_prev) = seen.insert(name.clone(), i) {
                    has_duplicates = true;
                    tracing::warn!(
                        policy = %name,
                        "duplicate policy name, later definition wins"
                    );
                }
            }
        }
    }

    if has_duplicates {
        let keep_named: HashSet<usize> = seen.values().copied().collect();
        let name_key = Value::String("name".to_string());
        let mut i = 0;
        policies.retain(|policy| {
            let idx = i;
            i += 1;
            let is_named =
                matches!(policy, Value::Mapping(m) if m.contains_key(&name_key));
            if is_named {
                keep_named.contains(&idx)
            } else {
                true
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Recipe expansion
// ---------------------------------------------------------------------------

fn expand_recipes(merged: &mut Value) -> Result<()> {
    let recipes = builtin_recipes()?;
    let recipe_map: HashMap<String, Recipe> = recipes
        .into_iter()
        .map(|r| (r.name.clone(), r))
        .collect();

    let policies = match merged {
        Value::Mapping(m) => match m.get_mut(Value::String("policies".into())) {
            Some(Value::Sequence(seq)) => seq,
            _ => return Ok(()),
        },
        _ => return Ok(()),
    };

    let mut expanded = Vec::new();
    for policy_val in policies.iter() {
        let policy_map = match policy_val.as_mapping() {
            Some(m) => m,
            None => {
                expanded.push(policy_val.clone());
                continue;
            }
        };

        // Check if this policy uses a recipe
        let recipe_name = match policy_map.get(Value::String("recipe".into())) {
            Some(Value::String(name)) => name.clone(),
            Some(_) => {
                return Err(CoreError::Recipe(
                    "'recipe' field must be a string".into(),
                ))
            }
            None => {
                expanded.push(policy_val.clone());
                continue;
            }
        };

        let recipe = recipe_map.get(&recipe_name).ok_or_else(|| {
            let mut available: Vec<&str> = recipe_map.keys().map(|s| s.as_str()).collect();
            available.sort();
            CoreError::Recipe(format!(
                "unknown recipe '{recipe_name}'. Available recipes: {}",
                available.join(", ")
            ))
        })?;

        // Validate targets is provided
        if !policy_map.contains_key(Value::String("targets".into())) {
            return Err(CoreError::Recipe(format!(
                "recipe '{recipe_name}' policy must include 'targets'"
            )));
        }

        // Extract params
        let params = extract_recipe_params(policy_map)?;

        // Expand recipe
        let mut expanded_val = recipe.expand(&params)?;

        // Merge user overrides (targets, severity, name, description)
        if let Value::Mapping(ref mut exp_map) = expanded_val {
            for key in &["targets", "severity", "name", "description"] {
                let k = Value::String((*key).to_string());
                if let Some(user_val) = policy_map.get(&k) {
                    exp_map.insert(k, user_val.clone());
                }
            }
        }

        expanded.push(expanded_val);
    }

    // Replace policies with expanded versions
    if let Value::Mapping(m) = merged {
        m.insert(
            Value::String("policies".into()),
            Value::Sequence(expanded),
        );
    }

    Ok(())
}

fn extract_recipe_params(policy_map: &Mapping) -> Result<HashMap<String, String>> {
    let mut params = HashMap::new();

    let params_val = match policy_map.get(Value::String("params".into())) {
        Some(v) => v,
        None => return Ok(params),
    };

    let params_map = params_val
        .as_mapping()
        .ok_or_else(|| CoreError::Recipe("'params' must be a mapping".into()))?;

    for (key, val) in params_map {
        let k = key
            .as_str()
            .ok_or_else(|| CoreError::Recipe("param key must be a string".into()))?;
        let v = match val {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => {
                return Err(CoreError::Recipe(format!(
                    "param '{k}' value must be a scalar"
                )))
            }
        };
        params.insert(k.to_string(), v);
    }

    Ok(params)
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate(config: &RootConfig) -> Result<()> {
    let mut errors = Vec::new();

    // Provider
    if config.provider.org.is_empty() {
        errors.push("provider.org must not be empty".to_string());
    }
    if config.provider.token.is_empty() {
        errors.push("provider.token must not be empty".to_string());
    }

    // Policies
    if config.policies.is_empty() {
        errors.push("at least one policy is required".to_string());
    }

    let mut policy_names = HashSet::new();
    for policy in &config.policies {
        if !policy_names.insert(&policy.name) {
            errors.push(format!("duplicate policy name: '{}'", policy.name));
        }
        validate_policy(policy, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(CoreError::ConfigValidation(errors))
    }
}

fn validate_policy(policy: &crate::config::PolicyConfig, errors: &mut Vec<String>) {
    if policy.rules.is_empty() {
        errors.push(format!(
            "policy '{}' must have at least one rule",
            policy.name
        ));
    }

    for (i, rule) in policy.rules.iter().enumerate() {
        validate_rule(rule, &policy.name, i, errors);
    }
}

fn validate_rule(
    rule: &crate::config::RuleDefinition,
    policy_name: &str,
    index: usize,
    errors: &mut Vec<String>,
) {
    use crate::config::RuleDefinition;
    let ctx = format!("policy '{}', rule {}", policy_name, index + 1);
    match rule {
        RuleDefinition::EnsureFile(cfg) => {
            if cfg.path.is_empty() {
                errors.push(format!("{ctx}: ensure_file.path must not be empty"));
            }
        }
        RuleDefinition::EnsureJsonKey(cfg) => {
            if cfg.path.is_empty() {
                errors.push(format!("{ctx}: ensure_json_key.path must not be empty"));
            }
            if cfg.key.is_empty() {
                errors.push(format!("{ctx}: ensure_json_key.key must not be empty"));
            }
        }
        RuleDefinition::EnsureYamlKey(cfg) => {
            if cfg.path.is_empty() {
                errors.push(format!("{ctx}: ensure_yaml_key.path must not be empty"));
            }
            if cfg.key.is_empty() {
                errors.push(format!("{ctx}: ensure_yaml_key.key must not be empty"));
            }
        }
        RuleDefinition::FileMatches(cfg) => {
            if cfg.path.is_empty() {
                errors.push(format!("{ctx}: file_matches.path must not be empty"));
            }
            if cfg.pattern.is_empty() {
                errors.push(format!("{ctx}: file_matches.pattern must not be empty"));
            } else if let Err(e) = regex::Regex::new(&cfg.pattern) {
                errors.push(format!("{ctx}: file_matches.pattern is invalid regex: {e}"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Deserialization error enhancement (Levenshtein suggestions)
// ---------------------------------------------------------------------------

fn enhance_deser_error(msg: &str) -> String {
    // serde_yaml_ng errors for unknown tagged variants look like:
    // "unknown variant `ensure_flie`, expected one of ..."
    let re = Regex::new(r"unknown variant `([^`]+)`").unwrap();
    if let Some(caps) = re.captures(msg) {
        let unknown = &caps[1];
        if let Some(suggestion) = suggest_rule_type(unknown) {
            return format!("{msg}. {suggestion}");
        }
    }
    msg.to_string()
}

fn suggest_rule_type(unknown: &str) -> Option<String> {
    const KNOWN: &[&str] = &[
        "ensure_file",
        "ensure_json_key",
        "ensure_yaml_key",
        "file_matches",
    ];
    let mut best: Option<(&str, usize)> = None;
    for &k in KNOWN {
        let dist = levenshtein(unknown, k);
        match &best {
            Some((_, d)) if dist < *d => best = Some((k, dist)),
            None if dist <= 3 => best = Some((k, dist)),
            _ => {}
        }
    }
    best.map(|(name, _)| format!("Did you mean '!{name}'?"))
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b_len = b.len();
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_len]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("multipush-tests-{n}"));
        // Remove any stale files from previous test runs to avoid
        // directory-based tests picking up leftover YAML files.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tmp_yaml(content: &str) -> PathBuf {
        let dir = tmp_dir();
        let path = dir.join("config.yml");
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

    fn load_single(content: &str) -> Result<RootConfig> {
        let path = tmp_yaml(content);
        load_config(&[ConfigSource::FilePath(path)])
    }

    #[test]
    fn load_valid_config() {
        let config = load_single(valid_yaml()).unwrap();
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

        let config = load_single(yaml).unwrap();
        assert_eq!(config.provider.org, "my-org");
        assert_eq!(config.provider.token, "ghp_abc");

        std::env::remove_var("TEST_MP_ORG");
        std::env::remove_var("TEST_MP_TOKEN");
    }

    #[test]
    fn env_var_with_default() {
        std::env::remove_var("UNSET_VAR_DEFAULT_TEST");
        let input = "value: ${UNSET_VAR_DEFAULT_TEST:-fallback_value}";
        let resolved = resolve_env_vars(input).unwrap();
        assert_eq!(resolved, "value: fallback_value");
    }

    #[test]
    fn env_var_with_default_when_set() {
        std::env::set_var("SET_VAR_DEFAULT_TEST", "real_value");
        let input = "value: ${SET_VAR_DEFAULT_TEST:-fallback}";
        let resolved = resolve_env_vars(input).unwrap();
        assert_eq!(resolved, "value: real_value");
        std::env::remove_var("SET_VAR_DEFAULT_TEST");
    }

    #[test]
    fn env_var_with_empty_default() {
        std::env::remove_var("UNSET_VAR_EMPTY_DEFAULT");
        let input = "value: ${UNSET_VAR_EMPTY_DEFAULT:-}";
        let resolved = resolve_env_vars(input).unwrap();
        assert_eq!(resolved, "value: ");
    }

    #[test]
    fn missing_env_var_error() {
        std::env::remove_var("MISSING_VAR_A");
        std::env::remove_var("MISSING_VAR_B");

        let input = "a: ${MISSING_VAR_A}\nb: ${MISSING_VAR_B}";
        let err = resolve_env_vars(input).unwrap_err();
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
        let err = load_single(yaml).unwrap_err();
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
        let err = load_single(yaml).unwrap_err();
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
        let err = load_single(yaml).unwrap_err();
        assert!(err.to_string().contains("empty-policy"));
        assert!(err.to_string().contains("at least one rule"));
    }

    #[test]
    fn collects_all_validation_errors() {
        let yaml = r#"
provider:
  type: github
  org: ""
  token: ""

policies:
  - name: bad-policy
    targets:
      repos: "*"
    rules: []
"#;
        let err = load_single(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("org must not be empty"), "got: {msg}");
        assert!(msg.contains("token must not be empty"), "got: {msg}");
        assert!(msg.contains("at least one rule"), "got: {msg}");
    }

    #[test]
    fn validates_rule_fields() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - name: bad-rules
    targets:
      repos: "*"
    rules:
      - !ensure_file
        path: ""
      - !file_matches
        path: test.txt
        pattern: "[invalid"
"#;
        let err = load_single(yaml).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ensure_file.path must not be empty"), "got: {msg}");
        assert!(msg.contains("file_matches.pattern is invalid regex"), "got: {msg}");
    }

    #[test]
    fn multi_file_merge() {
        let dir = tmp_dir();

        let provider_file = dir.join("01-provider.yml");
        std::fs::write(
            &provider_file,
            r#"
provider:
  type: github
  org: test-org
  token: ghp_test
"#,
        )
        .unwrap();

        let policies_file = dir.join("02-policies.yml");
        std::fs::write(
            &policies_file,
            r#"
policies:
  - name: require-readme
    targets:
      repos: "test-org/*"
    rules:
      - !ensure_file
        path: README.md
"#,
        )
        .unwrap();

        let config = load_config(&[ConfigSource::Directory(dir)]).unwrap();
        assert_eq!(config.provider.org, "test-org");
        assert_eq!(config.policies.len(), 1);
        assert_eq!(config.policies[0].name, "require-readme");
    }

    #[test]
    fn multi_file_policy_concatenation() {
        let dir = tmp_dir();

        let base = dir.join("01-base.yml");
        std::fs::write(
            &base,
            r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - name: policy-a
    targets:
      repos: "test-org/*"
    rules:
      - !ensure_file
        path: README.md
"#,
        )
        .unwrap();

        let extra = dir.join("02-extra.yml");
        std::fs::write(
            &extra,
            r#"
policies:
  - name: policy-b
    targets:
      repos: "test-org/*"
    rules:
      - !ensure_file
        path: LICENSE
"#,
        )
        .unwrap();

        let config = load_config(&[ConfigSource::Directory(dir)]).unwrap();
        assert_eq!(config.policies.len(), 2);
        assert_eq!(config.policies[0].name, "policy-a");
        assert_eq!(config.policies[1].name, "policy-b");
    }

    #[test]
    fn duplicate_policy_last_wins() {
        let dir = tmp_dir();

        let first = dir.join("01-first.yml");
        std::fs::write(
            &first,
            r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - name: my-policy
    targets:
      repos: "test-org/*"
    rules:
      - !ensure_file
        path: README.md
"#,
        )
        .unwrap();

        let second = dir.join("02-second.yml");
        std::fs::write(
            &second,
            r#"
policies:
  - name: my-policy
    targets:
      repos: "test-org/*"
    rules:
      - !ensure_file
        path: LICENSE
"#,
        )
        .unwrap();

        let config = load_config(&[ConfigSource::Directory(dir)]).unwrap();
        assert_eq!(config.policies.len(), 1);
        assert_eq!(config.policies[0].name, "my-policy");
        // The second file's rule should win
        match &config.policies[0].rules[0] {
            crate::config::RuleDefinition::EnsureFile(cfg) => {
                assert_eq!(cfg.path, "LICENSE");
            }
            _ => panic!("expected EnsureFile"),
        }
    }

    #[test]
    fn provider_deep_merge() {
        let dir = tmp_dir();

        let first = dir.join("01-partial.yml");
        std::fs::write(
            &first,
            r#"
provider:
  type: github
  org: test-org
  token: ghp_first

policies:
  - name: p1
    targets:
      repos: "*"
    rules:
      - !ensure_file
        path: README.md
"#,
        )
        .unwrap();

        let second = dir.join("02-override.yml");
        std::fs::write(
            &second,
            r#"
provider:
  token: ghp_override
"#,
        )
        .unwrap();

        let config = load_config(&[ConfigSource::Directory(dir)]).unwrap();
        assert_eq!(config.provider.org, "test-org");
        assert_eq!(config.provider.token, "ghp_override");
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", "abc"), 0);
    }

    #[test]
    fn suggest_rule_type_typo() {
        let suggestion = suggest_rule_type("ensure_flie");
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("ensure_file"));
    }

    #[test]
    fn suggest_rule_type_no_match() {
        let suggestion = suggest_rule_type("completely_wrong_name");
        assert!(suggestion.is_none());
    }

    #[test]
    fn no_sources_error() {
        // Create a tmp dir to avoid picking up any .multipush/ in CWD
        let dir = tmp_dir();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let result = load_config(&[]);
        std::env::set_current_dir(original).unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no configuration found"));
    }

    #[test]
    fn deep_merge_scalar_override() {
        let base = serde_yaml_ng::from_str::<Value>("a: 1\nb: 2").unwrap();
        let overlay = serde_yaml_ng::from_str::<Value>("b: 3\nc: 4").unwrap();
        let merged = deep_merge(base, overlay);
        let m = merged.as_mapping().unwrap();
        assert_eq!(m.get(&Value::String("a".into())), Some(&Value::Number(1.into())));
        assert_eq!(m.get(&Value::String("b".into())), Some(&Value::Number(3.into())));
        assert_eq!(m.get(&Value::String("c".into())), Some(&Value::Number(4.into())));
    }

    #[test]
    fn recipe_expansion_in_config() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - recipe: codeowners
    params:
      default_owner: "@platform-team"
    targets:
      repos: "test-org/*"
"#;
        let config = load_single(yaml).unwrap();
        assert_eq!(config.policies.len(), 1);
        assert_eq!(config.policies[0].name, "codeowners");
        assert_eq!(config.policies[0].rules.len(), 1);

        match &config.policies[0].rules[0] {
            crate::config::RuleDefinition::EnsureFile(cfg) => {
                assert_eq!(cfg.path, "CODEOWNERS");
                assert!(cfg.content.as_ref().unwrap().contains("@platform-team"));
            }
            _ => panic!("expected EnsureFile rule"),
        }
    }

    #[test]
    fn recipe_with_name_override() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - recipe: editorconfig
    name: custom-editorconfig
    severity: warning
    targets:
      repos: "test-org/*"
"#;
        let config = load_single(yaml).unwrap();
        assert_eq!(config.policies[0].name, "custom-editorconfig");
    }

    #[test]
    fn recipe_mixed_with_regular_policies() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - name: require-readme
    targets:
      repos: "test-org/*"
    rules:
      - !ensure_file
        path: README.md
  - recipe: dependabot
    params:
      ecosystem: cargo
    targets:
      repos: "test-org/*"
"#;
        let config = load_single(yaml).unwrap();
        assert_eq!(config.policies.len(), 2);
        assert_eq!(config.policies[0].name, "require-readme");
        assert_eq!(config.policies[1].name, "dependabot");
    }

    #[test]
    fn recipe_unknown_name_error() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - recipe: nonexistent
    targets:
      repos: "test-org/*"
"#;
        let err = load_single(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown recipe 'nonexistent'"));
    }

    #[test]
    fn recipe_missing_targets_error() {
        let yaml = r#"
provider:
  type: github
  org: test-org
  token: ghp_test

policies:
  - recipe: editorconfig
"#;
        let err = load_single(yaml).unwrap_err();
        assert!(err.to_string().contains("must include 'targets'"));
    }
}
