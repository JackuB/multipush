//! GitHub provider implementation for multipush.
//!
//! Implements the `multipush_core::provider::Provider` trait using the GitHub
//! REST API via octocrab.

use async_trait::async_trait;
use multipush_core::config::ProviderConfig;
use multipush_core::error::CoreError;
use multipush_core::model::{FileChange, FileContent, PullRequest, Repo, RepoSettings, Visibility};
use multipush_core::provider::Provider;
use octocrab::Octocrab;
use std::collections::HashMap;

pub struct GitHubProvider {
    client: Octocrab,
    #[allow(dead_code)] // Used in apply mode session
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
        _repo: &Repo,
        _head_branch: &str,
    ) -> multipush_core::Result<Option<PullRequest>> {
        todo!("Implemented in apply mode session")
    }

    async fn create_pr(
        &self,
        _repo: &Repo,
        _branch: &str,
        _base: &str,
        _title: &str,
        _body: &str,
        _changes: Vec<FileChange>,
    ) -> multipush_core::Result<PullRequest> {
        todo!("Implemented in apply mode session")
    }

    async fn update_pr(
        &self,
        _repo: &Repo,
        _pr: &PullRequest,
        _changes: Vec<FileChange>,
    ) -> multipush_core::Result<PullRequest> {
        todo!("Implemented in apply mode session")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_token_from_env() {
        // Use a unique env var name to avoid conflicts with parallel tests
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
}
