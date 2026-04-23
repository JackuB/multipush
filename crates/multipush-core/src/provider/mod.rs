use async_trait::async_trait;

use crate::model::{
    BranchProtection, BranchProtectionPatch, FileChange, FileContent, PullRequest, Repo,
    RepoSettings, RepoSettingsPatch,
};
use crate::Result;

/// Backend for interacting with a Git hosting platform (e.g. GitHub).
///
/// All methods operate via API — no local clones are required.
/// Implementations must be `Send + Sync` so they can be shared across
/// concurrent rule evaluations.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Human-readable provider name (e.g. `"github"`).
    fn name(&self) -> &str;

    /// List all repositories in the given organisation or user account.
    async fn list_repos(&self, org: &str) -> Result<Vec<Repo>>;

    /// Fetch a single file's contents at the given git ref.
    /// Returns `None` if the file does not exist.
    async fn get_file(&self, repo: &Repo, path: &str, git_ref: &str)
        -> Result<Option<FileContent>>;

    /// Retrieve repository-level settings (merge strategies, features, etc.).
    async fn get_repo_settings(&self, repo: &Repo) -> Result<RepoSettings>;

    /// Find an open pull request whose head branch matches `head_branch`.
    async fn find_open_pr(&self, repo: &Repo, head_branch: &str) -> Result<Option<PullRequest>>;

    /// Create a new pull request with the given file changes.
    async fn create_pr(
        &self,
        repo: &Repo,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
        changes: Vec<FileChange>,
    ) -> Result<PullRequest>;

    /// Push updated file changes to an existing pull request.
    async fn update_pr(
        &self,
        repo: &Repo,
        pr: &PullRequest,
        changes: Vec<FileChange>,
    ) -> Result<PullRequest>;

    /// Apply a partial update to repository-level settings.
    async fn update_repo_settings(&self, repo: &Repo, patch: &RepoSettingsPatch) -> Result<()>;

    /// Retrieve branch protection settings for a branch.
    /// Returns `None` if the branch has no protection configured.
    async fn get_branch_protection(
        &self,
        repo: &Repo,
        branch: &str,
    ) -> Result<Option<BranchProtection>>;

    /// Apply a partial update to branch protection for a branch.
    async fn update_branch_protection(
        &self,
        repo: &Repo,
        branch: &str,
        patch: &BranchProtectionPatch,
    ) -> Result<()>;
}
