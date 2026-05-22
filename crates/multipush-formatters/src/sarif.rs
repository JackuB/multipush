use serde::Serialize;

use multipush_core::engine::executor::ApplyReport;
use multipush_core::formatter::{Formatter, RepoOutcome, Report};
use multipush_core::model::Severity;

#[derive(Serialize)]
struct SarifLog {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<SarifRun>,
}

#[derive(Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Serialize)]
struct SarifDriver {
    name: &'static str,
    version: String,
    rules: Vec<SarifRuleDescriptor>,
}

#[derive(Serialize)]
struct SarifRuleDescriptor {
    id: String,
    #[serde(rename = "shortDescription")]
    short_description: SarifMessage,
}

#[derive(Serialize)]
struct SarifMessage {
    text: String,
}

#[derive(Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: &'static str,
    message: SarifMessage,
    locations: Vec<SarifLocation>,
}

#[derive(Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(Serialize)]
struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
}

#[derive(Serialize)]
struct SarifArtifactLocation {
    uri: String,
}

fn severity_to_sarif(severity: &Severity) -> &'static str {
    match severity {
        Severity::Info => "note",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

pub struct SarifFormatter;

impl SarifFormatter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SarifFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for SarifFormatter {
    fn name(&self) -> &str {
        "sarif"
    }

    fn format(&self, report: &Report) -> multipush_core::Result<String> {
        let mut rules = Vec::new();
        let mut results = Vec::new();

        for policy in &report.results {
            rules.push(SarifRuleDescriptor {
                id: policy.policy_name.clone(),
                short_description: SarifMessage {
                    text: policy
                        .description
                        .clone()
                        .unwrap_or_else(|| policy.policy_name.clone()),
                },
            });

            let level = severity_to_sarif(&policy.severity);

            for rr in &policy.repo_results {
                let message_text = match &rr.outcome {
                    RepoOutcome::Fail { detail, .. } => detail.clone(),
                    RepoOutcome::Error { message } => message.clone(),
                    RepoOutcome::Pass { .. } | RepoOutcome::Skip { .. } => continue,
                };

                results.push(SarifResult {
                    rule_id: policy.policy_name.clone(),
                    level,
                    message: SarifMessage {
                        text: message_text,
                    },
                    locations: vec![SarifLocation {
                        physical_location: SarifPhysicalLocation {
                            artifact_location: SarifArtifactLocation {
                                uri: rr.repo_name.clone(),
                            },
                        },
                    }],
                });
            }
        }

        let log = SarifLog {
            schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
            version: "2.1.0",
            runs: vec![SarifRun {
                tool: SarifTool {
                    driver: SarifDriver {
                        name: "multipush",
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        rules,
                    },
                },
                results,
            }],
        };

        let json = serde_json::to_string_pretty(&log)?;
        Ok(json)
    }

    fn format_apply(&self, apply_report: &ApplyReport) -> multipush_core::Result<String> {
        self.format(&apply_report.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::formatter::{PolicyReport, RepoResult, Summary};

    fn make_report(policies: Vec<PolicyReport>, summary: Summary) -> Report {
        Report {
            results: policies,
            summary,
        }
    }

    #[test]
    fn sarif_check_mode_produces_valid_json() {
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

        let formatter = SarifFormatter::new();
        let output = formatter.format(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["version"], "2.1.0");
        assert!(parsed["$schema"].as_str().unwrap().contains("sarif"));
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["name"], "multipush");

        let rules = parsed["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "require-license");

        let results = parsed["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["ruleId"], "require-license");
        assert_eq!(results[0]["level"], "error");
        assert!(results[0]["message"]["text"]
            .as_str()
            .unwrap()
            .contains("does not exist"));
        assert_eq!(
            results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "org/beta"
        );
    }

    #[test]
    fn sarif_empty_report() {
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

        let formatter = SarifFormatter::new();
        let output = formatter.format(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["version"], "2.1.0");
        assert!(parsed["runs"][0]["results"].as_array().unwrap().is_empty());
        assert!(parsed["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn sarif_severity_mapping() {
        let report = make_report(
            vec![
                PolicyReport {
                    policy_name: "info-policy".to_string(),
                    description: None,
                    severity: Severity::Info,
                    repo_results: vec![RepoResult {
                        repo_name: "org/a".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "info fail".to_string(),
                            remediations: vec![],
                        },
                    }],
                },
                PolicyReport {
                    policy_name: "warn-policy".to_string(),
                    description: None,
                    severity: Severity::Warning,
                    repo_results: vec![RepoResult {
                        repo_name: "org/b".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "warn fail".to_string(),
                            remediations: vec![],
                        },
                    }],
                },
                PolicyReport {
                    policy_name: "error-policy".to_string(),
                    description: None,
                    severity: Severity::Error,
                    repo_results: vec![RepoResult {
                        repo_name: "org/c".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Error {
                            message: "API error".to_string(),
                        },
                    }],
                },
            ],
            Summary {
                total_repos: 3,
                passing: 0,
                failing: 2,
                skipped: 0,
                errors: 1,
            },
        );

        let formatter = SarifFormatter::new();
        let output = formatter.format(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        let results = parsed["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["level"], "note");
        assert_eq!(results[1]["level"], "warning");
        assert_eq!(results[2]["level"], "error");
    }

    #[test]
    fn sarif_only_includes_failures() {
        let report = make_report(
            vec![PolicyReport {
                policy_name: "test".to_string(),
                description: None,
                severity: Severity::Error,
                repo_results: vec![
                    RepoResult {
                        repo_name: "org/pass".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Pass {
                            detail: "ok".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/skip".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Skip {
                            reason: "archived".to_string(),
                        },
                    },
                    RepoResult {
                        repo_name: "org/fail".to_string(),
                        default_branch: "main".to_string(),
                        outcome: RepoOutcome::Fail {
                            detail: "missing".to_string(),
                            remediations: vec![],
                        },
                    },
                ],
            }],
            Summary {
                total_repos: 3,
                passing: 1,
                failing: 1,
                skipped: 1,
                errors: 0,
            },
        );

        let formatter = SarifFormatter::new();
        let output = formatter.format(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        let results = parsed["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"], "org/fail");
    }
}
