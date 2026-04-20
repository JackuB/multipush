use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;

use crate::config::{ProviderConfig, ProviderType, RootConfig, PolicyConfig};
use crate::formatter::{PolicyReport, Report, RepoOutcome, RepoResult, Summary};
use crate::model::{
    FileChange, FileContent, PrState, PullRequest, Repo, RepoSettings, Severity, Visibility,
};
use crate::provider::Provider;
use crate::rule::Remediation;
use crate::Result;

/// A configurable mock provider for testing evaluator and executor logic.
pub struct MockProvider {
    pub repos: Vec<Repo>,
    pub files: Mutex<HashMap<String, FileContent>>,
    pub open_prs: Mutex<HashMap<String, PullRequest>>,
    pub create_pr_calls: AtomicUsize,
    pub update_pr_calls: AtomicUsize,
}

impl MockProvider {
    pub fn new(repos: Vec<Repo>) -> Self {
        Self {
            repos,
            files: Mutex::new(HashMap::new()),
            open_prs: Mutex::new(HashMap::new()),
            create_pr_calls: AtomicUsize::new(0),
            update_pr_calls: AtomicUsize::new(0),
        }
    }

    /// Add a file to the mock. Key format: `"owner/repo:path"`.
    pub fn with_file(self, repo_file_key: &str, content: &str) -> Self {
        self.files.lock().unwrap().insert(
            repo_file_key.to_string(),
            FileContent {
                path: repo_file_key.to_string(),
                content: content.to_string(),
                sha: "abc".to_string(),
            },
        );
        self
    }

    /// Add an open PR to the mock. Key format: `"owner/repo:branch"`.
    pub fn with_open_pr(self, repo_branch_key: &str, pr: PullRequest) -> Self {
        self.open_prs
            .lock()
            .unwrap()
            .insert(repo_branch_key.to_string(), pr);
        self
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn list_repos(&self, _org: &str) -> Result<Vec<Repo>> {
        Ok(self.repos.clone())
    }

    async fn get_file(
        &self,
        repo: &Repo,
        path: &str,
        _git_ref: &str,
    ) -> Result<Option<FileContent>> {
        let key = format!("{}:{}", repo.full_name, path);
        Ok(self.files.lock().unwrap().get(&key).cloned())
    }

    async fn get_repo_settings(&self, _repo: &Repo) -> Result<RepoSettings> {
        Ok(RepoSettings {
            has_issues: true,
            has_wiki: false,
            has_projects: false,
            allow_merge_commit: true,
            allow_squash_merge: true,
            allow_rebase_merge: false,
            delete_branch_on_merge: true,
            default_branch: "main".to_string(),
            allow_auto_merge: false,
        })
    }

    async fn find_open_pr(
        &self,
        repo: &Repo,
        head: &str,
    ) -> Result<Option<PullRequest>> {
        let key = format!("{}:{}", repo.full_name, head);
        Ok(self.open_prs.lock().unwrap().get(&key).cloned())
    }

    async fn create_pr(
        &self,
        repo: &Repo,
        branch: &str,
        _base: &str,
        title: &str,
        _body: &str,
        _changes: Vec<FileChange>,
    ) -> Result<PullRequest> {
        let n = self.create_pr_calls.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        Ok(PullRequest {
            number: n,
            title: title.to_string(),
            head_branch: branch.to_string(),
            url: format!("https://github.com/{}/pull/{n}", repo.full_name),
            state: PrState::Open,
        })
    }

    async fn update_pr(
        &self,
        _repo: &Repo,
        pr: &PullRequest,
        _changes: Vec<FileChange>,
    ) -> Result<PullRequest> {
        self.update_pr_calls.fetch_add(1, Ordering::SeqCst);
        Ok(pr.clone())
    }
}

/// Create a simple `Repo` from a `"owner/name"` string.
pub fn make_repo(full_name: &str) -> Repo {
    let parts: Vec<&str> = full_name.splitn(2, '/').collect();
    Repo {
        owner: parts[0].to_string(),
        name: parts.get(1).unwrap_or(&"").to_string(),
        full_name: full_name.to_string(),
        default_branch: "main".to_string(),
        archived: false,
        visibility: Visibility::Private,
        topics: vec![],
        language: None,
        custom_properties: HashMap::new(),
    }
}

/// Create an archived `Repo`.
pub fn make_repo_archived(full_name: &str) -> Repo {
    let mut repo = make_repo(full_name);
    repo.archived = true;
    repo
}

/// Create a `RootConfig` with the given policies.
pub fn test_config(policies: Vec<PolicyConfig>) -> RootConfig {
    RootConfig {
        provider: ProviderConfig {
            provider_type: ProviderType::Github,
            org: "org".to_string(),
            token: "ghp_test".to_string(),
            base_url: None,
        },
        defaults: None,
        policies,
    }
}

/// Create a minimal `RootConfig` with no policies.
pub fn default_config() -> RootConfig {
    test_config(vec![])
}

/// Build a `Report` with failing repos, optionally with remediations.
pub fn make_report_with_failures(repo_names: &[&str], with_remediations: bool) -> Report {
    let remediations = if with_remediations {
        vec![Remediation {
            description: "Create LICENSE file".to_string(),
            changes: vec![FileChange {
                path: "LICENSE".to_string(),
                content: Some("MIT License".to_string()),
                message: "Add LICENSE".to_string(),
            }],
        }]
    } else {
        vec![]
    };

    let repo_results = repo_names
        .iter()
        .map(|name| RepoResult {
            repo_name: name.to_string(),
            default_branch: "main".to_string(),
            outcome: RepoOutcome::Fail {
                detail: "Missing LICENSE".to_string(),
                remediations: remediations.clone(),
            },
        })
        .collect();

    Report {
        results: vec![PolicyReport {
            policy_name: "require-license".to_string(),
            description: Some("All repos must have LICENSE".to_string()),
            severity: Severity::Error,
            repo_results,
        }],
        summary: Summary {
            total_repos: repo_names.len(),
            passing: 0,
            failing: repo_names.len(),
            skipped: 0,
            errors: 0,
        },
    }
}
