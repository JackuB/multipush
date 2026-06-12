use async_trait::async_trait;
use tracing::debug;

use multipush_core::config::EnsureAutolinkConfig;
use multipush_core::rule::{Remediation, Rule, RuleContext, RuleResult};

/// Ensures a repository has an autolink reference for a given key prefix,
/// pointing at the desired URL template. This is how a policy wires GitHub
/// references such as `JIRA-123` to an external tracker (e.g. Jira).
pub struct EnsureAutolinkRule {
    config: EnsureAutolinkConfig,
}

impl EnsureAutolinkRule {
    pub fn new(config: EnsureAutolinkConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Rule for EnsureAutolinkRule {
    fn rule_type(&self) -> &str {
        "ensure_autolink"
    }

    fn description(&self) -> String {
        format!(
            "Ensure autolink '{}' -> '{}'",
            self.config.key_prefix, self.config.url_template
        )
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        let spec = self.config.to_spec();
        debug!(
            repo = %ctx.repo.full_name,
            key_prefix = %spec.key_prefix,
            "evaluating ensure_autolink rule"
        );

        let existing = ctx.provider.list_autolinks(ctx.repo).await?;

        // Exact match on the prefix means the autolink is already in place.
        if existing.iter().any(|a| a.matches(&spec)) {
            return Ok(RuleResult::Pass {
                detail: format!("Autolink '{}' is configured", spec.key_prefix),
            });
        }

        // A link on the same prefix that points somewhere else is drift we can
        // correct; a missing link is a plain create. Both share one remediation.
        let (detail, description) = match existing.iter().find(|a| a.key_prefix == spec.key_prefix)
        {
            Some(current) => (
                format!(
                    "Autolink '{}' targets '{}' (is_alphanumeric: {}), expected '{}' (is_alphanumeric: {})",
                    spec.key_prefix,
                    current.url_template,
                    current.is_alphanumeric,
                    spec.url_template,
                    spec.is_alphanumeric
                ),
                format!(
                    "Update autolink '{}' to target '{}'",
                    spec.key_prefix, spec.url_template
                ),
            ),
            None => (
                format!("Autolink '{}' is not configured", spec.key_prefix),
                format!(
                    "Create autolink '{}' targeting '{}'",
                    spec.key_prefix, spec.url_template
                ),
            ),
        };

        Ok(RuleResult::Fail {
            detail,
            remediation: Some(Remediation::Autolink { description, spec }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::model::Autolink;
    use multipush_core::testing::{make_repo, MockProvider};

    fn config(prefix: &str, url: &str, is_alphanumeric: bool) -> EnsureAutolinkConfig {
        EnsureAutolinkConfig {
            key_prefix: prefix.to_string(),
            url_template: url.to_string(),
            is_alphanumeric,
        }
    }

    fn autolink(id: u64, prefix: &str, url: &str, is_alphanumeric: bool) -> Autolink {
        Autolink {
            id,
            key_prefix: prefix.to_string(),
            url_template: url.to_string(),
            is_alphanumeric,
        }
    }

    #[tokio::test]
    async fn passes_when_autolink_present_and_matching() {
        let repo = make_repo("org/alpha");
        let provider = MockProvider::new(vec![repo.clone()]).with_autolinks(
            "org/alpha",
            vec![autolink(
                1,
                "JIRA-",
                "https://example.atlassian.net/browse/JIRA-<num>",
                false,
            )],
        );

        let rule = EnsureAutolinkRule::new(config(
            "JIRA-",
            "https://example.atlassian.net/browse/JIRA-<num>",
            false,
        ));

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Pass { .. }));
    }

    #[tokio::test]
    async fn fails_when_autolink_missing() {
        let repo = make_repo("org/alpha");
        let provider = MockProvider::new(vec![repo.clone()]);

        let rule = EnsureAutolinkRule::new(config(
            "JIRA-",
            "https://example.atlassian.net/browse/JIRA-<num>",
            false,
        ));

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { remediation, .. } => match remediation.unwrap() {
                Remediation::Autolink { spec, .. } => {
                    assert_eq!(spec.key_prefix, "JIRA-");
                    assert_eq!(
                        spec.url_template,
                        "https://example.atlassian.net/browse/JIRA-<num>"
                    );
                    assert!(!spec.is_alphanumeric);
                }
                other => panic!("expected Autolink remediation, got {other:?}"),
            },
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fails_when_autolink_target_differs() {
        let repo = make_repo("org/alpha");
        let provider = MockProvider::new(vec![repo.clone()]).with_autolinks(
            "org/alpha",
            vec![autolink(
                7,
                "JIRA-",
                "https://old.example.com/browse/JIRA-<num>",
                false,
            )],
        );

        let rule = EnsureAutolinkRule::new(config(
            "JIRA-",
            "https://example.atlassian.net/browse/JIRA-<num>",
            false,
        ));

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                assert!(detail.contains("old.example.com"));
                assert!(matches!(remediation.unwrap(), Remediation::Autolink { .. }));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fails_when_only_is_alphanumeric_differs() {
        let repo = make_repo("org/alpha");
        // Same prefix and URL, but the existing link is alphanumeric while the
        // policy wants numeric-only. This is the drift that previously produced
        // a confusing "expected X, got X" message with identical URLs.
        let provider = MockProvider::new(vec![repo.clone()]).with_autolinks(
            "org/alpha",
            vec![autolink(
                3,
                "MC-",
                "https://example.atlassian.net/browse/MC-<num>",
                true,
            )],
        );

        let rule = EnsureAutolinkRule::new(config(
            "MC-",
            "https://example.atlassian.net/browse/MC-<num>",
            false,
        ));

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail {
                detail,
                remediation,
            } => {
                // The message must surface the differing flag so an identical
                // URL on both sides is no longer baffling.
                assert!(detail.contains("is_alphanumeric: true"));
                assert!(detail.contains("is_alphanumeric: false"));
                assert!(matches!(remediation.unwrap(), Remediation::Autolink { .. }));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
