//! Built-in rule implementations for multipush.
//!
//! Provides `EnsureFile`, `EnsureJsonKey`, `EnsureYamlKey`, and `FileMatches`
//! rules that implement the `multipush_core::rule::Rule` trait.

mod ensure_file;

pub use ensure_file::EnsureFileRule;

use multipush_core::config::RuleDefinition;
use multipush_core::rule::Rule;

pub fn create_rule(def: &RuleDefinition) -> multipush_core::Result<Box<dyn Rule>> {
    match def {
        RuleDefinition::EnsureFile(config) => Ok(Box::new(EnsureFileRule::new(config.clone()))),
        RuleDefinition::EnsureJsonKey(_) => Err(multipush_core::CoreError::Config(
            "ensure_json_key rule is not yet implemented".into(),
        )),
        RuleDefinition::EnsureYamlKey(_) => Err(multipush_core::CoreError::Config(
            "ensure_yaml_key rule is not yet implemented".into(),
        )),
        RuleDefinition::FileMatches(_) => Err(multipush_core::CoreError::Config(
            "file_matches rule is not yet implemented".into(),
        )),
    }
}
