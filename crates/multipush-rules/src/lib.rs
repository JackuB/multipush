//! Built-in rule implementations for multipush.
//!
//! Provides `EnsureFile`, `EnsureJsonKey`, `EnsureYamlKey`, and `FileMatches`
//! rules that implement the `multipush_core::rule::Rule` trait.

mod ensure_file;
mod ensure_json_key;
mod ensure_yaml_key;
mod file_matches;
mod key_path;

pub use ensure_file::EnsureFileRule;
pub use ensure_json_key::EnsureJsonKeyRule;
pub use ensure_yaml_key::EnsureYamlKeyRule;
pub use file_matches::FileMatchesRule;

use multipush_core::config::RuleDefinition;
use multipush_core::rule::Rule;

pub fn create_rule(def: &RuleDefinition) -> multipush_core::Result<Box<dyn Rule>> {
    match def {
        RuleDefinition::EnsureFile(config) => Ok(Box::new(EnsureFileRule::new(config.clone()))),
        RuleDefinition::EnsureJsonKey(config) => {
            Ok(Box::new(EnsureJsonKeyRule::new(config.clone())))
        }
        RuleDefinition::EnsureYamlKey(config) => {
            Ok(Box::new(EnsureYamlKeyRule::new(config.clone())))
        }
        RuleDefinition::FileMatches(config) => Ok(Box::new(FileMatchesRule::new(config.clone())?)),
    }
}
