use async_trait::async_trait;

use serde::{Deserialize, Serialize};

use crate::model::{FileChange, Repo};
use crate::provider::Provider;
use crate::Result;

#[derive(Debug, Clone)]
pub enum RuleResult {
    Pass {
        detail: String,
    },
    Fail {
        detail: String,
        remediation: Option<Remediation>,
    },
    Skip {
        reason: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remediation {
    pub description: String,
    pub changes: Vec<FileChange>,
}

pub struct RuleContext<'a> {
    pub provider: &'a dyn Provider,
    pub repo: &'a Repo,
}

#[async_trait]
pub trait Rule: Send + Sync {
    fn rule_type(&self) -> &str;

    fn description(&self) -> String;

    async fn evaluate(&self, ctx: &RuleContext<'_>) -> Result<RuleResult>;
}
