//! Provenance metadata for the policy run, surfaced in PR bodies.
//!
//! When multipush opens a PR in `waratek/portal`, the reviewer has no way to
//! know which repo the YAML lives in or which workflow run produced the PR —
//! unless we tell them. This module collects that context, primarily from
//! GitHub Actions environment variables, and exposes a renderer that the
//! executor inlines into PR descriptions.
//!
//! Local runs leave the info empty and the executor skips the block entirely,
//! so unit tests don't need to scrub env vars.

use std::env;

/// Where the policy came from and what produced this run.
///
/// All fields are `Option` because each piece of context is independently
/// available — a manual `multipush apply` from a developer machine knows
/// nothing of `GITHUB_REPOSITORY`, but a `--policy-source-url` flag still
/// produces a useful "Policy defined in …" link.
#[derive(Debug, Clone, Default)]
pub struct PolicySourceInfo {
    /// Browser URL of the policy repo, e.g. `https://github.com/waratek/cml`.
    pub repo_url: Option<String>,
    /// Commit SHA pinned to this run, for a `/tree/<sha>` deep link.
    pub commit_sha: Option<String>,
    /// Branch/ref name (fallback when no SHA is available).
    pub ref_name: Option<String>,
    /// URL of the workflow run, e.g.
    /// `https://github.com/waratek/cml/actions/runs/123`.
    pub workflow_run_url: Option<String>,
    /// Human-readable workflow name (e.g. `policy-enforcement`).
    pub workflow_name: Option<String>,
}

impl PolicySourceInfo {
    /// True when at least one piece of context is set — used by the executor
    /// to decide whether to emit a `### Source` block at all.
    pub fn is_present(&self) -> bool {
        self.repo_url.is_some()
            || self.commit_sha.is_some()
            || self.ref_name.is_some()
            || self.workflow_run_url.is_some()
            || self.workflow_name.is_some()
    }

    /// Populate from GitHub Actions env vars when `GITHUB_ACTIONS=true`.
    /// Returns an all-`None` instance otherwise — callers can layer a CLI
    /// override on top with [`Self::with_repo_url`].
    pub fn from_github_actions_env() -> Self {
        // GITHUB_ACTIONS is the canonical "we are in Actions" sentinel; honor
        // it strictly to avoid false positives from stray env vars.
        if env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            return Self::default();
        }

        let server = env::var("GITHUB_SERVER_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://github.com".to_string());
        let repository = env::var("GITHUB_REPOSITORY").ok().filter(|s| !s.is_empty());
        let sha = env::var("GITHUB_SHA").ok().filter(|s| !s.is_empty());
        let ref_name = env::var("GITHUB_REF_NAME").ok().filter(|s| !s.is_empty());
        let run_id = env::var("GITHUB_RUN_ID").ok().filter(|s| !s.is_empty());
        let workflow = env::var("GITHUB_WORKFLOW").ok().filter(|s| !s.is_empty());

        let repo_url = repository.as_ref().map(|r| format!("{server}/{r}"));
        let workflow_run_url = repository
            .as_ref()
            .zip(run_id.as_ref())
            .map(|(repo, id)| format!("{server}/{repo}/actions/runs/{id}"));

        Self {
            repo_url,
            commit_sha: sha,
            ref_name,
            workflow_run_url,
            workflow_name: workflow,
        }
    }

    /// Override the repo URL — used by the CLI's `--policy-source-url` flag
    /// for runs outside GitHub Actions.
    pub fn with_repo_url(mut self, url: impl Into<String>) -> Self {
        self.repo_url = Some(url.into());
        self
    }

    /// Render as Markdown for inclusion in a PR body. Returns `None` when no
    /// context is set, so the caller can skip the section header.
    pub fn render_markdown(&self) -> Option<String> {
        if !self.is_present() {
            return None;
        }
        let mut out = String::new();

        if let Some(repo_url) = &self.repo_url {
            // Prefer linking to the exact tree at the run's SHA so a future
            // reviewer can read the policy as it was when this PR was opened
            // — `main` could have moved.
            let (target, label) = match self.commit_sha.as_deref() {
                Some(sha) if !sha.is_empty() => {
                    let short = sha.get(..7).unwrap_or(sha);
                    (
                        format!("{repo_url}/tree/{sha}"),
                        format!("{} @ {short}", strip_url_prefix(repo_url)),
                    )
                }
                _ => (repo_url.clone(), strip_url_prefix(repo_url).to_string()),
            };
            out.push_str(&format!("Policy defined in [{label}]({target})\n"));
        }

        if let Some(run_url) = &self.workflow_run_url {
            let label = self
                .workflow_name
                .as_deref()
                .map(|w| format!("workflow run ({w})"))
                .unwrap_or_else(|| "workflow run".to_string());
            out.push_str(&format!("Opened by [{label}]({run_url})\n"));
        }

        Some(out)
    }
}

/// Strip `https://github.com/` (or any scheme) to produce a compact
/// `<org>/<repo>` label for Markdown links. Falls back to the original URL.
fn strip_url_prefix(url: &str) -> &str {
    url.split_once("://")
        .map(|(_, rest)| rest.split_once('/').map(|(_, path)| path).unwrap_or(rest))
        .unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_absent() {
        let info = PolicySourceInfo::default();
        assert!(!info.is_present());
        assert!(info.render_markdown().is_none());
    }

    #[test]
    fn repo_url_only_renders_tree_link_without_sha() {
        let info = PolicySourceInfo::default().with_repo_url("https://github.com/waratek/cml");
        let md = info.render_markdown().unwrap();
        assert!(md.contains("[waratek/cml](https://github.com/waratek/cml)"));
        assert!(!md.contains("/tree/"));
    }

    #[test]
    fn sha_pins_tree_link_to_commit() {
        let mut info = PolicySourceInfo::default().with_repo_url("https://github.com/waratek/cml");
        info.commit_sha = Some("992aaab123456789abcdef".to_string());
        let md = info.render_markdown().unwrap();
        assert!(md.contains("/tree/992aaab123456789abcdef"));
        // Label uses the short sha for readability.
        assert!(md.contains("waratek/cml @ 992aaab"));
    }

    #[test]
    fn workflow_run_emits_dedicated_line() {
        let info = PolicySourceInfo {
            workflow_run_url: Some(
                "https://github.com/waratek/cml/actions/runs/8123456789".to_string(),
            ),
            workflow_name: Some("policy-enforcement".to_string()),
            ..Default::default()
        };
        let md = info.render_markdown().unwrap();
        assert!(md.contains("workflow run (policy-enforcement)"));
        assert!(md.contains("actions/runs/8123456789"));
    }
}
