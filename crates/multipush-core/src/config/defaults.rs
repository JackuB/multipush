use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::policy::TargetConfig;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DefaultsConfig {
    pub targets: Option<TargetConfig>,
    pub apply: Option<ApplyConfig>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyConfig {
    #[serde(default = "default_pr_prefix")]
    pub pr_prefix: String,
    pub commit_author: Option<String>,
    #[serde(default)]
    pub pr_labels: Vec<String>,
    #[serde(default)]
    pub pr_draft: bool,
    #[serde(default)]
    pub existing_pr: ExistingPrStrategy,
    #[serde(default)]
    pub auto_merge: bool,
}

fn default_pr_prefix() -> String {
    "multipush".to_string()
}

#[derive(Debug, Default, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExistingPrStrategy {
    Skip,
    #[default]
    Update,
    Recreate,
}
