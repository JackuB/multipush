use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::rules::RuleDefinition;
use crate::model::{Severity, Visibility};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub severity: Severity,
    pub targets: TargetConfig,
    pub rules: Vec<RuleDefinition>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetConfig {
    /// One glob pattern, or a list of glob patterns. A repo matches if **any**
    /// pattern matches. Brace expansion works inside a single pattern, e.g.
    /// `org/{api,web}`.
    #[serde(deserialize_with = "deserialize_repos")]
    #[schemars(with = "ReposSchema")]
    pub repos: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default = "default_true")]
    pub exclude_archived: bool,
    #[serde(default)]
    pub filters: Vec<FilterConfig>,
}

fn default_true() -> bool {
    true
}

fn deserialize_repos<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    let v = match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    };
    if v.is_empty() {
        return Err(D::Error::custom(
            "`repos` must contain at least one glob pattern",
        ));
    }
    Ok(v)
}

/// Schema marker: `repos` accepts a single string or a list of strings.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum ReposSchema {
    Single(String),
    Multiple(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repos_accepts_single_string() {
        let t: TargetConfig = serde_yaml_ng::from_str("repos: \"org/*\"\n").unwrap();
        assert_eq!(t.repos, vec!["org/*".to_string()]);
    }

    #[test]
    fn repos_accepts_list() {
        let yaml = "repos:\n  - acme/api\n  - acme/web*\n";
        let t: TargetConfig = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(
            t.repos,
            vec!["acme/api".to_string(), "acme/web*".to_string()]
        );
    }

    #[test]
    fn repos_rejects_empty_list() {
        let err = serde_yaml_ng::from_str::<TargetConfig>("repos: []\n").unwrap_err();
        assert!(err.to_string().contains("at least one glob pattern"));
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterConfig {
    HasFile(String),
    Topic(String),
    Visibility(Visibility),
    CustomProperty(CustomPropertyFilter),
}

/// Matches (or, with `negate`, excludes) repos where the custom property
/// `key` is set to `value`.
///
/// Custom properties are organization-defined repo metadata; see
/// <https://docs.github.com/en/organizations/managing-organization-settings/managing-custom-properties-for-repositories-in-your-organization>.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomPropertyFilter {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub negate: bool,
}
