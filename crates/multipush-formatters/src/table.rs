use std::fmt::Write;

use tabled::settings::Style;
use tabled::{Table, Tabled};

use multipush_core::engine::executor::{
    ApplyReport, BranchProtectionAction, SettingsAction, SettingsActionKind,
};
use multipush_core::engine::plan_apply_actions;
use multipush_core::formatter::{
    build_pr_action_map, derive_check_action, derive_pr_action, format_branch_protection_summary,
    format_branch_protection_summary_for_check, format_pr_summary, format_pr_summary_for_check,
    format_settings_summary, format_settings_summary_for_check, group_by_repo,
    has_branch_protection_actions as has_apply_branch_protection_actions,
    has_settings_actions as has_apply_settings_actions, Formatter, PolicyCounts, PolicyReport,
    RepoCounts, RepoOutcome, Report,
};

#[derive(Tabled)]
struct CheckRow {
    #[tabled(rename = "Repository")]
    repo: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Detail")]
    detail: String,
    #[tabled(rename = "Action")]
    action: String,
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

    /// Short single-character glyph for compact list views.
    fn format_glyph(&self, outcome: &RepoOutcome) -> String {
        let g = match outcome {
            RepoOutcome::Pass { .. } => "✓",
            RepoOutcome::Fail { .. } => "✗",
            RepoOutcome::Skip { .. } => "•",
            RepoOutcome::Error { .. } => "!",
        };

        if !self.color {
            return g.to_string();
        }

        use owo_colors::OwoColorize;
        match outcome {
            RepoOutcome::Pass { .. } => g.green().to_string(),
            RepoOutcome::Fail { .. } => g.red().to_string(),
            RepoOutcome::Skip { .. } => g.yellow().to_string(),
            RepoOutcome::Error { .. } => g.bold().red().to_string(),
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

    /// Render the per-policy header + body (4-col table) and the policy
    /// summary line. `rows_for` builds the Tabled rows; this helper is shared
    /// by check (CheckRow) and apply (ApplyRow) rendering.
    fn render_policy_block<R: Tabled>(
        &self,
        policy: &PolicyReport,
        rows_for: impl FnOnce(&PolicyReport) -> Vec<R>,
    ) -> String {
        let mut out = String::new();
        let desc = policy
            .description
            .as_deref()
            .map(|d| format!("  {d}"))
            .unwrap_or_default();
        writeln!(out, "Policy: {}{desc}", policy.policy_name).unwrap();

        let rows = rows_for(policy);
        if rows.is_empty() {
            writeln!(out, "  (no repositories matched)").unwrap();
        } else {
            let table = Table::new(rows).with(Style::sharp()).to_string();
            writeln!(out, "{table}").unwrap();
            writeln!(
                out,
                "Policy summary: {}",
                format_counts_line(&PolicyCounts::from_policy(policy))
            )
            .unwrap();
        }
        out
    }
}

/// Compact counts summary used in the per-repo header. Only non-zero kinds
/// are listed so a clean repo reads `(2 pass)` instead of `(2 pass, 0 fail, …)`.
fn format_repo_counts(counts: &RepoCounts) -> String {
    let mut parts = Vec::new();
    if counts.passing > 0 {
        parts.push(format!("{} pass", counts.passing));
    }
    if counts.failing > 0 {
        parts.push(format!("{} fail", counts.failing));
    }
    if counts.skipped > 0 {
        parts.push(format!("{} skip", counts.skipped));
    }
    if counts.errors > 0 {
        parts.push(format!(
            "{} {}",
            counts.errors,
            if counts.errors == 1 {
                "error"
            } else {
                "errors"
            },
        ));
    }
    if parts.is_empty() {
        "no evaluations".to_string()
    } else {
        parts.join(", ")
    }
}

fn format_counts_line(counts: &PolicyCounts) -> String {
    let mut s = format!(
        "{} pass, {} fail, {} skip, {} {}",
        counts.passing,
        counts.failing,
        counts.skipped,
        counts.errors,
        if counts.errors == 1 {
            "error"
        } else {
            "errors"
        },
    );
    if let Some(rate) = counts.success_rate() {
        write!(s, " ({rate:.1}% pass)").unwrap();
    }
    s
}

fn format_overview(report: &Report) -> String {
    let s = &report.summary;
    let policies = report.results.len();
    let mut out = String::new();
    writeln!(out, "Overview").unwrap();
    writeln!(out, "────────").unwrap();
    writeln!(out, "Policies:     {policies}").unwrap();
    writeln!(out, "Repositories: {}", s.total_repos).unwrap();
    writeln!(out, "Pass:         {}", s.passing).unwrap();
    writeln!(out, "Fail:         {}", s.failing).unwrap();
    writeln!(out, "Skip:         {}", s.skipped).unwrap();
    writeln!(out, "Error:        {}", s.errors).unwrap();
    match s.success_rate() {
        Some(rate) => write!(
            out,
            "Success rate: {:.1}%  ({} pass / {} evaluated)",
            rate,
            s.passing,
            s.evaluated(),
        )
        .unwrap(),
        None => write!(out, "Success rate: n/a  (no repositories evaluated)").unwrap(),
    }
    out
}

fn render_settings_table(applied: &[SettingsAction], errored: &[SettingsAction]) -> String {
    let mut rows: Vec<SettingsRow> = Vec::new();
    for a in applied {
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
    for a in errored {
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
    Table::new(rows).with(Style::sharp()).to_string()
}

fn render_protection_table(
    applied: &[BranchProtectionAction],
    errored: &[BranchProtectionAction],
) -> String {
    let mut rows: Vec<BranchProtectionRow> = Vec::new();
    for a in applied {
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
    for a in errored {
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
    Table::new(rows).with(Style::sharp()).to_string()
}

impl Formatter for TableFormatter {
    fn name(&self) -> &str {
        "table"
    }

    fn format(&self, report: &Report) -> multipush_core::Result<String> {
        let mut out = String::new();
        let (planned_settings, planned_protection) = plan_apply_actions(report);

        for (i, policy) in report.results.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&self.render_policy_block(policy, |p| {
                p.repo_results
                    .iter()
                    .map(|rr| CheckRow {
                        repo: rr.repo_name.clone(),
                        status: self.format_status(&rr.outcome),
                        detail: Self::format_detail(&rr.outcome).to_string(),
                        action: derive_check_action(&rr.outcome),
                    })
                    .collect()
            }));
        }

        if !planned_settings.is_empty() {
            out.push('\n');
            writeln!(out, "Repo settings updates:").unwrap();
            writeln!(out, "{}", render_settings_table(&planned_settings, &[])).unwrap();
        }

        if !planned_protection.is_empty() {
            out.push('\n');
            writeln!(out, "Branch protection updates:").unwrap();
            writeln!(out, "{}", render_protection_table(&planned_protection, &[])).unwrap();
        }

        if !report.results.is_empty() {
            out.push('\n');
        }
        out.push_str(&format_overview(report));
        write!(
            out,
            "\nPRs:               {}\nSettings:          {}\nBranch protection: {}",
            format_pr_summary_for_check(report),
            format_settings_summary_for_check(&planned_settings),
            format_branch_protection_summary_for_check(&planned_protection),
        )
        .unwrap();

        Ok(out)
    }

    fn format_by_repo(&self, report: &Report) -> multipush_core::Result<String> {
        let mut out = String::new();
        let grouped = group_by_repo(report);

        for (i, (repo, results)) in grouped.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }

            let counts = RepoCounts::from_results(results);
            writeln!(
                out,
                "{repo}  — {} {} ({})",
                counts.total(),
                if counts.total() == 1 {
                    "policy"
                } else {
                    "policies"
                },
                format_repo_counts(&counts),
            )
            .unwrap();

            // Left-align policy names so details start at the same column.
            let policy_width = results
                .iter()
                .map(|(name, _)| name.chars().count())
                .max()
                .unwrap_or(0);

            for (policy_name, rr) in results {
                writeln!(
                    out,
                    "  {} {:<policy_width$}  {}",
                    self.format_glyph(&rr.outcome),
                    policy_name,
                    Self::format_detail(&rr.outcome),
                    policy_width = policy_width,
                )
                .unwrap();
            }
        }

        if !grouped.is_empty() {
            out.push('\n');
        }
        out.push_str(&format_overview(report));

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
            out.push_str(&self.render_policy_block(policy, |p| {
                p.repo_results
                    .iter()
                    .map(|rr| {
                        let (action_label, pr_url) = derive_pr_action(
                            &action_map,
                            &rr.repo_name,
                            &p.policy_name,
                            &rr.outcome,
                        );
                        ApplyRow {
                            repo: rr.repo_name.clone(),
                            status: self.format_status(&rr.outcome),
                            action: action_label,
                            pr: pr_url,
                        }
                    })
                    .collect()
            }));
        }

        if has_apply_settings_actions(apply_report) {
            out.push('\n');
            writeln!(out, "Repo settings updates:").unwrap();
            writeln!(
                out,
                "{}",
                render_settings_table(
                    &apply_report.settings_applied,
                    &apply_report.settings_errored,
                )
            )
            .unwrap();
        }

        if has_apply_branch_protection_actions(apply_report) {
            out.push('\n');
            writeln!(out, "Branch protection updates:").unwrap();
            writeln!(
                out,
                "{}",
                render_protection_table(
                    &apply_report.branch_protection_applied,
                    &apply_report.branch_protection_errored,
                )
            )
            .unwrap();
        }

        if !report.results.is_empty() {
            out.push('\n');
        }
        out.push_str(&format_overview(report));
        write!(
            out,
            "\nPRs:               {}\nSettings:          {}\nBranch protection: {}",
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

fn format_branch_protection_patch(patch: &multipush_core::model::BranchProtectionPatch) -> String {
    serde_json::to_string(patch).unwrap_or_else(|_| "<unserializable>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::engine::executor::{PrAction, PrActionKind};
    use multipush_core::formatter::{RepoResult, Summary};
    use multipush_core::model::{PrState, PullRequest, RepoSettingsPatch, Severity};
    use multipush_core::rule::{AttributedRemediation, Remediation};

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
        // Check now exposes an Action column derived from the outcome.
        assert!(output.contains("Action"));
        assert!(output.contains("Report only"));
        assert!(output.contains("Policy summary: 1 pass, 1 fail, 1 skip, 1 error (50.0% pass)"));
        assert!(output.contains("Overview"));
        assert!(output.contains("Pass:         1"));
        assert!(output.contains("Fail:         1"));
        assert!(output.contains("Skip:         1"));
        assert!(output.contains("Error:        1"));
        assert!(output.contains("Success rate: 50.0%  (1 pass / 2 evaluated)"));
        // Summary footer parity with apply.
        assert!(output.contains("PRs:               0 actions"));
        assert!(output.contains("Settings:          0 actions"));
        assert!(output.contains("Branch protection: 0 actions"));
    }

    /// Regression: with color enabled, ANSI escapes in the Status cell used to
    /// inflate the column-width calculation and break alignment. Verify that
    /// every `│` in a body row sits at the same *visible* column as the `┼` in
    /// the divider above it.
    #[test]
    fn colored_status_does_not_break_alignment() {
        use multipush_core::formatter::Summary;

        let report = make_report(
            vec![PolicyReport {
                policy_name: "p".to_string(),
                description: None,
                severity: Severity::Error,
                repo_results: vec![
                    RepoResult {
                        repo_name: "org/a".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Pass {
                            detail: "ok".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/b".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "nope".to_string(),
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

        let formatter = TableFormatter::with_color(true);
        let output = formatter.format(&report).unwrap();

        fn strip_ansi(s: &str) -> String {
            let mut out = String::with_capacity(s.len());
            let mut chars = s.chars();
            while let Some(c) = chars.next() {
                if c == '\x1b' {
                    // Skip CSI sequence: `[` ... terminator in `@-~`.
                    if chars.next() == Some('[') {
                        for c2 in chars.by_ref() {
                            if ('@'..='~').contains(&c2) {
                                break;
                            }
                        }
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }
        let visible: Vec<String> = output.lines().map(strip_ansi).collect();

        let divider = visible
            .iter()
            .find(|l| l.contains('┼'))
            .expect("expected a middle divider row");
        // Visible-character positions of inner column junctions (`┼`).
        let junction_cols: Vec<usize> = divider
            .chars()
            .enumerate()
            .filter_map(|(i, c)| (c == '┼').then_some(i))
            .collect();
        assert!(!junction_cols.is_empty(), "no inner junctions on divider");

        // Each body row's *inner* `│` must sit at the same visible columns.
        for row in visible
            .iter()
            .filter(|l| l.starts_with('│') && (l.contains("org/a") || l.contains("org/b")))
        {
            let mut pipe_cols: Vec<usize> = row
                .chars()
                .enumerate()
                .filter_map(|(i, c)| (c == '│').then_some(i))
                .collect();
            // Drop the leading and trailing border `│`; keep inner separators.
            pipe_cols.pop();
            pipe_cols.remove(0);
            assert_eq!(
                pipe_cols, junction_cols,
                "row dividers misaligned with header divider\n  divider: {divider:?}\n  row    : {row:?}",
            );
        }
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
        assert!(output.contains("Policies:     2"));
        assert!(output.contains("Success rate: 50.0%  (1 pass / 2 evaluated)"));
    }

    #[test]
    fn by_repo_groups_results_and_lists_each_policy() {
        let report = make_report(
            vec![
                PolicyReport {
                    policy_name: "codeowners-everywhere".to_string(),
                    description: None,
                    severity: Severity::Error,
                    repo_results: vec![
                        RepoResult {
                            repo_name: "acme/api".to_string(),
                            default_branch: "main".to_string(),
                            outcome: RepoOutcome::Pass {
                                detail: "File .github/CODEOWNERS exists".to_string(),
                            },
                        },
                        RepoResult {
                            repo_name: "acme/web".to_string(),
                            default_branch: "main".to_string(),
                            outcome: RepoOutcome::Fail {
                                detail: "missing".to_string(),
                                remediations: vec![],
                            },
                        },
                    ],
                },
                PolicyReport {
                    policy_name: "codeowners-portal".to_string(),
                    description: None,
                    severity: Severity::Warning,
                    repo_results: vec![RepoResult {
                        repo_name: "acme/api".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Skip {
                            reason: "not in scope".to_string(),
                        },
                    }],
                },
            ],
            Summary {
                total_repos: 2,
                passing: 1,
                failing: 1,
                skipped: 1,
                errors: 0,
            },
        );

        let formatter = TableFormatter::with_color(false);
        let output = formatter.format_by_repo(&report).unwrap();

        // Repos appear sorted, each followed by a per-policy line.
        let api_header = output.find("acme/api  — 2 policies").expect("api header");
        let web_header = output.find("acme/web  — 1 policy").expect("web header");
        assert!(
            api_header < web_header,
            "acme/api should come before acme/web"
        );

        // Counts are summarised non-zero kinds only.
        assert!(output.contains("acme/api  — 2 policies (1 pass, 1 skip)"));
        assert!(output.contains("acme/web  — 1 policy (1 fail)"));

        // Per-policy lines under each repo.
        assert!(output.contains("✓ codeowners-everywhere"));
        assert!(output.contains("• codeowners-portal"));
        assert!(output.contains("✗ codeowners-everywhere"));

        // Overview block still rendered.
        assert!(output.contains("Overview"));
        assert!(output.contains("Success rate: 50.0%  (1 pass / 2 evaluated)"));
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

        // No policies → just the overview block, no leading blank line.
        assert!(output.starts_with("Overview"));
        assert!(output.contains("Policies:     0"));
        assert!(output.contains("Repositories: 0"));
        assert!(output.contains("Success rate: n/a  (no repositories evaluated)"));
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
        assert!(output.contains("PRs:               1 created"));
        assert!(output.contains("Success rate: 50.0%  (1 pass / 2 evaluated)"));
    }

    #[test]
    fn check_surfaces_planned_settings_and_pr_action() {
        let remediation = AttributedRemediation::new(
            "repo_settings",
            Remediation::RepoSettings {
                description: "delete branches on merge".to_string(),
                patch: RepoSettingsPatch {
                    delete_branch_on_merge: Some(true),
                    ..Default::default()
                },
            },
        );
        let report = make_report(
            vec![PolicyReport {
                policy_name: "delete-head-branches".to_string(),
                description: None,
                severity: Severity::Warning,
                repo_results: vec![RepoResult {
                    repo_name: "org/a".to_string(),
                    default_branch: "main".to_string(),
                    outcome: RepoOutcome::Fail {
                        detail: "missing".to_string(),
                        remediations: vec![remediation],
                    },
                }],
            }],
            Summary {
                total_repos: 1,
                passing: 0,
                failing: 1,
                skipped: 0,
                errors: 0,
            },
        );

        let formatter = TableFormatter::with_color(false);
        let output = formatter.format(&report).unwrap();

        // Per-policy table shows the planned action.
        assert!(output.contains("Would update settings"));
        // Dedicated sub-table for settings updates.
        assert!(output.contains("Repo settings updates:"));
        assert!(output.contains("would update settings"));
        assert!(output.contains("\"delete_branch_on_merge\":true"));
        // Footer summary mirrors apply.
        assert!(output.contains("Settings:          1 would apply"));
    }
}
