use async_trait::async_trait;

use serde::{Deserialize, Serialize};

use crate::model::{FileChange, Repo, RepoSettingsPatch};
use crate::provider::Provider;
use crate::Result;

/// The outcome of evaluating a single rule against a repository.
#[derive(Debug, Clone)]
pub enum RuleResult {
    /// The repository satisfies the rule.
    Pass { detail: String },
    /// The repository violates the rule, with an optional remediation.
    Fail {
        detail: String,
        remediation: Option<Remediation>,
    },
    /// The rule was not applicable to this repository.
    Skip { reason: String },
    /// An error occurred during evaluation.
    Error { message: String },
}

/// A remediation that can fix a rule violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Remediation {
    FileChanges {
        description: String,
        changes: Vec<FileChange>,
    },
    RepoSettings {
        description: String,
        patch: RepoSettingsPatch,
    },
}

impl Remediation {
    pub fn description(&self) -> &str {
        match self {
            Self::FileChanges { description, .. } => description,
            Self::RepoSettings { description, .. } => description,
        }
    }
}

/// Context passed to each rule evaluation, providing access to the
/// provider and the target repository.
pub struct RuleContext<'a> {
    pub provider: &'a dyn Provider,
    pub repo: &'a Repo,
}

/// A compliance rule that can be evaluated against a repository.
#[async_trait]
pub trait Rule: Send + Sync {
    /// Machine-readable rule type identifier (e.g. `"ensure_file"`).
    fn rule_type(&self) -> &str;

    /// Human-readable description of what this rule checks.
    fn description(&self) -> String;

    /// Evaluate the rule against the repository in `ctx`.
    async fn evaluate(&self, ctx: &RuleContext<'_>) -> Result<RuleResult>;
}
