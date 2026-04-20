use async_trait::async_trait;
use tracing::debug;

use multipush_core::config::RepoSettingsConfig;
use multipush_core::model::{RepoSettings, RepoSettingsPatch};
use multipush_core::rule::{Remediation, Rule, RuleContext, RuleResult};

pub struct RepoSettingsRule {
    config: RepoSettingsConfig,
}

impl RepoSettingsRule {
    pub fn new(config: RepoSettingsConfig) -> Self {
        Self { config }
    }

    fn diff(&self, actual: &RepoSettings) -> (Vec<String>, RepoSettingsPatch) {
        let mut mismatches = Vec::new();
        let mut patch = RepoSettingsPatch::default();

        if let Some(expected) = self.config.has_issues {
            if actual.has_issues != expected {
                mismatches.push(format!(
                    "has_issues: actual={} expected={}",
                    actual.has_issues, expected
                ));
                patch.has_issues = Some(expected);
            }
        }
        if let Some(expected) = self.config.has_wiki {
            if actual.has_wiki != expected {
                mismatches.push(format!(
                    "has_wiki: actual={} expected={}",
                    actual.has_wiki, expected
                ));
                patch.has_wiki = Some(expected);
            }
        }
        if let Some(expected) = self.config.has_projects {
            if actual.has_projects != expected {
                mismatches.push(format!(
                    "has_projects: actual={} expected={}",
                    actual.has_projects, expected
                ));
                patch.has_projects = Some(expected);
            }
        }
        if let Some(expected) = self.config.allow_merge_commit {
            if actual.allow_merge_commit != expected {
                mismatches.push(format!(
                    "allow_merge_commit: actual={} expected={}",
                    actual.allow_merge_commit, expected
                ));
                patch.allow_merge_commit = Some(expected);
            }
        }
        if let Some(expected) = self.config.allow_squash_merge {
            if actual.allow_squash_merge != expected {
                mismatches.push(format!(
                    "allow_squash_merge: actual={} expected={}",
                    actual.allow_squash_merge, expected
                ));
                patch.allow_squash_merge = Some(expected);
            }
        }
        if let Some(expected) = self.config.allow_rebase_merge {
            if actual.allow_rebase_merge != expected {
                mismatches.push(format!(
                    "allow_rebase_merge: actual={} expected={}",
                    actual.allow_rebase_merge, expected
                ));
                patch.allow_rebase_merge = Some(expected);
            }
        }
        if let Some(expected) = self.config.delete_branch_on_merge {
            if actual.delete_branch_on_merge != expected {
                mismatches.push(format!(
                    "delete_branch_on_merge: actual={} expected={}",
                    actual.delete_branch_on_merge, expected
                ));
                patch.delete_branch_on_merge = Some(expected);
            }
        }
        if let Some(expected) = self.config.allow_auto_merge {
            if actual.allow_auto_merge != expected {
                mismatches.push(format!(
                    "allow_auto_merge: actual={} expected={}",
                    actual.allow_auto_merge, expected
                ));
                patch.allow_auto_merge = Some(expected);
            }
        }
        if let Some(ref expected) = self.config.default_branch {
            if &actual.default_branch != expected {
                mismatches.push(format!(
                    "default_branch: actual={} expected={}",
                    actual.default_branch, expected
                ));
                patch.default_branch = Some(expected.clone());
            }
        }

        (mismatches, patch)
    }
}

#[async_trait]
impl Rule for RepoSettingsRule {
    fn rule_type(&self) -> &str {
        "repo_settings"
    }

    fn description(&self) -> String {
        "Ensure repository settings match policy".to_string()
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        debug!(repo = %ctx.repo.full_name, "evaluating repo_settings rule");

        let actual = ctx.provider.get_repo_settings(ctx.repo).await?;
        let (mismatches, patch) = self.diff(&actual);

        if mismatches.is_empty() {
            return Ok(RuleResult::Pass {
                detail: "Repository settings match policy".to_string(),
            });
        }

        let detail = format!("Settings differ: {}", mismatches.join(", "));
        let description = format!("Update repo settings: {}", mismatches.join(", "));

        Ok(RuleResult::Fail {
            detail,
            remediation: Some(Remediation::RepoSettings { description, patch }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::testing::{make_repo, MockProvider};

    fn settings(has_wiki: bool, has_projects: bool, delete_branch_on_merge: bool) -> RepoSettings {
        RepoSettings {
            has_issues: true,
            has_wiki,
            has_projects,
            allow_merge_commit: true,
            allow_squash_merge: true,
            allow_rebase_merge: false,
            delete_branch_on_merge,
            default_branch: "main".to_string(),
            allow_auto_merge: false,
        }
    }

    #[tokio::test]
    async fn passes_when_matching() {
        let repo = make_repo("org/alpha");
        let provider = MockProvider::new(vec![repo.clone()])
            .with_repo_settings("org/alpha", settings(false, false, true));

        let rule = RepoSettingsRule::new(RepoSettingsConfig {
            has_wiki: Some(false),
            has_projects: Some(false),
            delete_branch_on_merge: Some(true),
            ..Default::default()
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        assert!(matches!(result, RuleResult::Pass { .. }));
    }

    #[tokio::test]
    async fn fails_with_minimal_patch() {
        let repo = make_repo("org/alpha");
        // actual differs in two of three declared fields.
        let provider = MockProvider::new(vec![repo.clone()])
            .with_repo_settings("org/alpha", settings(true, true, true));

        let rule = RepoSettingsRule::new(RepoSettingsConfig {
            has_wiki: Some(false),
            has_projects: Some(false),
            delete_branch_on_merge: Some(true),
            ..Default::default()
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { remediation, .. } => match remediation.unwrap() {
                Remediation::RepoSettings { patch, .. } => {
                    assert_eq!(patch.has_wiki, Some(false));
                    assert_eq!(patch.has_projects, Some(false));
                    // delete_branch_on_merge matched, so should not be in patch
                    assert_eq!(patch.delete_branch_on_merge, None);
                }
                other => panic!("expected RepoSettings remediation, got {other:?}"),
            },
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
