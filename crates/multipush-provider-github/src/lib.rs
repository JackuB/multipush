//! GitHub provider implementation for multipush.
//!
//! Implements the `multipush_core::provider::Provider` trait using the GitHub
//! REST API via octocrab.

use async_trait::async_trait;
use multipush_core::config::ProviderConfig;
use multipush_core::error::CoreError;
use multipush_core::model::{
    FileChange, FileContent, PrState, PullRequest, Repo, RepoSettings, Visibility,
};
use multipush_core::provider::Provider;
use octocrab::Octocrab;
use std::collections::HashMap;

// --- Git Trees API structs (not covered by octocrab typed API) ---

#[derive(serde::Serialize)]
struct CreateTreeRequest {
    base_tree: String,
    tree: Vec<TreeEntry>,
}

#[derive(serde::Serialize)]
struct TreeEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    entry_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct GitTreeResponse {
    sha: String,
}

#[derive(serde::Serialize)]
struct UpdateRefRequest {
    sha: String,
    force: bool,
}

pub struct GitHubProvider {
    client: Octocrab,
    org: String,
}

impl GitHubProvider {
    pub fn new(config: &ProviderConfig) -> multipush_core::Result<Self> {
        let token = resolve_token(&config.token)?;

        let mut builder = Octocrab::builder().personal_token(token);

        if let Some(ref url) = config.base_url {
            builder = builder
                .base_uri(url)
                .map_err(|e| CoreError::Provider(e.to_string()))?;
        }

        let client = builder
            .build()
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        Ok(Self {
            client,
            org: config.org.clone(),
        })
    }

    async fn get_branch_sha(&self, repo: &Repo, branch: &str) -> multipush_core::Result<String> {
        let reference = self
            .client
            .repos(&repo.owner, &repo.name)
            .get_ref(&octocrab::params::repos::Reference::Branch(
                branch.to_string(),
            ))
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        match reference.object {
            octocrab::models::repos::Object::Commit { sha, .. } => Ok(sha),
            octocrab::models::repos::Object::Tag { sha, .. } => Ok(sha),
            other => Err(CoreError::Provider(format!(
                "Unexpected ref object type: {other:?}"
            ))),
        }
    }

    async fn create_or_reuse_branch(
        &self,
        repo: &Repo,
        branch: &str,
        base_sha: &str,
    ) -> multipush_core::Result<()> {
        let result = self
            .client
            .repos(&repo.owner, &repo.name)
            .create_ref(
                &octocrab::params::repos::Reference::Branch(branch.to_string()),
                base_sha,
            )
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(octocrab::Error::GitHub { source, .. })
                if source.status_code.as_u16() == 422 =>
            {
                tracing::debug!("Branch {} already exists, reusing", branch);
                Ok(())
            }
            Err(e) => Err(CoreError::Provider(e.to_string())),
        }
    }

    async fn push_single_file_change(
        &self,
        repo: &Repo,
        branch: &str,
        change: &FileChange,
    ) -> multipush_core::Result<()> {
        let repos = self.client.repos(&repo.owner, &repo.name);

        if let Some(ref content) = change.content {
            let existing = self.get_file(repo, &change.path, branch).await?;
            match existing {
                Some(file) => {
                    repos
                        .update_file(&change.path, &change.message, content, &file.sha)
                        .branch(branch)
                        .send()
                        .await
                        .map_err(|e| CoreError::Provider(e.to_string()))?;
                }
                None => {
                    repos
                        .create_file(&change.path, &change.message, content)
                        .branch(branch)
                        .send()
                        .await
                        .map_err(|e| CoreError::Provider(e.to_string()))?;
                }
            }
        } else {
            let existing = self
                .get_file(repo, &change.path, branch)
                .await?
                .ok_or_else(|| {
                    CoreError::Provider(format!(
                        "Cannot delete non-existent file: {}",
                        change.path
                    ))
                })?;
            repos
                .delete_file(&change.path, &change.message, &existing.sha)
                .branch(branch)
                .send()
                .await
                .map_err(|e| CoreError::Provider(e.to_string()))?;
        }

        Ok(())
    }

    async fn push_changes_tree_api(
        &self,
        repo: &Repo,
        branch: &str,
        changes: &[FileChange],
    ) -> multipush_core::Result<()> {
        let branch_sha = self.get_branch_sha(repo, branch).await?;

        // Get the commit's tree SHA
        let commit: serde_json::Value = self
            .client
            .get(
                format!(
                    "/repos/{}/{}/git/commits/{}",
                    repo.owner, repo.name, branch_sha
                ),
                None::<&()>,
            )
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        let base_tree_sha = commit["tree"]["sha"]
            .as_str()
            .ok_or_else(|| CoreError::Provider("Missing tree SHA in commit".into()))?
            .to_string();

        // Build tree entries
        let tree_entries: Vec<TreeEntry> = changes
            .iter()
            .map(|c| {
                if c.content.is_some() {
                    TreeEntry {
                        path: c.path.clone(),
                        mode: "100644".to_string(),
                        entry_type: "blob".to_string(),
                        content: c.content.clone(),
                        sha: None,
                    }
                } else {
                    TreeEntry {
                        path: c.path.clone(),
                        mode: "100644".to_string(),
                        entry_type: "blob".to_string(),
                        content: None,
                        sha: Some(serde_json::Value::Null),
                    }
                }
            })
            .collect();

        // Create new tree
        let tree_response: GitTreeResponse = self
            .client
            .post(
                format!("/repos/{}/{}/git/trees", repo.owner, repo.name),
                Some(&CreateTreeRequest {
                    base_tree: base_tree_sha,
                    tree: tree_entries,
                }),
            )
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        // Create commit
        let message = changes
            .first()
            .map(|c| c.message.as_str())
            .unwrap_or("multipush apply");

        let new_commit: serde_json::Value = self
            .client
            .post(
                format!("/repos/{}/{}/git/commits", repo.owner, repo.name),
                Some(&serde_json::json!({
                    "message": message,
                    "tree": tree_response.sha,
                    "parents": [branch_sha],
                })),
            )
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        let new_commit_sha = new_commit["sha"]
            .as_str()
            .ok_or_else(|| CoreError::Provider("Missing SHA in new commit".into()))?;

        // Update branch ref
        let _: serde_json::Value = self
            .client
            .patch(
                format!(
                    "/repos/{}/{}/git/refs/heads/{}",
                    repo.owner, repo.name, branch
                ),
                Some(&UpdateRefRequest {
                    sha: new_commit_sha.to_string(),
                    force: false,
                }),
            )
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        Ok(())
    }
}

fn resolve_token(raw: &str) -> multipush_core::Result<String> {
    if raw == "${GITHUB_TOKEN}" {
        match std::env::var("GITHUB_TOKEN") {
            Ok(val) if !val.is_empty() => Ok(val),
            _ => Err(CoreError::Provider(
                "GitHub token not configured. Set GITHUB_TOKEN environment variable or add provider.token to your config.".into(),
            )),
        }
    } else if raw.is_empty() {
        Err(CoreError::Provider(
            "GitHub token not configured. Set GITHUB_TOKEN environment variable or add provider.token to your config.".into(),
        ))
    } else {
        Ok(raw.to_string())
    }
}

fn map_visibility(vis: Option<&str>) -> Visibility {
    match vis {
        Some("public") => Visibility::Public,
        Some("internal") => Visibility::Internal,
        _ => Visibility::Private,
    }
}

fn map_octocrab_pr(pr: &octocrab::models::pulls::PullRequest) -> PullRequest {
    use octocrab::models::IssueState;

    let state = match pr.state.as_ref() {
        Some(IssueState::Open) => PrState::Open,
        Some(IssueState::Closed) => {
            if pr.merged_at.is_some() {
                PrState::Merged
            } else {
                PrState::Closed
            }
        }
        _ => PrState::Open,
    };

    PullRequest {
        number: pr.number,
        title: pr.title.clone().unwrap_or_default(),
        head_branch: pr.head.ref_field.clone(),
        url: pr
            .html_url
            .as_ref()
            .map(|u| u.to_string())
            .unwrap_or_default(),
        state,
    }
}

fn map_repo(repo: octocrab::models::Repository, org: &str) -> Repo {
    let name = repo.name;
    let full_name = repo
        .full_name
        .unwrap_or_else(|| format!("{org}/{name}"));
    let owner = repo
        .owner
        .map(|o| o.login)
        .unwrap_or_else(|| org.to_string());
    let default_branch = repo.default_branch.unwrap_or_else(|| "main".to_string());
    let archived = repo.archived.unwrap_or(false);
    let visibility = map_visibility(repo.visibility.as_deref());
    let topics = repo.topics.unwrap_or_default();
    let language = repo
        .language
        .as_ref()
        .and_then(|v| v.as_str())
        .map(String::from);

    Repo {
        name,
        full_name,
        owner,
        default_branch,
        archived,
        visibility,
        topics,
        language,
        custom_properties: HashMap::new(),
    }
}

#[async_trait]
impl Provider for GitHubProvider {
    fn name(&self) -> &str {
        "github"
    }

    async fn list_repos(&self, org: &str) -> multipush_core::Result<Vec<Repo>> {
        let mut page = self
            .client
            .orgs(org)
            .list_repos()
            .per_page(100)
            .send()
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        let mut repos = page.take_items();

        while let Some(next_page) = self
            .client
            .get_page::<octocrab::models::Repository>(&page.next)
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?
        {
            page = next_page;
            repos.extend(page.take_items());
        }

        let result: Vec<Repo> = repos
            .into_iter()
            .map(|r| map_repo(r, org))
            .collect();

        tracing::debug!("Listed {} repos for org {}", result.len(), org);

        Ok(result)
    }

    async fn get_file(
        &self,
        repo: &Repo,
        path: &str,
        git_ref: &str,
    ) -> multipush_core::Result<Option<FileContent>> {
        let result = self
            .client
            .repos(&repo.owner, &repo.name)
            .get_content()
            .path(path)
            .r#ref(git_ref)
            .send()
            .await;

        match result {
            Ok(content) => {
                let item = content
                    .items
                    .into_iter()
                    .next()
                    .ok_or_else(|| CoreError::Provider("No content items returned".into()))?;

                let decoded = item.decoded_content().ok_or_else(|| {
                    CoreError::Provider("Failed to decode file content".into())
                })?;

                Ok(Some(FileContent {
                    path: path.to_string(),
                    content: decoded,
                    sha: item.sha,
                }))
            }
            Err(octocrab::Error::GitHub { source, .. })
                if source.status_code.as_u16() == 404 =>
            {
                Ok(None)
            }
            Err(e) => Err(CoreError::Provider(e.to_string())),
        }
    }

    async fn get_repo_settings(&self, _repo: &Repo) -> multipush_core::Result<RepoSettings> {
        todo!("Implemented in apply mode session")
    }

    async fn find_open_pr(
        &self,
        repo: &Repo,
        head_branch: &str,
    ) -> multipush_core::Result<Option<PullRequest>> {
        let head = format!("{}:{}", self.org, head_branch);
        let page = self
            .client
            .pulls(&repo.owner, &repo.name)
            .list()
            .state(octocrab::params::State::Open)
            .head(&head)
            .per_page(1)
            .send()
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        Ok(page.items.first().map(map_octocrab_pr))
    }

    async fn create_pr(
        &self,
        repo: &Repo,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
        changes: Vec<FileChange>,
    ) -> multipush_core::Result<PullRequest> {
        let base_sha = self.get_branch_sha(repo, base).await?;
        self.create_or_reuse_branch(repo, branch, &base_sha)
            .await?;

        if changes.len() >= 2 {
            self.push_changes_tree_api(repo, branch, &changes).await?;
        } else {
            for change in &changes {
                self.push_single_file_change(repo, branch, change).await?;
            }
        }

        let pr = self
            .client
            .pulls(&repo.owner, &repo.name)
            .create(title, branch, base)
            .body(body)
            .send()
            .await
            .map_err(|e| CoreError::Provider(e.to_string()))?;

        Ok(map_octocrab_pr(&pr))
    }

    async fn update_pr(
        &self,
        repo: &Repo,
        pr: &PullRequest,
        changes: Vec<FileChange>,
    ) -> multipush_core::Result<PullRequest> {
        if changes.len() >= 2 {
            self.push_changes_tree_api(repo, &pr.head_branch, &changes)
                .await?;
        } else {
            for change in &changes {
                self.push_single_file_change(repo, &pr.head_branch, change)
                    .await?;
            }
        }

        Ok(pr.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use multipush_core::config::ProviderType;
    use multipush_core::model::PrState;
    use multipush_core::provider::Provider;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Guards env-var-mutating tests so they don't race each other.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_resolve_token_from_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("GITHUB_TOKEN", "ghp_test123");
        let result = resolve_token("${GITHUB_TOKEN}");
        assert_eq!(result.unwrap(), "ghp_test123");
    }

    #[test]
    fn test_resolve_token_literal() {
        let result = resolve_token("ghp_literal_token");
        assert_eq!(result.unwrap(), "ghp_literal_token");
    }

    #[test]
    fn test_resolve_token_empty_fails() {
        let result = resolve_token("");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("GitHub token not configured"));
    }

    #[test]
    fn test_resolve_token_missing_env_fails() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::remove_var("GITHUB_TOKEN");
        let result = resolve_token("${GITHUB_TOKEN}");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("GitHub token not configured"));
    }

    #[test]
    fn test_map_visibility() {
        assert_eq!(map_visibility(Some("public")), Visibility::Public);
        assert_eq!(map_visibility(Some("private")), Visibility::Private);
        assert_eq!(map_visibility(Some("internal")), Visibility::Internal);
        assert_eq!(map_visibility(None), Visibility::Private);
        assert_eq!(map_visibility(Some("unknown")), Visibility::Private);
    }

    fn sample_owner(login: &str) -> serde_json::Value {
        serde_json::json!({
            "login": login,
            "id": 1,
            "node_id": "MDQ6VXNlcjE=",
            "gravatar_id": "",
            "avatar_url": "https://github.com/images/error/octocat_happy.gif",
            "url": format!("https://api.github.com/users/{login}"),
            "html_url": format!("https://github.com/{login}"),
            "type": "Organization",
            "followers_url": format!("https://api.github.com/users/{login}/followers"),
            "following_url": format!("https://api.github.com/users/{login}/following{{/other_user}}"),
            "gists_url": format!("https://api.github.com/users/{login}/gists{{/gist_id}}"),
            "starred_url": format!("https://api.github.com/users/{login}/starred{{/owner}}{{/repo}}"),
            "subscriptions_url": format!("https://api.github.com/users/{login}/subscriptions"),
            "organizations_url": format!("https://api.github.com/users/{login}/orgs"),
            "repos_url": format!("https://api.github.com/users/{login}/repos"),
            "events_url": format!("https://api.github.com/users/{login}/events{{/privacy}}"),
            "received_events_url": format!("https://api.github.com/users/{login}/received_events"),
            "site_admin": false
        })
    }

    #[test]
    fn test_map_repo_from_json() {
        let json = serde_json::json!({
            "id": 1,
            "node_id": "MDEwOlJlcG9zaXRvcnkx",
            "name": "my-repo",
            "full_name": "my-org/my-repo",
            "owner": sample_owner("my-org"),
            "html_url": "https://github.com/my-org/my-repo",
            "url": "https://api.github.com/repos/my-org/my-repo",
            "default_branch": "develop",
            "archived": false,
            "visibility": "public",
            "topics": ["rust", "cli"],
            "language": "Rust",
            "created_at": "2020-01-01T00:00:00Z",
            "updated_at": "2020-06-01T00:00:00Z"
        });

        let gh_repo: octocrab::models::Repository = serde_json::from_value(json).unwrap();
        let repo = map_repo(gh_repo, "my-org");

        assert_eq!(repo.name, "my-repo");
        assert_eq!(repo.full_name, "my-org/my-repo");
        assert_eq!(repo.owner, "my-org");
        assert_eq!(repo.default_branch, "develop");
        assert!(!repo.archived);
        assert_eq!(repo.visibility, Visibility::Public);
        assert_eq!(repo.topics, vec!["rust", "cli"]);
        assert_eq!(repo.language, Some("Rust".to_string()));
        assert!(repo.custom_properties.is_empty());
    }

    #[test]
    fn test_map_repo_with_defaults() {
        let json = serde_json::json!({
            "id": 2,
            "node_id": "MDEwOlJlcG9zaXRvcnky",
            "name": "minimal-repo",
            "owner": sample_owner("test-org"),
            "html_url": "https://github.com/test-org/minimal-repo",
            "url": "https://api.github.com/repos/test-org/minimal-repo",
            "created_at": "2020-01-01T00:00:00Z",
            "updated_at": "2020-06-01T00:00:00Z"
        });

        let gh_repo: octocrab::models::Repository = serde_json::from_value(json).unwrap();
        let repo = map_repo(gh_repo, "test-org");

        assert_eq!(repo.name, "minimal-repo");
        assert_eq!(repo.full_name, "test-org/minimal-repo");
        assert_eq!(repo.default_branch, "main");
        assert!(!repo.archived);
        assert_eq!(repo.visibility, Visibility::Private);
        assert!(repo.topics.is_empty());
        assert!(repo.language.is_none());
    }

    // --- Wiremock integration tests ---

    async fn provider_with_mock(server: &MockServer) -> GitHubProvider {
        let config = ProviderConfig {
            provider_type: ProviderType::Github,
            org: "test-org".to_string(),
            token: "ghp_test".to_string(),
            base_url: Some(server.uri()),
        };
        GitHubProvider::new(&config).unwrap()
    }

    fn test_repo() -> Repo {
        Repo {
            name: "test-repo".to_string(),
            full_name: "test-org/test-repo".to_string(),
            owner: "test-org".to_string(),
            default_branch: "main".to_string(),
            archived: false,
            visibility: Visibility::Private,
            topics: vec![],
            language: None,
            custom_properties: HashMap::new(),
        }
    }

    fn pr_json(number: u64, branch: &str) -> serde_json::Value {
        serde_json::json!({
            "url": format!("https://api.github.com/repos/test-org/test-repo/pulls/{number}"),
            "id": number,
            "html_url": format!("https://github.com/test-org/test-repo/pull/{number}"),
            "number": number,
            "state": "open",
            "title": "Fix policy",
            "head": {
                "ref": branch,
                "sha": "head_sha_123",
                "label": format!("test-org:{branch}")
            },
            "base": {
                "ref": "main",
                "sha": "base_sha_456",
                "label": "test-org:main"
            },
            "created_at": "2020-01-01T00:00:00Z",
            "updated_at": "2020-01-01T00:00:00Z"
        })
    }

    fn ref_json(sha: &str) -> serde_json::Value {
        serde_json::json!({
            "ref": "refs/heads/main",
            "node_id": "MDM6UmVmcmVmcy9oZWFkcy9tYWlu",
            "url": "https://api.github.com/repos/test-org/test-repo/git/refs/heads/main",
            "object": {
                "type": "commit",
                "sha": sha,
                "url": format!("https://api.github.com/repos/test-org/test-repo/git/commits/{sha}")
            }
        })
    }

    fn file_content_json(path: &str, sha: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "file",
            "encoding": "base64",
            "size": 12,
            "name": path,
            "path": path,
            "content": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "Hello World"),
            "sha": sha,
            "url": format!("https://api.github.com/repos/test-org/test-repo/contents/{path}"),
            "git_url": format!("https://api.github.com/repos/test-org/test-repo/git/blobs/{sha}"),
            "html_url": format!("https://github.com/test-org/test-repo/blob/main/{path}"),
            "download_url": format!("https://raw.githubusercontent.com/test-org/test-repo/main/{path}"),
            "_links": {
                "self": format!("https://api.github.com/repos/test-org/test-repo/contents/{path}"),
                "git": format!("https://api.github.com/repos/test-org/test-repo/git/blobs/{sha}"),
                "html": format!("https://github.com/test-org/test-repo/blob/main/{path}")
            }
        })
    }

    fn file_update_json() -> serde_json::Value {
        serde_json::json!({
            "content": {
                "name": "README.md",
                "path": "README.md",
                "sha": "new_sha_456",
                "size": 12,
                "url": "https://api.github.com/repos/test-org/test-repo/contents/README.md",
                "html_url": "https://github.com/test-org/test-repo/blob/main/README.md",
                "git_url": "https://api.github.com/repos/test-org/test-repo/git/blobs/new_sha_456",
                "download_url": "https://raw.githubusercontent.com/test-org/test-repo/main/README.md",
                "type": "file",
                "_links": {
                    "self": "https://api.github.com/repos/test-org/test-repo/contents/README.md",
                    "git": "https://api.github.com/repos/test-org/test-repo/git/blobs/new_sha_456",
                    "html": "https://github.com/test-org/test-repo/blob/main/README.md"
                }
            },
            "commit": {
                "sha": "commit_sha_789",
                "node_id": "MDY6Q29tbWl0",
                "url": "https://api.github.com/repos/test-org/test-repo/git/commits/commit_sha_789",
                "html_url": "https://github.com/test-org/test-repo/commit/commit_sha_789",
                "author": { "date": "2020-01-01T00:00:00Z", "name": "bot", "email": "bot@test.com" },
                "committer": { "date": "2020-01-01T00:00:00Z", "name": "bot", "email": "bot@test.com" },
                "message": "create file",
                "tree": { "url": "https://api.github.com/repos/test-org/test-repo/git/trees/tree_sha", "sha": "tree_sha" },
                "parents": []
            }
        })
    }

    fn github_error_json(message: &str) -> serde_json::Value {
        serde_json::json!({
            "message": message,
            "documentation_url": "https://docs.github.com/rest"
        })
    }

    #[tokio::test]
    async fn find_open_pr_found() {
        let server = MockServer::start().await;
        let provider = provider_with_mock(&server).await;
        let repo = test_repo();

        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([
                    pr_json(42, "multipush/fix")
                ])),
            )
            .mount(&server)
            .await;

        let result = provider.find_open_pr(&repo, "multipush/fix").await.unwrap();
        let pr = result.unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Fix policy");
        assert_eq!(pr.head_branch, "multipush/fix");
        assert_eq!(pr.state, PrState::Open);
        assert!(pr.url.contains("/pull/42"));
    }

    #[tokio::test]
    async fn find_open_pr_none() {
        let server = MockServer::start().await;
        let provider = provider_with_mock(&server).await;
        let repo = test_repo();

        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([])),
            )
            .mount(&server)
            .await;

        let result = provider.find_open_pr(&repo, "multipush/fix").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn create_pr_single_file() {
        let server = MockServer::start().await;
        let provider = provider_with_mock(&server).await;
        let repo = test_repo();

        // GET ref (base branch)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/git/ref/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ref_json("base_sha_abc")))
            .mount(&server)
            .await;

        // POST refs (create branch)
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/git/refs"))
            .respond_with(ResponseTemplate::new(201).set_body_json(ref_json("base_sha_abc")))
            .mount(&server)
            .await;

        // GET contents (file doesn't exist)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(github_error_json("Not Found")),
            )
            .mount(&server)
            .await;

        // PUT contents (create file)
        Mock::given(method("PUT"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(ResponseTemplate::new(201).set_body_json(file_update_json()))
            .mount(&server)
            .await;

        // POST pulls (create PR)
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/pulls"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(pr_json(99, "multipush/fix")),
            )
            .mount(&server)
            .await;

        let changes = vec![FileChange {
            path: "README.md".to_string(),
            content: Some("# Hello".to_string()),
            message: "Add README".to_string(),
        }];

        let result = provider
            .create_pr(&repo, "multipush/fix", "main", "Fix policy", "Body", changes)
            .await
            .unwrap();

        assert_eq!(result.number, 99);
        assert_eq!(result.head_branch, "multipush/fix");
    }

    #[tokio::test]
    async fn create_pr_multi_file() {
        let server = MockServer::start().await;
        let provider = provider_with_mock(&server).await;
        let repo = test_repo();

        // GET ref (base branch)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/git/ref/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ref_json("base_sha_abc")))
            .expect(1..)
            .mount(&server)
            .await;

        // POST refs (create branch)
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/git/refs"))
            .respond_with(ResponseTemplate::new(201).set_body_json(ref_json("base_sha_abc")))
            .mount(&server)
            .await;

        // GET ref (feature branch - for tree API)
        Mock::given(method("GET"))
            .and(path(
                "/repos/test-org/test-repo/git/ref/heads/multipush/fix",
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(ref_json("branch_sha_def")),
            )
            .mount(&server)
            .await;

        // GET git/commits (get tree SHA)
        Mock::given(method("GET"))
            .and(path(
                "/repos/test-org/test-repo/git/commits/branch_sha_def",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "branch_sha_def",
                "tree": { "sha": "tree_sha_base", "url": "https://api.github.com/..." },
                "message": "Previous commit",
                "parents": [{ "sha": "parent_sha" }]
            })))
            .mount(&server)
            .await;

        // POST git/trees
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/git/trees"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "sha": "new_tree_sha",
                "url": "https://api.github.com/repos/test-org/test-repo/git/trees/new_tree_sha",
                "tree": []
            })))
            .mount(&server)
            .await;

        // POST git/commits (create commit)
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/git/commits"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "sha": "new_commit_sha",
                "tree": { "sha": "new_tree_sha" },
                "message": "Add files",
                "parents": [{ "sha": "branch_sha_def" }]
            })))
            .mount(&server)
            .await;

        // PATCH git/refs (update branch)
        Mock::given(method("PATCH"))
            .and(path(
                "/repos/test-org/test-repo/git/refs/heads/multipush/fix",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ref": "refs/heads/multipush/fix",
                "object": { "type": "commit", "sha": "new_commit_sha" }
            })))
            .mount(&server)
            .await;

        // POST pulls (create PR)
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/pulls"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(pr_json(100, "multipush/fix")),
            )
            .mount(&server)
            .await;

        let changes = vec![
            FileChange {
                path: "README.md".to_string(),
                content: Some("# Hello".to_string()),
                message: "Add files".to_string(),
            },
            FileChange {
                path: "LICENSE".to_string(),
                content: Some("MIT".to_string()),
                message: "Add files".to_string(),
            },
        ];

        let result = provider
            .create_pr(
                &repo,
                "multipush/fix",
                "main",
                "Fix policy",
                "Body",
                changes,
            )
            .await
            .unwrap();

        assert_eq!(result.number, 100);
    }

    #[tokio::test]
    async fn create_pr_branch_exists() {
        let server = MockServer::start().await;
        let provider = provider_with_mock(&server).await;
        let repo = test_repo();

        // GET ref (base branch)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/git/ref/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ref_json("base_sha_abc")))
            .mount(&server)
            .await;

        // POST refs → 422 (branch already exists)
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/git/refs"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_json(github_error_json("Reference already exists")),
            )
            .mount(&server)
            .await;

        // GET contents (file doesn't exist)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(github_error_json("Not Found")),
            )
            .mount(&server)
            .await;

        // PUT contents (create file)
        Mock::given(method("PUT"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(ResponseTemplate::new(201).set_body_json(file_update_json()))
            .mount(&server)
            .await;

        // POST pulls
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/pulls"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(pr_json(101, "multipush/fix")),
            )
            .mount(&server)
            .await;

        let changes = vec![FileChange {
            path: "README.md".to_string(),
            content: Some("# Hello".to_string()),
            message: "Add README".to_string(),
        }];

        let result = provider
            .create_pr(&repo, "multipush/fix", "main", "Fix policy", "Body", changes)
            .await
            .unwrap();

        assert_eq!(result.number, 101);
    }

    #[tokio::test]
    async fn create_pr_file_exists() {
        let server = MockServer::start().await;
        let provider = provider_with_mock(&server).await;
        let repo = test_repo();

        // GET ref (base branch)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/git/ref/heads/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ref_json("base_sha_abc")))
            .mount(&server)
            .await;

        // POST refs (create branch)
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/git/refs"))
            .respond_with(ResponseTemplate::new(201).set_body_json(ref_json("base_sha_abc")))
            .mount(&server)
            .await;

        // GET contents → 200 (file exists with sha)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(file_content_json("README.md", "existing_sha_999")),
            )
            .mount(&server)
            .await;

        // PUT contents (update file - should include existing sha)
        Mock::given(method("PUT"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_update_json()))
            .mount(&server)
            .await;

        // POST pulls
        Mock::given(method("POST"))
            .and(path("/repos/test-org/test-repo/pulls"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(pr_json(102, "multipush/fix")),
            )
            .mount(&server)
            .await;

        let changes = vec![FileChange {
            path: "README.md".to_string(),
            content: Some("# Updated".to_string()),
            message: "Update README".to_string(),
        }];

        let result = provider
            .create_pr(&repo, "multipush/fix", "main", "Fix policy", "Body", changes)
            .await
            .unwrap();

        assert_eq!(result.number, 102);
    }

    #[tokio::test]
    async fn update_pr_pushes_changes() {
        let server = MockServer::start().await;
        let provider = provider_with_mock(&server).await;
        let repo = test_repo();

        let existing_pr = PullRequest {
            number: 50,
            title: "Existing PR".to_string(),
            head_branch: "multipush/fix".to_string(),
            url: "https://github.com/test-org/test-repo/pull/50".to_string(),
            state: PrState::Open,
        };

        // GET contents (file doesn't exist)
        Mock::given(method("GET"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(github_error_json("Not Found")),
            )
            .mount(&server)
            .await;

        // PUT contents (create file)
        Mock::given(method("PUT"))
            .and(path("/repos/test-org/test-repo/contents/README.md"))
            .respond_with(ResponseTemplate::new(201).set_body_json(file_update_json()))
            .mount(&server)
            .await;

        let changes = vec![FileChange {
            path: "README.md".to_string(),
            content: Some("# Hello".to_string()),
            message: "Add README".to_string(),
        }];

        let result = provider.update_pr(&repo, &existing_pr, changes).await.unwrap();

        // update_pr returns the original PR unchanged
        assert_eq!(result.number, 50);
        assert_eq!(result.title, "Existing PR");
        assert_eq!(result.head_branch, "multipush/fix");
    }
}
