use serde::Deserialize;

use crate::config::rules::RuleDefinition;
use crate::model::{Severity, Visibility};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub severity: Severity,
    pub targets: TargetConfig,
    pub rules: Vec<RuleDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetConfig {
    pub repos: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterConfig {
    HasFile(String),
    Topic(String),
    Visibility(Visibility),
}
