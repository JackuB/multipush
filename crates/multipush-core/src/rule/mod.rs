use async_trait::async_trait;

use serde::{Deserialize, Serialize};

use crate::model::{AutolinkSpec, BranchProtectionPatch, FileChange, Repo, RepoSettingsPatch};
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
    BranchProtection {
        description: String,
        branch: String,
        patch: BranchProtectionPatch,
    },
    /// Ensure a single autolink reference exists on the repo. Autolinks are
    /// keyed by `key_prefix`; the executor creates the link (replacing any
    /// existing link on the same prefix that differs).
    Autolink {
        description: String,
        spec: AutolinkSpec,
    },
}

impl Remediation {
    pub fn description(&self) -> &str {
        match self {
            Self::FileChanges { description, .. } => description,
            Self::RepoSettings { description, .. } => description,
            Self::BranchProtection { description, .. } => description,
            Self::Autolink { description, .. } => description,
        }
    }
}

/// A [`Remediation`] paired with the type of rule that produced it.
///
/// The evaluator attaches `rule_type` after `Rule::evaluate` returns, so
/// individual `Rule` implementations don't need to thread it through every
/// construction site. Consumers (PR body, SARIF) use it for provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributedRemediation {
    pub rule_type: String,
    #[serde(flatten)]
    pub remediation: Remediation,
}

impl AttributedRemediation {
    pub fn new(rule_type: impl Into<String>, remediation: Remediation) -> Self {
        Self {
            rule_type: rule_type.into(),
            remediation,
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
