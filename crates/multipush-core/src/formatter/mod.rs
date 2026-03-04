use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::engine::executor::{ApplyReport, PrAction, PrActionKind};
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

/// Build a lookup from (repo_name, policy_name) to (action_label, pr_url) for apply reports.
pub fn build_pr_action_map(report: &ApplyReport) -> std::collections::HashMap<(String, String), (String, String)> {
    let mut map = std::collections::HashMap::new();

    fn insert_actions(
        map: &mut std::collections::HashMap<(String, String), (String, String)>,
        actions: &[PrAction],
    ) {
        for action in actions {
            let label = match action.action {
                PrActionKind::Created => "PR created".to_string(),
                PrActionKind::Updated => "PR updated".to_string(),
                PrActionKind::Skipped => "Skipped (existing)".to_string(),
                PrActionKind::DryRun => "Would create PR".to_string(),
            };
            let url = action
                .pr
                .as_ref()
                .map(|pr| pr.url.clone())
                .unwrap_or_else(|| "-".to_string());
            map.insert(
                (action.repo_name.clone(), action.policy_name.clone()),
                (label, url),
            );
        }
    }

    insert_actions(&mut map, &report.prs_created);
    insert_actions(&mut map, &report.prs_updated);
    insert_actions(&mut map, &report.prs_skipped);

    map
}

/// Build the PR summary line for apply reports.
pub fn format_pr_summary(report: &ApplyReport) -> String {
    let created = report.prs_created.iter().filter(|a| a.action == PrActionKind::Created).count();
    let would_create = report.prs_created.iter().filter(|a| a.action == PrActionKind::DryRun).count();
    let updated = report.prs_updated.iter().filter(|a| a.action == PrActionKind::Updated).count();
    let would_update = report.prs_updated.iter().filter(|a| a.action == PrActionKind::DryRun).count();
    let skipped = report.prs_skipped.len();
    let limited = report.prs_limited;

    let mut parts = Vec::new();
    if created > 0 { parts.push(format!("{created} created")); }
    if would_create > 0 { parts.push(format!("{would_create} would create")); }
    if updated > 0 { parts.push(format!("{updated} updated")); }
    if would_update > 0 { parts.push(format!("{would_update} would update")); }
    if skipped > 0 { parts.push(format!("{skipped} skipped")); }
    if limited > 0 { parts.push(format!("{limited} limited (max-prs)")); }

    if parts.is_empty() {
        "0 actions".to_string()
    } else {
        parts.join(", ")
    }
}

pub trait Formatter: Send + Sync {
    fn name(&self) -> &str;

    fn format(&self, report: &Report) -> Result<String>;

    /// Format an apply report. Default implementation delegates to `format()` with a PR summary.
    fn format_apply(&self, apply_report: &ApplyReport) -> Result<String> {
        let mut out = self.format(&apply_report.report)?;
        write!(out, "\nPRs: {}", format_pr_summary(apply_report)).unwrap();
        Ok(out)
    }
}
