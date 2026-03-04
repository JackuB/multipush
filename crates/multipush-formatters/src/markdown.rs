use std::fmt::Write;

use multipush_core::engine::executor::ApplyReport;
use multipush_core::formatter::{
    build_pr_action_map, format_pr_summary, Formatter, PolicyReport, Report, RepoOutcome,
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

        out
    }
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

        let s = &report.summary;
        write!(
            out,
            "**Summary:** {} pass, {} fail, {} skip, {} errors",
            s.passing, s.failing, s.skipped, s.errors,
        )
        .unwrap();

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

            out.push('\n');
        }

        let s = &report.summary;
        write!(
            out,
            "**Summary:** {} pass, {} fail, {} skip, {} errors | PRs: {}",
            s.passing,
            s.failing,
            s.skipped,
            s.errors,
            format_pr_summary(apply_report),
        )
        .unwrap();

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::formatter::{RepoResult, Summary};
    use multipush_core::engine::executor::{PrAction, PrActionKind};
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
    fn markdown_format_check_mode() {
        let report = make_check_report();
        let formatter = MarkdownFormatter::new();
        let output = formatter.format(&report).unwrap();

        assert!(output.starts_with("# multipush Report"));
        assert!(output.contains("## Policy: codeowners-required"));
        assert!(output.contains("> All repos must have a CODEOWNERS file"));
        assert!(output.contains("| acme/api-gateway | PASS | File CODEOWNERS exists |"));
        assert!(output.contains("| acme/web-frontend | FAIL | File CODEOWNERS does not exist |"));
        assert!(output.contains("**Summary:** 1 pass, 1 fail, 0 skip, 0 errors"));
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
            }],
            prs_updated: vec![],
            prs_skipped: vec![],
            prs_limited: 0,
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
