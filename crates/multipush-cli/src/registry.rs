use anyhow::{bail, Result};

use multipush_core::config::{PolicyConfig, ProviderConfig, ProviderType};
use multipush_core::formatter::Formatter;
use multipush_core::provider::Provider;
use multipush_core::rule::Rule;
use multipush_formatters::{MarkdownFormatter, TableFormatter};
use multipush_provider_github::GitHubProvider;

pub fn create_provider(config: &ProviderConfig) -> Result<Box<dyn Provider>> {
    match config.provider_type {
        ProviderType::Github => {
            let provider = GitHubProvider::new(config)?;
            Ok(Box::new(provider))
        }
        ProviderType::Gitea => bail!("gitea provider is not yet implemented"),
    }
}

pub fn create_rules(policy: &PolicyConfig) -> multipush_core::Result<Vec<Box<dyn Rule>>> {
    policy
        .rules
        .iter()
        .map(multipush_rules::create_rule)
        .collect()
}

pub fn create_formatter(format: &str, no_color: bool) -> Result<Box<dyn Formatter>> {
    match format {
        "table" => {
            let color = !no_color;
            Ok(Box::new(TableFormatter::with_color(color)))
        }
        "markdown" => Ok(Box::new(MarkdownFormatter::new())),
        other => bail!("unknown format: {other}"),
    }
}
