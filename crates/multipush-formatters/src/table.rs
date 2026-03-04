use std::fmt::Write;

use tabled::settings::Style;
use tabled::{Table, Tabled};

use multipush_core::formatter::{Formatter, PolicyReport, Report, RepoOutcome};

#[derive(Tabled)]
struct Row {
    #[tabled(rename = "Repository")]
    repo: String,
    #[tabled(rename = "Status")]
    status: String,
    #[tabled(rename = "Detail")]
    detail: String,
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
            RepoOutcome::Fail { detail } => detail,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::formatter::{RepoResult, Summary};
    use multipush_core::model::Severity;

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
                        outcome: RepoOutcome::Pass {
                            detail: "File LICENSE exists".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/beta".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "File LICENSE does not exist".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/gamma".to_string(),
                        outcome: RepoOutcome::Skip {
                            reason: "Repo is archived".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/delta".to_string(),
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
                        outcome: RepoOutcome::Fail {
                            detail: "not ok".to_string(),
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
}
