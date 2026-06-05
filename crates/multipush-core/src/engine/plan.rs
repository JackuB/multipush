//! Cross-policy planning of API-only remediations.
//!
//! Repo-settings and branch-protection remediations can come from many
//! policies that target the same repo (e.g. "auto-delete branches" and
//! "disable wikis" both want to PATCH `/repos/{owner}/{repo}`). We aggregate
//! them up-front so:
//!
//! - the executor issues one PATCH per repo (or repo+branch) regardless of
//!   how many policies contributed to it, and
//! - the check formatter can show the *same* planned actions without re-doing
//!   the merge logic.
//!
//! Both consumers receive `Vec<SettingsAction>` / `Vec<BranchProtectionAction>`
//! with `action: SettingsActionKind::DryRun` — the executor mutates each entry
//! into `Applied` or moves it to its `_errored` list after the actual API
//! call.

use std::collections::HashMap;

use crate::formatter::{RepoOutcome, Report};
use crate::model::{BranchProtectionPatch, Repo, RepoSettingsPatch, Visibility};
use crate::rule::Remediation;

use super::executor::{BranchProtectionAction, SettingsAction, SettingsActionKind};

/// Plan all cross-policy API actions implied by a check [`Report`].
///
/// Returns `(settings, branch_protection)` lists of would-apply actions, one
/// entry per (repo) and (repo, branch) target respectively. Entries with an
/// empty patch after merging are dropped — they represent rules that emitted
/// a `RepoSettings`/`BranchProtection` remediation only for provenance.
pub fn plan_apply_actions(report: &Report) -> (Vec<SettingsAction>, Vec<BranchProtectionAction>) {
    let mut settings_by_repo: HashMap<String, (Repo, RepoSettingsPatch, Vec<String>)> =
        HashMap::new();
    let mut protection_by_target: HashMap<
        (String, String),
        (Repo, BranchProtectionPatch, Vec<String>),
    > = HashMap::new();

    for policy_report in &report.results {
        let policy_name = &policy_report.policy_name;

        for repo_result in &policy_report.repo_results {
            let remediations = match &repo_result.outcome {
                RepoOutcome::Fail { remediations, .. } if !remediations.is_empty() => remediations,
                _ => continue,
            };

            let repo = build_repo(&repo_result.repo_name, &repo_result.default_branch);

            for rem in remediations {
                match &rem.remediation {
                    Remediation::RepoSettings { patch, .. } => {
                        let entry = settings_by_repo
                            .entry(repo.full_name.clone())
                            .or_insert_with(|| {
                                (repo.clone(), RepoSettingsPatch::default(), Vec::new())
                            });
                        entry.1.merge(patch.clone());
                        if !entry.2.contains(policy_name) {
                            entry.2.push(policy_name.clone());
                        }
                    }
                    Remediation::BranchProtection { branch, patch, .. } => {
                        let entry = protection_by_target
                            .entry((repo.full_name.clone(), branch.clone()))
                            .or_insert_with(|| {
                                (repo.clone(), BranchProtectionPatch::default(), Vec::new())
                            });
                        entry.1.merge(patch.clone());
                        if !entry.2.contains(policy_name) {
                            entry.2.push(policy_name.clone());
                        }
                    }
                    Remediation::FileChanges { .. } => {}
                }
            }
        }
    }

    let settings: Vec<SettingsAction> = settings_by_repo
        .into_iter()
        .filter(|(_, (_, patch, _))| !patch.is_empty())
        .map(|(_, (repo, patch, policy_names))| SettingsAction {
            repo_name: repo.full_name,
            policy_names,
            patch,
            action: SettingsActionKind::DryRun,
            error: None,
        })
        .collect();

    let protection: Vec<BranchProtectionAction> = protection_by_target
        .into_iter()
        .filter(|(_, (_, patch, _))| !patch.is_empty())
        .map(
            |((_, branch), (repo, patch, policy_names))| BranchProtectionAction {
                repo_name: repo.full_name,
                branch,
                policy_names,
                patch,
                action: SettingsActionKind::DryRun,
                error: None,
            },
        )
        .collect();

    (settings, protection)
}

/// Build a minimal Repo for the aggregation key. Visibility/topics/etc. are
/// not used by either consumer (executor only needs full_name+default_branch
/// for the PATCH call; formatters only need full_name).
pub(crate) fn build_repo(full_name: &str, default_branch: &str) -> Repo {
    let parts: Vec<&str> = full_name.splitn(2, '/').collect();
    Repo {
        owner: parts[0].to_string(),
        name: parts.get(1).unwrap_or(&"").to_string(),
        full_name: full_name.to_string(),
        default_branch: default_branch.to_string(),
        archived: false,
        visibility: Visibility::Private,
        topics: vec![],
        language: None,
        custom_properties: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::{PolicyReport, RepoResult, Summary};
    use crate::model::Severity;
    use crate::rule::AttributedRemediation;

    fn fail_with(remediations: Vec<AttributedRemediation>) -> RepoOutcome {
        RepoOutcome::Fail {
            detail: "fail".to_string(),
            remediations,
        }
    }

    #[test]
    fn aggregates_settings_across_policies_for_same_repo() {
        let report = Report {
            results: vec![
                PolicyReport {
                    policy_name: "p1".to_string(),
                    description: None,
                    severity: Severity::Warning,
                    repo_results: vec![RepoResult {
                        repo_name: "org/a".to_string(),
                        default_branch: "main".to_string(),
                        outcome: fail_with(vec![AttributedRemediation::new(
                            "repo_settings",
                            Remediation::RepoSettings {
                                description: "delete branches".to_string(),
                                patch: RepoSettingsPatch {
                                    delete_branch_on_merge: Some(true),
                                    ..Default::default()
                                },
                            },
                        )]),
                    }],
                },
                PolicyReport {
                    policy_name: "p2".to_string(),
                    description: None,
                    severity: Severity::Warning,
                    repo_results: vec![RepoResult {
                        repo_name: "org/a".to_string(),
                        default_branch: "main".to_string(),
                        outcome: fail_with(vec![AttributedRemediation::new(
                            "repo_settings",
                            Remediation::RepoSettings {
                                description: "disable wikis".to_string(),
                                patch: RepoSettingsPatch {
                                    has_wiki: Some(false),
                                    ..Default::default()
                                },
                            },
                        )]),
                    }],
                },
            ],
            summary: Summary::default(),
        };

        let (settings, protection) = plan_apply_actions(&report);
        assert_eq!(settings.len(), 1);
        let a = &settings[0];
        assert_eq!(a.repo_name, "org/a");
        assert_eq!(a.policy_names, vec!["p1", "p2"]);
        assert_eq!(a.patch.delete_branch_on_merge, Some(true));
        assert_eq!(a.patch.has_wiki, Some(false));
        assert!(protection.is_empty());
    }

    #[test]
    fn skips_empty_patches() {
        let report = Report {
            results: vec![PolicyReport {
                policy_name: "p".to_string(),
                description: None,
                severity: Severity::Warning,
                repo_results: vec![RepoResult {
                    repo_name: "org/a".to_string(),
                    default_branch: "main".to_string(),
                    outcome: fail_with(vec![AttributedRemediation::new(
                        "repo_settings",
                        Remediation::RepoSettings {
                            description: "noop".to_string(),
                            patch: RepoSettingsPatch::default(),
                        },
                    )]),
                }],
            }],
            summary: Summary::default(),
        };

        let (settings, _) = plan_apply_actions(&report);
        assert!(settings.is_empty());
    }

    #[test]
    fn aggregates_branch_protection_per_branch() {
        let mk = |branch: &str, enforce_admins: bool| {
            AttributedRemediation::new(
                "branch_protection",
                Remediation::BranchProtection {
                    description: "protect".to_string(),
                    branch: branch.to_string(),
                    patch: BranchProtectionPatch {
                        enforce_admins: Some(enforce_admins),
                        ..Default::default()
                    },
                },
            )
        };
        let report = Report {
            results: vec![PolicyReport {
                policy_name: "p".to_string(),
                description: None,
                severity: Severity::Warning,
                repo_results: vec![RepoResult {
                    repo_name: "org/a".to_string(),
                    default_branch: "main".to_string(),
                    outcome: fail_with(vec![mk("main", true), mk("release", false)]),
                }],
            }],
            summary: Summary::default(),
        };

        let (_, protection) = plan_apply_actions(&report);
        assert_eq!(protection.len(), 2);
        let mut branches: Vec<&str> = protection.iter().map(|p| p.branch.as_str()).collect();
        branches.sort();
        assert_eq!(branches, vec!["main", "release"]);
    }

    #[test]
    fn pass_outcomes_contribute_nothing() {
        let report = Report {
            results: vec![PolicyReport {
                policy_name: "p".to_string(),
                description: None,
                severity: Severity::Warning,
                repo_results: vec![RepoResult {
                    repo_name: "org/a".to_string(),
                    default_branch: "main".to_string(),
                    outcome: RepoOutcome::Pass {
                        detail: "ok".to_string(),
                    },
                }],
            }],
            summary: Summary::default(),
        };

        let (s, p) = plan_apply_actions(&report);
        assert!(s.is_empty());
        assert!(p.is_empty());
    }
}
