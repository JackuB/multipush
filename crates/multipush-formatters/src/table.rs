use std::fmt::Write;

use tabled::settings::Style;
use tabled::{Table, Tabled};

use multipush_core::engine::executor::{ApplyReport, SettingsActionKind};
use multipush_core::formatter::{
    build_pr_action_map, format_branch_protection_summary, format_pr_summary,
    format_settings_summary, has_branch_protection_actions, has_settings_actions, Formatter,
    PolicyReport, RepoOutcome, Report,
};

#[derive(Tabled)]
struct Row {
    #[tabled(rename = "Repository")]
    repo: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Detail")]
    detail: String,
}

#[derive(Tabled)]
struct ApplyRow {
    #[tabled(rename = "Repository")]
    repo: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "PR")]
    pr: String,
}

#[derive(Tabled)]
struct SettingsRow {
    #[tabled(rename = "Repository")]
    repo: String,
    #[tabled(rename = "Policies")]
    policies: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Patch")]
    patch: String,
}

#[derive(Tabled)]
struct BranchProtectionRow {
    #[tabled(rename = "Repository")]
    repo: String,
    #[tabled(rename = "Branch")]
    branch: String,
    #[tabled(rename = "Policies")]
    policies: String,
    #[tabled(rename = "Action")]
    action: String,
    #[tabled(rename = "Patch")]
    patch: String,
}

pub struct TableFormatter {
    color: bool,
}

impl Default for TableFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl TableFormatter {
    pub fn new() -> Self {
        let color = !std::env::var("NO_COLOR").is_ok_and(|v| !v.is_empty())
            && std::io::IsTerminal::is_terminal(&std::io::stdout());
        Self { color }
    }

    pub fn with_color(color: bool) -> Self {
        Self { color }
    }

    fn format_status(&self, outcome: &RepoOutcome) -> String {
        let label = match outcome {
            RepoOutcome::Pass { .. } => "PASS",
            RepoOutcome::Fail { .. } => "FAIL",
            RepoOutcome::Skip { .. } => "SKIP",
            RepoOutcome::Error { .. } => "ERROR",
        };

        if !self.color {
            return label.to_string();
        }

        use owo_colors::OwoColorize;
        match outcome {
            RepoOutcome::Pass { .. } => label.green().to_string(),
            RepoOutcome::Fail { .. } => label.red().to_string(),
            RepoOutcome::Skip { .. } => label.yellow().to_string(),
            RepoOutcome::Error { .. } => label.bold().red().to_string(),
        }
    }

    fn format_detail(outcome: &RepoOutcome) -> &str {
        match outcome {
            RepoOutcome::Pass { detail } => detail,
            RepoOutcome::Fail { detail, .. } => detail,
            RepoOutcome::Skip { reason } => reason,
            RepoOutcome::Error { message } => message,
        }
    }

    fn format_policy(&self, policy: &PolicyReport) -> String {
        let mut out = String::new();

        let desc = policy
            .description
            .as_deref()
            .map(|d| format!("  {d}"))
            .unwrap_or_default();
        writeln!(out, "Policy: {}{desc}", policy.policy_name).unwrap();

        let rows: Vec<Row> = policy
            .repo_results
            .iter()
            .map(|rr| Row {
                repo: rr.repo_name.clone(),
                status: self.format_status(&rr.outcome),
                detail: Self::format_detail(&rr.outcome).to_string(),
            })
            .collect();

        if rows.is_empty() {
            writeln!(out, "  (no repositories matched)").unwrap();
        } else {
            let table = Table::new(rows).with(Style::sharp()).to_string();
            writeln!(out, "{table}").unwrap();
        }

        out
    }
}

impl Formatter for TableFormatter {
    fn name(&self) -> &str {
        "table"
    }

    fn format(&self, report: &Report) -> multipush_core::Result<String> {
        let mut out = String::new();

        for (i, policy) in report.results.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&self.format_policy(policy));
        }

        let s = &report.summary;
        write!(
            out,
            "Summary: {} pass, {} fail, {} skip, {} errors",
            s.passing, s.failing, s.skipped, s.errors
        )
        .unwrap();

        Ok(out)
    }

    fn format_apply(&self, apply_report: &ApplyReport) -> multipush_core::Result<String> {
        let report = &apply_report.report;
        let action_map = build_pr_action_map(apply_report);

        let mut out = String::new();

        for (i, policy) in report.results.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }

            let desc = policy
                .description
                .as_deref()
                .map(|d| format!("  {d}"))
                .unwrap_or_default();
            writeln!(out, "Policy: {}{desc}", policy.policy_name).unwrap();

            let rows: Vec<ApplyRow> = policy
                .repo_results
                .iter()
                .map(|rr| {
                    let key = (rr.repo_name.clone(), policy.policy_name.clone());
                    let (action_label, pr_url) = action_map
                        .get(&key)
                        .map(|(a, u)| (a.clone(), u.clone()))
                        .unwrap_or_else(|| ("-".to_string(), "-".to_string()));

                    ApplyRow {
                        repo: rr.repo_name.clone(),
                        status: self.format_status(&rr.outcome),
                        action: action_label,
                        pr: pr_url,
                    }
                })
                .collect();

            if rows.is_empty() {
                writeln!(out, "  (no repositories matched)").unwrap();
            } else {
                let table = Table::new(rows).with(Style::sharp()).to_string();
                writeln!(out, "{table}").unwrap();
            }
        }

        if has_settings_actions(apply_report) {
            out.push('\n');
            writeln!(out, "Repo settings updates:").unwrap();
            let mut rows: Vec<SettingsRow> = Vec::new();
            for a in &apply_report.settings_applied {
                let label = match a.action {
                    SettingsActionKind::Applied => "settings updated",
                    SettingsActionKind::DryRun => "would update settings",
                    SettingsActionKind::Error => "error",
                };
                rows.push(SettingsRow {
                    repo: a.repo_name.clone(),
                    policies: a.policy_names.join(", "),
                    action: label.to_string(),
                    patch: format_patch(&a.patch),
                });
            }
            for a in &apply_report.settings_errored {
                rows.push(SettingsRow {
                    repo: a.repo_name.clone(),
                    policies: a.policy_names.join(", "),
                    action: a
                        .error
                        .clone()
                        .map(|e| format!("error: {e}"))
                        .unwrap_or_else(|| "error".to_string()),
                    patch: format_patch(&a.patch),
                });
            }
            let table = Table::new(rows).with(Style::sharp()).to_string();
            writeln!(out, "{table}").unwrap();
        }

        if has_branch_protection_actions(apply_report) {
            out.push('\n');
            writeln!(out, "Branch protection updates:").unwrap();
            let mut rows: Vec<BranchProtectionRow> = Vec::new();
            for a in &apply_report.branch_protection_applied {
                let label = match a.action {
                    SettingsActionKind::Applied => "protection updated",
                    SettingsActionKind::DryRun => "would update protection",
                    SettingsActionKind::Error => "error",
                };
                rows.push(BranchProtectionRow {
                    repo: a.repo_name.clone(),
                    branch: a.branch.clone(),
                    policies: a.policy_names.join(", "),
                    action: label.to_string(),
                    patch: format_branch_protection_patch(&a.patch),
                });
            }
            for a in &apply_report.branch_protection_errored {
                rows.push(BranchProtectionRow {
                    repo: a.repo_name.clone(),
                    branch: a.branch.clone(),
                    policies: a.policy_names.join(", "),
                    action: a
                        .error
                        .clone()
                        .map(|e| format!("error: {e}"))
                        .unwrap_or_else(|| "error".to_string()),
                    patch: format_branch_protection_patch(&a.patch),
                });
            }
            let table = Table::new(rows).with(Style::sharp()).to_string();
            writeln!(out, "{table}").unwrap();
        }

        let s = &report.summary;
        write!(
            out,
            "Summary: {} pass, {} fail, {} skip, {} errors | PRs: {} | Settings: {} | Branch protection: {}",
            s.passing,
            s.failing,
            s.skipped,
            s.errors,
            format_pr_summary(apply_report),
            format_settings_summary(apply_report),
            format_branch_protection_summary(apply_report),
        )
        .unwrap();

        Ok(out)
    }
}

fn format_patch(patch: &multipush_core::model::RepoSettingsPatch) -> String {
    serde_json::to_string(patch).unwrap_or_else(|_| "<unserializable>".to_string())
}

fn format_branch_protection_patch(
    patch: &multipush_core::model::BranchProtectionPatch,
) -> String {
    serde_json::to_string(patch).unwrap_or_else(|_| "<unserializable>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::engine::executor::{PrAction, PrActionKind};
    use multipush_core::formatter::{RepoResult, Summary};
    use multipush_core::model::{PrState, PullRequest, Severity};

    fn make_report(policies: Vec<PolicyReport>, summary: Summary) -> Report {
        Report {
            results: policies,
            summary,
        }
    }

    #[test]
    fn single_policy_mixed_outcomes() {
        let report = make_report(
            vec![PolicyReport {
                policy_name: "require-license".to_string(),
                description: Some("All repos must have a LICENSE".to_string()),
                severity: Severity::Error,
                repo_results: vec![
                    RepoResult {
                        repo_name: "org/alpha".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Pass {
                            detail: "File LICENSE exists".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/beta".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "File LICENSE does not exist".to_string(),
                            remediations: vec![],
                        },
                    },
                    RepoResult {
                        repo_name: "org/gamma".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Skip {
                            reason: "Repo is archived".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/delta".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Error {
                            message: "API rate limit".to_string(),
                        },
                    },
                ],
            }],
            Summary {
                total_repos: 4,
                passing: 1,
                failing: 1,
                skipped: 1,
                errors: 1,
            },
        );

        let formatter = TableFormatter::with_color(false);
        let output = formatter.format(&report).unwrap();

        assert!(output.contains("Policy: require-license"));
        assert!(output.contains("All repos must have a LICENSE"));
        assert!(output.contains("org/alpha"));
        assert!(output.contains("PASS"));
        assert!(output.contains("FAIL"));
        assert!(output.contains("SKIP"));
        assert!(output.contains("ERROR"));
        assert!(output.contains("Summary: 1 pass, 1 fail, 1 skip, 1 errors"));
    }

    #[test]
    fn multiple_policies_separated_by_blank_line() {
        let report = make_report(
            vec![
                PolicyReport {
                    policy_name: "policy-a".to_string(),
                    description: None,
                    severity: Severity::Warning,
                    repo_results: vec![RepoResult {
                        repo_name: "org/one".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Pass {
                            detail: "ok".to_string(),
                        },
                    }],
                },
                PolicyReport {
                    policy_name: "policy-b".to_string(),
                    description: None,
                    severity: Severity::Error,
                    repo_results: vec![RepoResult {
                        repo_name: "org/two".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "not ok".to_string(),
                            remediations: vec![],
                        },
                    }],
                },
            ],
            Summary {
                total_repos: 2,
                passing: 1,
                failing: 1,
                skipped: 0,
                errors: 0,
            },
        );

        let formatter = TableFormatter::with_color(false);
        let output = formatter.format(&report).unwrap();

        assert!(output.contains("Policy: policy-a"));
        assert!(output.contains("Policy: policy-b"));
        // Blank line separates policies
        assert!(output.contains("\n\nPolicy: policy-b"));
        assert!(output.contains("Summary: 1 pass, 1 fail, 0 skip, 0 errors"));
    }

    #[test]
    fn empty_report() {
        let report = make_report(
            vec![],
            Summary {
                total_repos: 0,
                passing: 0,
                failing: 0,
                skipped: 0,
                errors: 0,
            },
        );

        let formatter = TableFormatter::with_color(false);
        let output = formatter.format(&report).unwrap();

        assert_eq!(output, "Summary: 0 pass, 0 fail, 0 skip, 0 errors");
    }

    #[test]
    fn format_apply_with_pr_actions() {
        let report = make_report(
            vec![PolicyReport {
                policy_name: "require-license".to_string(),
                description: Some("All repos must have a LICENSE".to_string()),
                severity: Severity::Error,
                repo_results: vec![
                    RepoResult {
                        repo_name: "org/alpha".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Pass {
                            detail: "File LICENSE exists".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/beta".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "File LICENSE does not exist".to_string(),
                            remediations: vec![],
                        },
                    },
                ],
            }],
            Summary {
                total_repos: 2,
                passing: 1,
                failing: 1,
                skipped: 0,
                errors: 0,
            },
        );

        let apply_report = ApplyReport {
            report,
            prs_created: vec![PrAction {
                repo_name: "org/beta".to_string(),
                policy_name: "require-license".to_string(),
                branch: "multipush/require-license".to_string(),
                pr: Some(PullRequest {
                    number: 7,
                    title: "Add LICENSE".to_string(),
                    head_branch: "multipush/require-license".to_string(),
                    url: "https://github.com/org/beta/pull/7".to_string(),
                    state: PrState::Open,
                }),
                action: PrActionKind::Created,
                error: None,
            }],
            prs_updated: vec![],
            prs_skipped: vec![],
            prs_errored: vec![],
            prs_limited: 0,
            settings_applied: vec![],
            settings_errored: vec![],
            branch_protection_applied: vec![],
            branch_protection_errored: vec![],
        };

        let formatter = TableFormatter::with_color(false);
        let output = formatter.format_apply(&apply_report).unwrap();

        assert!(output.contains("Policy: require-license"));
        assert!(output.contains("PR created"));
        assert!(output.contains("https://github.com/org/beta/pull/7"));
        assert!(output.contains("PRs: 1 created"));
    }
}
