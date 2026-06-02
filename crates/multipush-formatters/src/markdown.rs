use std::fmt::Write;

use multipush_core::engine::executor::{ApplyReport, SettingsActionKind};
use multipush_core::formatter::{
    build_pr_action_map, format_branch_protection_summary, format_pr_summary,
    format_settings_summary, group_by_repo, has_branch_protection_actions, has_settings_actions,
    Formatter, PolicyCounts, PolicyReport, RepoCounts, RepoOutcome, Report,
};

pub struct MarkdownFormatter;

impl MarkdownFormatter {
    pub fn new() -> Self {
        Self
    }

    fn status_label(outcome: &RepoOutcome) -> &'static str {
        match outcome {
            RepoOutcome::Pass { .. } => "PASS",
            RepoOutcome::Fail { .. } => "FAIL",
            RepoOutcome::Skip { .. } => "SKIP",
            RepoOutcome::Error { .. } => "ERROR",
        }
    }

    fn detail(outcome: &RepoOutcome) -> &str {
        match outcome {
            RepoOutcome::Pass { detail } => detail,
            RepoOutcome::Fail { detail, .. } => detail,
            RepoOutcome::Skip { reason } => reason,
            RepoOutcome::Error { message } => message,
        }
    }

    fn format_policy_check(&self, policy: &PolicyReport) -> String {
        let mut out = String::new();

        writeln!(out, "## Policy: {}", policy.policy_name).unwrap();
        if let Some(desc) = &policy.description {
            writeln!(out, "> {desc}").unwrap();
        }
        writeln!(out).unwrap();

        if policy.repo_results.is_empty() {
            writeln!(out, "*(no repositories matched)*").unwrap();
            return out;
        }

        writeln!(out, "| Repository | Status | Detail |").unwrap();
        writeln!(out, "|---|---|---|").unwrap();

        for rr in &policy.repo_results {
            writeln!(
                out,
                "| {} | {} | {} |",
                rr.repo_name,
                Self::status_label(&rr.outcome),
                Self::detail(&rr.outcome),
            )
            .unwrap();
        }

        writeln!(out).unwrap();
        writeln!(
            out,
            "**Policy summary:** {}",
            format_counts_line(&PolicyCounts::from_policy(policy)),
        )
        .unwrap();

        out
    }
}

/// Same shape as the table formatter's helper; only non-zero kinds are listed.
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
    let mut out = String::from("## Overview\n\n");
    writeln!(out, "- Policies: {policies}").unwrap();
    writeln!(out, "- Repositories: {}", s.total_repos).unwrap();
    writeln!(out, "- Pass: {}", s.passing).unwrap();
    writeln!(out, "- Fail: {}", s.failing).unwrap();
    writeln!(out, "- Skip: {}", s.skipped).unwrap();
    writeln!(out, "- Error: {}", s.errors).unwrap();
    match s.success_rate() {
        Some(rate) => writeln!(
            out,
            "- **Success rate:** {:.1}% ({} pass / {} evaluated)",
            rate,
            s.passing,
            s.evaluated(),
        )
        .unwrap(),
        None => writeln!(out, "- **Success rate:** n/a (no repositories evaluated)").unwrap(),
    }
    out
}

impl Default for MarkdownFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for MarkdownFormatter {
    fn name(&self) -> &str {
        "markdown"
    }

    fn format(&self, report: &Report) -> multipush_core::Result<String> {
        let mut out = String::from("# multipush Report\n\n");

        for policy in &report.results {
            out.push_str(&self.format_policy_check(policy));
            out.push('\n');
        }

        out.push_str(&format_overview(report));

        Ok(out)
    }

    fn format_by_repo(&self, report: &Report) -> multipush_core::Result<String> {
        let mut out = String::from("# multipush Report (by repo)\n\n");
        let grouped = group_by_repo(report);

        for (repo, results) in &grouped {
            let counts = RepoCounts::from_results(results);
            writeln!(
                out,
                "### {repo} — {} {} ({})",
                counts.total(),
                if counts.total() == 1 {
                    "policy"
                } else {
                    "policies"
                },
                format_repo_counts(&counts),
            )
            .unwrap();
            writeln!(out).unwrap();

            for (policy_name, rr) in results {
                writeln!(
                    out,
                    "- {} **{policy_name}** — {}",
                    Self::status_label(&rr.outcome),
                    Self::detail(&rr.outcome),
                )
                .unwrap();
            }
            writeln!(out).unwrap();
        }

        out.push_str(&format_overview(report));
        Ok(out)
    }

    fn format_apply(&self, apply_report: &ApplyReport) -> multipush_core::Result<String> {
        let report = &apply_report.report;
        let action_map = build_pr_action_map(apply_report);

        let mut out = String::from("# multipush Apply Report\n\n");

        for policy in &report.results {
            writeln!(out, "## Policy: {}", policy.policy_name).unwrap();
            if let Some(desc) = &policy.description {
                writeln!(out, "> {desc}").unwrap();
            }
            writeln!(out).unwrap();

            if policy.repo_results.is_empty() {
                writeln!(out, "*(no repositories matched)*").unwrap();
                out.push('\n');
                continue;
            }

            writeln!(out, "| Repository | Status | Action | PR |").unwrap();
            writeln!(out, "|---|---|---|---|").unwrap();

            for rr in &policy.repo_results {
                let key = (rr.repo_name.clone(), policy.policy_name.clone());
                let (action_label, pr_url) = action_map
                    .get(&key)
                    .map(|(a, u)| (a.as_str(), u.as_str()))
                    .unwrap_or(("-", "-"));

                writeln!(
                    out,
                    "| {} | {} | {} | {} |",
                    rr.repo_name,
                    Self::status_label(&rr.outcome),
                    action_label,
                    pr_url,
                )
                .unwrap();
            }

            writeln!(out).unwrap();
            writeln!(
                out,
                "**Policy summary:** {}",
                format_counts_line(&PolicyCounts::from_policy(policy)),
            )
            .unwrap();

            out.push('\n');
        }

        if has_settings_actions(apply_report) {
            writeln!(out, "## Repo settings updates\n").unwrap();
            writeln!(out, "| Repository | Policies | Action | Patch |").unwrap();
            writeln!(out, "|---|---|---|---|").unwrap();
            for a in &apply_report.settings_applied {
                let label = match a.action {
                    SettingsActionKind::Applied => "settings updated",
                    SettingsActionKind::DryRun => "would update settings",
                    SettingsActionKind::Error => "error",
                };
                writeln!(
                    out,
                    "| {} | {} | {} | `{}` |",
                    a.repo_name,
                    a.policy_names.join(", "),
                    label,
                    serde_json::to_string(&a.patch).unwrap_or_default(),
                )
                .unwrap();
            }
            for a in &apply_report.settings_errored {
                writeln!(
                    out,
                    "| {} | {} | error: {} | `{}` |",
                    a.repo_name,
                    a.policy_names.join(", "),
                    a.error.as_deref().unwrap_or("unknown"),
                    serde_json::to_string(&a.patch).unwrap_or_default(),
                )
                .unwrap();
            }
            out.push('\n');
        }

        if has_branch_protection_actions(apply_report) {
            writeln!(out, "## Branch protection updates\n").unwrap();
            writeln!(out, "| Repository | Branch | Policies | Action | Patch |").unwrap();
            writeln!(out, "|---|---|---|---|---|").unwrap();
            for a in &apply_report.branch_protection_applied {
                let label = match a.action {
                    SettingsActionKind::Applied => "protection updated",
                    SettingsActionKind::DryRun => "would update protection",
                    SettingsActionKind::Error => "error",
                };
                writeln!(
                    out,
                    "| {} | {} | {} | {} | `{}` |",
                    a.repo_name,
                    a.branch,
                    a.policy_names.join(", "),
                    label,
                    serde_json::to_string(&a.patch).unwrap_or_default(),
                )
                .unwrap();
            }
            for a in &apply_report.branch_protection_errored {
                writeln!(
                    out,
                    "| {} | {} | {} | error: {} | `{}` |",
                    a.repo_name,
                    a.branch,
                    a.policy_names.join(", "),
                    a.error.as_deref().unwrap_or("unknown"),
                    serde_json::to_string(&a.patch).unwrap_or_default(),
                )
                .unwrap();
            }
            out.push('\n');
        }

        out.push_str(&format_overview(&apply_report.report));
        writeln!(out, "- PRs: {}", format_pr_summary(apply_report)).unwrap();
        writeln!(out, "- Settings: {}", format_settings_summary(apply_report)).unwrap();
        write!(
            out,
            "- Branch protection: {}",
            format_branch_protection_summary(apply_report),
        )
        .unwrap();

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::engine::executor::{PrAction, PrActionKind};
    use multipush_core::formatter::{RepoResult, Summary};
    use multipush_core::model::{PrState, PullRequest, Severity};

    fn make_check_report() -> Report {
        Report {
            results: vec![PolicyReport {
                policy_name: "codeowners-required".to_string(),
                description: Some("All repos must have a CODEOWNERS file".to_string()),
                severity: Severity::Error,
                repo_results: vec![
                    RepoResult {
                        repo_name: "acme/api-gateway".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Pass {
                            detail: "File CODEOWNERS exists".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "acme/web-frontend".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "File CODEOWNERS does not exist".to_string(),
                            remediations: vec![],
                        },
                    },
                ],
            }],
            summary: Summary {
                total_repos: 2,
                passing: 1,
                failing: 1,
                skipped: 0,
                errors: 0,
            },
        }
    }

    #[test]
    fn markdown_format_by_repo() {
        let report = make_check_report();
        let formatter = MarkdownFormatter::new();
        let output = formatter.format_by_repo(&report).unwrap();

        assert!(output.starts_with("# multipush Report (by repo)"));
        assert!(output.contains("### acme/api-gateway — 1 policy (1 pass)"));
        assert!(output.contains("### acme/web-frontend — 1 policy (1 fail)"));
        assert!(output.contains("- PASS **codeowners-required** — File CODEOWNERS exists"));
        assert!(output.contains("- FAIL **codeowners-required** — File CODEOWNERS does not exist"));
        assert!(output.contains("## Overview"));
    }

    #[test]
    fn markdown_format_check_mode() {
        let report = make_check_report();
        let formatter = MarkdownFormatter::new();
        let output = formatter.format(&report).unwrap();

        assert!(output.starts_with("# multipush Report"));
        assert!(output.contains("## Policy: codeowners-required"));
        assert!(output.contains("> All repos must have a CODEOWNERS file"));
        assert!(output.contains("| acme/api-gateway | PASS | File CODEOWNERS exists |"));
        assert!(output.contains("| acme/web-frontend | FAIL | File CODEOWNERS does not exist |"));
        assert!(
            output.contains("**Policy summary:** 1 pass, 1 fail, 0 skip, 0 errors (50.0% pass)")
        );
        assert!(output.contains("## Overview"));
        assert!(output.contains("- Policies: 1"));
        assert!(output.contains("- Repositories: 2"));
        assert!(output.contains("- **Success rate:** 50.0% (1 pass / 2 evaluated)"));
    }

    #[test]
    fn markdown_format_apply_mode() {
        let report = make_check_report();
        let apply_report = ApplyReport {
            report,
            prs_created: vec![PrAction {
                repo_name: "acme/web-frontend".to_string(),
                policy_name: "codeowners-required".to_string(),
                branch: "multipush/codeowners-required".to_string(),
                pr: Some(PullRequest {
                    number: 42,
                    title: "Add CODEOWNERS".to_string(),
                    head_branch: "multipush/codeowners-required".to_string(),
                    url: "https://github.com/acme/web-frontend/pull/42".to_string(),
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

        let formatter = MarkdownFormatter::new();
        let output = formatter.format_apply(&apply_report).unwrap();

        assert!(output.starts_with("# multipush Apply Report"));
        assert!(output.contains("| Repository | Status | Action | PR |"));
        assert!(output.contains("| acme/api-gateway | PASS | - | - |"));
        assert!(output.contains("| acme/web-frontend | FAIL | PR created | https://github.com/acme/web-frontend/pull/42 |"));
        assert!(output.contains("PRs: 1 created"));
    }
}
