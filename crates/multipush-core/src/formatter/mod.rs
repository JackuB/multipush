use serde::{Deserialize, Serialize};

use crate::model::Severity;
use crate::rule::Remediation;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub results: Vec<PolicyReport>,
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReport {
    pub policy_name: String,
    pub description: Option<String>,
    pub severity: Severity,
    pub repo_results: Vec<RepoResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoResult {
    pub repo_name: String,
    pub default_branch: String,
    pub outcome: RepoOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RepoOutcome {
    Pass {
        detail: String,
    },
    Fail {
        detail: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remediations: Vec<Remediation>,
    },
    Skip {
        reason: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total_repos: usize,
    pub passing: usize,
    pub failing: usize,
    pub skipped: usize,
    pub errors: usize,
}

pub trait Formatter: Send + Sync {
    fn name(&self) -> &str;

    fn format(&self, report: &Report) -> Result<String>;
}
