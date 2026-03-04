mod defaults;
mod loader;
mod policy;
mod provider;
mod rules;

pub use defaults::{ApplyConfig, DefaultsConfig, ExistingPrStrategy};
pub use loader::{load_config, ConfigSource};
pub use policy::{FilterConfig, PolicyConfig, TargetConfig};
pub use provider::{ProviderConfig, ProviderType};
pub use rules::{
    EnsureFileConfig, EnsureFileMode, EnsureJsonKeyConfig, EnsureYamlKeyConfig, FileMatchesConfig,
    JsonKeyMode, RuleDefinition,
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootConfig {
    pub provider: ProviderConfig,
    pub defaults: Option<DefaultsConfig>,
    pub policies: Vec<PolicyConfig>,
}
