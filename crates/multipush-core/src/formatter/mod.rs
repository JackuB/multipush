use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::engine::executor::{ApplyReport, PrAction, PrActionKind, SettingsActionKind};
use crate::model::Severity;
use crate::rule::{AttributedRemediation, Remediation};
use crate::Result;

/// Top-level check report containing per-policy results and an aggregate summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub results: Vec<PolicyReport>,
    pub summary: Summary,
}

/// Results of evaluating a single policy across all targeted repositories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyReport {
    pub policy_name: String,
    pub description: Option<String>,
    pub severity: Severity,
    pub repo_results: Vec<RepoResult>,
}

/// The evaluation result for one repository under one policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoResult {
    pub repo_name: String,
    pub default_branch: String,
    pub outcome: RepoOutcome,
}

/// Aggregated outcome for a repository: pass, fail, skip, or error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RepoOutcome {
    Pass {
        detail: String,
    },
    Fail {
        detail: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        remediations: Vec<AttributedRemediation>,
    },
    Skip {
        reason: String,
    },
    Error {
        message: String,
    },
}

/// Aggregate counts across all repositories in a report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub total_repos: usize,
    pub passing: usize,
    pub failing: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl Summary {
    /// Repos where a pass/fail verdict was actually reached (excludes skipped and errored).
    pub fn evaluated(&self) -> usize {
        self.passing + self.failing
    }

    /// Pass rate as `pass / (pass + fail)` in percent. `None` when nothing was evaluated.
    pub fn success_rate(&self) -> Option<f64> {
        let denom = self.evaluated();
        if denom == 0 {
            None
        } else {
            Some((self.passing as f64 / denom as f64) * 100.0)
        }
    }
}

/// Per-policy counts for the per-policy summary footer.
#[derive(Debug, Clone, Default)]
pub struct PolicyCounts {
    pub passing: usize,
    pub failing: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl PolicyCounts {
    pub fn from_policy(policy: &PolicyReport) -> Self {
        let mut c = Self::default();
        for rr in &policy.repo_results {
            match &rr.outcome {
                RepoOutcome::Pass { .. } => c.passing += 1,
                RepoOutcome::Fail { .. } => c.failing += 1,
                RepoOutcome::Skip { .. } => c.skipped += 1,
                RepoOutcome::Error { .. } => c.errors += 1,
            }
        }
        c
    }

    pub fn total(&self) -> usize {
        self.passing + self.failing + self.skipped + self.errors
    }

    pub fn evaluated(&self) -> usize {
        self.passing + self.failing
    }

    pub fn success_rate(&self) -> Option<f64> {
        let denom = self.evaluated();
        if denom == 0 {
            None
        } else {
            Some((self.passing as f64 / denom as f64) * 100.0)
        }
    }
}

/// Build a lookup from (repo_name, policy_name) to (action_label, pr_url) for apply reports.
pub fn build_pr_action_map(
    report: &ApplyReport,
) -> std::collections::HashMap<(String, String), (String, String)> {
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
                PrActionKind::Error => "Error".to_string(),
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
    insert_actions(&mut map, &report.prs_errored);

    map
}

/// Compute the (action, pr_url) labels for a single (repo, policy) row in an
/// **apply** table.
///
/// Falls back to deriving a label from the outcome when there's no PR action
/// recorded, so the table can distinguish:
///
/// - `Report only` — FAIL with no remediation (rule found a problem but
///   doesn't know how to fix it; e.g. discovery-mode `ensure_file` with no
///   `default_content`).
/// - `Limited (max-prs)` — FAIL with a real file remediation that the
///   executor suppressed because the `--max-prs` budget was exhausted. Only
///   reached in apply mode — check has no budget concept and should use
///   [`derive_check_action`] instead.
/// - `-` — anything else (skipped, settings-only remediation handled in its
///   own table, etc.).
pub fn derive_pr_action(
    action_map: &std::collections::HashMap<(String, String), (String, String)>,
    repo_name: &str,
    policy_name: &str,
    outcome: &RepoOutcome,
) -> (String, String) {
    if let Some((label, url)) = action_map.get(&(repo_name.to_string(), policy_name.to_string())) {
        return (label.clone(), url.clone());
    }
    match outcome {
        RepoOutcome::Fail { remediations, .. } => {
            let has_file_changes = remediations.iter().any(|r| {
                matches!(
                    &r.remediation,
                    Remediation::FileChanges { changes, .. } if !changes.is_empty()
                )
            });
            if has_file_changes {
                ("Limited (max-prs)".to_string(), "-".to_string())
            } else if remediations.is_empty() {
                ("Report only".to_string(), "-".to_string())
            } else {
                ("-".to_string(), "-".to_string())
            }
        }
        _ => ("-".to_string(), "-".to_string()),
    }
}

/// Compute the Action label for a single (repo, policy) row in a **check**
/// table. There is no action_map in check mode — the label answers "what
/// would `apply` do here?" by inspecting the remediations the rule emitted.
///
/// Returns:
/// - `Would create PR` if at least one remediation is a non-empty
///   `FileChanges`.
/// - `Would update settings` / `Would update protection` for the
///   corresponding API-only remediations.
/// - Both kinds of API remediations combine with `+` (e.g.
///   `Would update settings + protection`); when a PR is also pending, the
///   PR wins (apply will open a PR *and* PATCH settings, but the PR is the
///   more reviewable action so it leads).
/// - `Report only` for a FAIL with no remediation (the rule detected a
///   problem it can't fix).
/// - `-` for non-FAIL outcomes.
pub fn derive_check_action(outcome: &RepoOutcome) -> String {
    let RepoOutcome::Fail { remediations, .. } = outcome else {
        return "-".to_string();
    };

    if remediations.is_empty() {
        return "Report only".to_string();
    }

    let mut would_pr = false;
    let mut would_settings = false;
    let mut would_protection = false;
    for r in remediations {
        match &r.remediation {
            Remediation::FileChanges { changes, .. } if !changes.is_empty() => would_pr = true,
            Remediation::FileChanges { .. } => {}
            Remediation::RepoSettings { .. } => would_settings = true,
            Remediation::BranchProtection { .. } => would_protection = true,
        }
    }

    if would_pr {
        return "Would create PR".to_string();
    }
    match (would_settings, would_protection) {
        (true, true) => "Would update settings + protection".to_string(),
        (true, false) => "Would update settings".to_string(),
        (false, true) => "Would update protection".to_string(),
        // Only FileChanges-with-empty-changes remediations were present.
        (false, false) => "Report only".to_string(),
    }
}

/// Build the check-mode PR summary line: how many PRs `apply` would create
/// if run against this report. Counts each (repo, policy) FAIL whose
/// remediation list contains at least one non-empty `FileChanges`.
pub fn format_pr_summary_for_check(report: &Report) -> String {
    let mut would_create = 0usize;
    for policy in &report.results {
        for rr in &policy.repo_results {
            if let RepoOutcome::Fail { remediations, .. } = &rr.outcome {
                if remediations.iter().any(|r| {
                    matches!(
                        &r.remediation,
                        Remediation::FileChanges { changes, .. } if !changes.is_empty()
                    )
                }) {
                    would_create += 1;
                }
            }
        }
    }
    if would_create == 0 {
        "0 actions".to_string()
    } else {
        format!("{would_create} would create")
    }
}

/// Build a settings-summary line for check mode using the planned actions
/// returned by [`crate::engine::plan_apply_actions`]. Mirrors the shape of
/// [`format_settings_summary`] so check output matches apply output.
pub fn format_settings_summary_for_check(planned: &[crate::engine::SettingsAction]) -> String {
    if planned.is_empty() {
        "0 actions".to_string()
    } else {
        format!("{} would apply", planned.len())
    }
}

/// Build a branch-protection summary line for check mode.
pub fn format_branch_protection_summary_for_check(
    planned: &[crate::engine::BranchProtectionAction],
) -> String {
    if planned.is_empty() {
        "0 actions".to_string()
    } else {
        format!("{} would apply", planned.len())
    }
}

/// Build the PR summary line for apply reports.
pub fn format_pr_summary(report: &ApplyReport) -> String {
    let created = report
        .prs_created
        .iter()
        .filter(|a| a.action == PrActionKind::Created)
        .count();
    let would_create = report
        .prs_created
        .iter()
        .filter(|a| a.action == PrActionKind::DryRun)
        .count();
    let updated = report
        .prs_updated
        .iter()
        .filter(|a| a.action == PrActionKind::Updated)
        .count();
    let would_update = report
        .prs_updated
        .iter()
        .filter(|a| a.action == PrActionKind::DryRun)
        .count();
    let skipped = report.prs_skipped.len();
    let limited = report.prs_limited;

    let mut parts = Vec::new();
    if created > 0 {
        parts.push(format!("{created} created"));
    }
    if would_create > 0 {
        parts.push(format!("{would_create} would create"));
    }
    if updated > 0 {
        parts.push(format!("{updated} updated"));
    }
    if would_update > 0 {
        parts.push(format!("{would_update} would update"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    if limited > 0 {
        parts.push(format!("{limited} limited (max-prs)"));
    }
    let errored = report.prs_errored.len();
    if errored > 0 {
        parts.push(format!("{errored} errored"));
    }

    if parts.is_empty() {
        "0 actions".to_string()
    } else {
        parts.join(", ")
    }
}

/// Build the settings-action summary line for apply reports.
pub fn format_settings_summary(report: &ApplyReport) -> String {
    let applied = report
        .settings_applied
        .iter()
        .filter(|a| a.action == SettingsActionKind::Applied)
        .count();
    let would_apply = report
        .settings_applied
        .iter()
        .filter(|a| a.action == SettingsActionKind::DryRun)
        .count();
    let errored = report.settings_errored.len();

    let mut parts = Vec::new();
    if applied > 0 {
        parts.push(format!("{applied} applied"));
    }
    if would_apply > 0 {
        parts.push(format!("{would_apply} would apply"));
    }
    if errored > 0 {
        parts.push(format!("{errored} errored"));
    }

    if parts.is_empty() {
        "0 actions".to_string()
    } else {
        parts.join(", ")
    }
}

/// True if the apply report has any settings actions (applied, would-apply, or errored).
pub fn has_settings_actions(report: &ApplyReport) -> bool {
    !report.settings_applied.is_empty() || !report.settings_errored.is_empty()
}

/// Build the branch-protection summary line for apply reports.
pub fn format_branch_protection_summary(report: &ApplyReport) -> String {
    let applied = report
        .branch_protection_applied
        .iter()
        .filter(|a| a.action == SettingsActionKind::Applied)
        .count();
    let would_apply = report
        .branch_protection_applied
        .iter()
        .filter(|a| a.action == SettingsActionKind::DryRun)
        .count();
    let errored = report.branch_protection_errored.len();

    let mut parts = Vec::new();
    if applied > 0 {
        parts.push(format!("{applied} applied"));
    }
    if would_apply > 0 {
        parts.push(format!("{would_apply} would apply"));
    }
    if errored > 0 {
        parts.push(format!("{errored} errored"));
    }

    if parts.is_empty() {
        "0 actions".to_string()
    } else {
        parts.join(", ")
    }
}

/// True if the apply report has any branch-protection actions.
pub fn has_branch_protection_actions(report: &ApplyReport) -> bool {
    !report.branch_protection_applied.is_empty() || !report.branch_protection_errored.is_empty()
}

/// Renders a [`Report`] or [`ApplyReport`] into a human-readable string.
pub trait Formatter: Send + Sync {
    /// Formatter identifier (e.g. `"table"`, `"json"`, `"markdown"`).
    fn name(&self) -> &str;

    /// Format a check-mode report, grouped by policy (default view).
    fn format(&self, report: &Report) -> Result<String>;

    /// Format a check-mode report, grouped by repository.
    ///
    /// Default implementation falls back to [`Formatter::format`]; formatters
    /// that have a meaningful per-repo presentation should override this.
    fn format_by_repo(&self, report: &Report) -> Result<String> {
        self.format(report)
    }

    /// Format an apply report. Default implementation delegates to `format()` with a PR summary.
    fn format_apply(&self, apply_report: &ApplyReport) -> Result<String> {
        let mut out = self.format(&apply_report.report)?;
        write!(out, "\nPRs: {}", format_pr_summary(apply_report)).unwrap();
        Ok(out)
    }
}

/// Group a report's per-policy results into `repo_name -> [(policy_name, &RepoResult)]`.
///
/// Repositories appear in sorted order; within each repo, policy results are
/// in the order the policies were declared in the config.
pub fn group_by_repo(
    report: &Report,
) -> std::collections::BTreeMap<&str, Vec<(&str, &RepoResult)>> {
    let mut map: std::collections::BTreeMap<&str, Vec<(&str, &RepoResult)>> =
        std::collections::BTreeMap::new();
    for policy in &report.results {
        for rr in &policy.repo_results {
            map.entry(rr.repo_name.as_str())
                .or_default()
                .push((policy.policy_name.as_str(), rr));
        }
    }
    map
}

/// Per-repository tally across all policies the repo was evaluated under.
#[derive(Debug, Clone, Default)]
pub struct RepoCounts {
    pub passing: usize,
    pub failing: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl RepoCounts {
    pub fn from_results(results: &[(&str, &RepoResult)]) -> Self {
        let mut c = Self::default();
        for (_, rr) in results {
            match &rr.outcome {
                RepoOutcome::Pass { .. } => c.passing += 1,
                RepoOutcome::Fail { .. } => c.failing += 1,
                RepoOutcome::Skip { .. } => c.skipped += 1,
                RepoOutcome::Error { .. } => c.errors += 1,
            }
        }
        c
    }

    pub fn total(&self) -> usize {
        self.passing + self.failing + self.skipped + self.errors
    }
}
