use async_trait::async_trait;

use crate::model::{FileChange, FileContent, PullRequest, Repo, RepoSettings};
use crate::Result;

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    async fn list_repos(&self, org: &str) -> Result<Vec<Repo>>;

    async fn get_file(&self, repo: &Repo, path: &str, git_ref: &str)
        -> Result<Option<FileContent>>;

    async fn get_repo_settings(&self, repo: &Repo) -> Result<RepoSettings>;

    async fn find_open_pr(
        &self,
        repo: &Repo,
        head_branch: &str,
    ) -> Result<Option<PullRequest>>;

    async fn create_pr(
        &self,
        repo: &Repo,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
        changes: Vec<FileChange>,
    ) -> Result<PullRequest>;

    async fn update_pr(
        &self,
        repo: &Repo,
        pr: &PullRequest,
        changes: Vec<FileChange>,
    ) -> Result<PullRequest>;
}
