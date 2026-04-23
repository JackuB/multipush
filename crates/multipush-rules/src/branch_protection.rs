use async_trait::async_trait;
use tracing::debug;

use multipush_core::config::{
    BranchProtectionConfig, RequiredPullRequestReviewsConfig, RequiredStatusChecksConfig,
};
use multipush_core::model::{
    BranchProtection, BranchProtectionPatch, RequiredPullRequestReviews, RequiredStatusChecks,
};
use multipush_core::rule::{Remediation, Rule, RuleContext, RuleResult};

pub struct BranchProtectionRule {
    config: BranchProtectionConfig,
}

impl BranchProtectionRule {
    pub fn new(config: BranchProtectionConfig) -> Self {
        Self { config }
    }

    fn diff(&self, actual: Option<&BranchProtection>) -> (Vec<String>, BranchProtectionPatch) {
        let mut mismatches = Vec::new();
        let mut patch = BranchProtectionPatch::default();

        if let Some(expected) = &self.config.required_status_checks {
            let expected_model = to_required_status_checks(expected);
            let actual_model = actual.and_then(|a| a.required_status_checks.as_ref());
            if actual_model != Some(&expected_model) {
                mismatches.push(format!(
                    "required_status_checks: actual={:?} expected={:?}",
                    actual_model, expected_model
                ));
                patch.required_status_checks = Some(expected_model);
            }
        }

        if let Some(expected) = &self.config.required_pull_request_reviews {
            let expected_model = to_required_pr_reviews(expected);
            let actual_model = actual.and_then(|a| a.required_pull_request_reviews.as_ref());
            if actual_model != Some(&expected_model) {
                mismatches.push(format!(
                    "required_pull_request_reviews: actual={:?} expected={:?}",
                    actual_model, expected_model
                ));
                patch.required_pull_request_reviews = Some(expected_model);
            }
        }

        if let Some(expected) = self.config.enforce_admins {
            let actual_val = actual.map(|a| a.enforce_admins).unwrap_or(false);
            if actual_val != expected {
                mismatches.push(format!(
                    "enforce_admins: actual={} expected={}",
                    actual_val, expected
                ));
                patch.enforce_admins = Some(expected);
            }
        }

        if let Some(expected) = self.config.required_linear_history {
            let actual_val = actual.map(|a| a.required_linear_history).unwrap_or(false);
            if actual_val != expected {
                mismatches.push(format!(
                    "required_linear_history: actual={} expected={}",
                    actual_val, expected
                ));
                patch.required_linear_history = Some(expected);
            }
        }

        if let Some(expected) = self.config.allow_force_pushes {
            let actual_val = actual.map(|a| a.allow_force_pushes).unwrap_or(false);
            if actual_val != expected {
                mismatches.push(format!(
                    "allow_force_pushes: actual={} expected={}",
                    actual_val, expected
                ));
                patch.allow_force_pushes = Some(expected);
            }
        }

        if let Some(expected) = self.config.allow_deletions {
            let actual_val = actual.map(|a| a.allow_deletions).unwrap_or(false);
            if actual_val != expected {
                mismatches.push(format!(
                    "allow_deletions: actual={} expected={}",
                    actual_val, expected
                ));
                patch.allow_deletions = Some(expected);
            }
        }

        (mismatches, patch)
    }
}

fn to_required_status_checks(cfg: &RequiredStatusChecksConfig) -> RequiredStatusChecks {
    RequiredStatusChecks {
        strict: cfg.strict,
        contexts: cfg.contexts.clone(),
    }
}

fn to_required_pr_reviews(cfg: &RequiredPullRequestReviewsConfig) -> RequiredPullRequestReviews {
    RequiredPullRequestReviews {
        required_approving_review_count: cfg.required_approving_review_count,
        dismiss_stale_reviews: cfg.dismiss_stale_reviews,
        require_code_owner_reviews: cfg.require_code_owner_reviews,
    }
}

#[async_trait]
impl Rule for BranchProtectionRule {
    fn rule_type(&self) -> &str {
        "branch_protection"
    }

    fn description(&self) -> String {
        "Ensure branch protection matches policy".to_string()
    }

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> multipush_core::Result<RuleResult> {
        let branch = self
            .config
            .branch
            .clone()
            .unwrap_or_else(|| ctx.repo.default_branch.clone());

        debug!(repo = %ctx.repo.full_name, branch = %branch, "evaluating branch_protection rule");

        let actual = ctx
            .provider
            .get_branch_protection(ctx.repo, &branch)
            .await?;
        let (mismatches, patch) = self.diff(actual.as_ref());

        if mismatches.is_empty() {
            return Ok(RuleResult::Pass {
                detail: format!("Branch protection on '{branch}' matches policy"),
            });
        }

        let detail = format!(
            "Branch protection on '{branch}' differs: {}",
            mismatches.join(", ")
        );
        let description = format!(
            "Update branch protection on '{branch}': {}",
            mismatches.join(", ")
        );

        Ok(RuleResult::Fail {
            detail,
            remediation: Some(Remediation::BranchProtection {
                description,
                branch,
                patch,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::testing::{make_repo, MockProvider};

    fn protection(reviews: u32, enforce_admins: bool) -> BranchProtection {
        BranchProtection {
            required_status_checks: None,
            required_pull_request_reviews: Some(RequiredPullRequestReviews {
                required_approving_review_count: reviews,
                dismiss_stale_reviews: false,
                require_code_owner_reviews: false,
            }),
            enforce_admins,
            required_linear_history: false,
            allow_force_pushes: false,
            allow_deletions: false,
        }
    }

    #[tokio::test]
    async fn passes_when_matching() {
        let repo = make_repo("org/alpha");
        let provider = MockProvider::new(vec![repo.clone()])
            .with_branch_protection("org/alpha:main", protection(1, true));

        let rule = BranchProtectionRule::new(BranchProtectionConfig {
            required_pull_request_reviews: Some(RequiredPullRequestReviewsConfig {
                required_approving_review_count: 1,
                ..Default::default()
            }),
            enforce_admins: Some(true),
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
        // required reviews matches but enforce_admins differs
        let provider = MockProvider::new(vec![repo.clone()])
            .with_branch_protection("org/alpha:main", protection(1, false));

        let rule = BranchProtectionRule::new(BranchProtectionConfig {
            required_pull_request_reviews: Some(RequiredPullRequestReviewsConfig {
                required_approving_review_count: 1,
                ..Default::default()
            }),
            enforce_admins: Some(true),
            ..Default::default()
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { remediation, .. } => match remediation.unwrap() {
                Remediation::BranchProtection { branch, patch, .. } => {
                    assert_eq!(branch, "main");
                    assert_eq!(patch.enforce_admins, Some(true));
                    // matching field should not be in patch
                    assert!(patch.required_pull_request_reviews.is_none());
                }
                other => panic!("expected BranchProtection remediation, got {other:?}"),
            },
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn treats_missing_protection_as_all_defaults() {
        let repo = make_repo("org/alpha");
        // No protection configured at all on the branch.
        let provider = MockProvider::new(vec![repo.clone()]);

        let rule = BranchProtectionRule::new(BranchProtectionConfig {
            enforce_admins: Some(true),
            ..Default::default()
        });

        let ctx = RuleContext {
            provider: &provider,
            repo: &repo,
        };
        let result = rule.evaluate(&ctx).await.unwrap();
        match result {
            RuleResult::Fail { remediation, .. } => match remediation.unwrap() {
                Remediation::BranchProtection { patch, .. } => {
                    assert_eq!(patch.enforce_admins, Some(true));
                }
                other => panic!("expected BranchProtection remediation, got {other:?}"),
            },
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
