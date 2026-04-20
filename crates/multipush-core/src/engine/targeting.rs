use std::collections::HashMap;

use globset::{Glob, GlobSetBuilder};

use crate::config::{FilterConfig, TargetConfig};
use crate::error::CoreError;
use crate::model::Repo;
use crate::provider::Provider;
use crate::Result;

/// Apply name globs, exclude lists, and `exclude_archived` to `repos`.
///
/// This intentionally does not run the more expensive `filters` (`has_file`,
/// `topic`, `visibility`). Use [`filter_repos`] for the full async filter
/// chain.
pub fn filter_repos_basic(repos: &[Repo], targets: &TargetConfig) -> Result<Vec<Repo>> {
    let include = Glob::new(&targets.repos)
        .map_err(|e| CoreError::Config(format!("invalid include glob '{}': {e}", targets.repos)))?
        .compile_matcher();

    let exclude = {
        let mut builder = GlobSetBuilder::new();
        for pattern in &targets.exclude {
            let glob = Glob::new(pattern)
                .map_err(|e| CoreError::Config(format!("invalid exclude glob '{pattern}': {e}")))?;
            builder.add(glob);
        }
        builder
            .build()
            .map_err(|e| CoreError::Config(format!("failed to build exclude set: {e}")))?
    };

    Ok(repos
        .iter()
        .filter(|r| include.is_match(&r.full_name))
        .filter(|r| !exclude.is_match(&r.full_name))
        .filter(|r| !(targets.exclude_archived && r.archived))
        .cloned()
        .collect())
}

/// Apply targeting (name globs, excludes, archived, and `filters`) to `repos`.
///
/// `has_file` calls go through `provider.get_file` and are cached for the
/// lifetime of this call by `(full_name, path)`.
pub async fn filter_repos(
    repos: &[Repo],
    targets: &TargetConfig,
    provider: &dyn Provider,
) -> Result<Vec<Repo>> {
    let basic = filter_repos_basic(repos, targets)?;

    if targets.filters.is_empty() {
        return Ok(basic);
    }

    let mut file_cache: HashMap<(String, String), bool> = HashMap::new();
    let mut result = Vec::with_capacity(basic.len());

    'repo: for repo in basic {
        for filter in &targets.filters {
            match filter {
                FilterConfig::HasFile(path) => {
                    let key = (repo.full_name.clone(), path.clone());
                    let has = if let Some(v) = file_cache.get(&key) {
                        *v
                    } else {
                        let exists = provider
                            .get_file(&repo, path, &repo.default_branch)
                            .await?
                            .is_some();
                        file_cache.insert(key, exists);
                        exists
                    };
                    if !has {
                        continue 'repo;
                    }
                }
                FilterConfig::Topic(topic) => {
                    if !repo.topics.iter().any(|t| t == topic) {
                        continue 'repo;
                    }
                }
                FilterConfig::Visibility(vis) => {
                    if &repo.visibility != vis {
                        continue 'repo;
                    }
                }
            }
        }
        result.push(repo);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Visibility;
    use crate::testing::MockProvider;
    use std::collections::HashMap;

    fn make_repo(full_name: &str, archived: bool) -> Repo {
        let parts: Vec<&str> = full_name.splitn(2, '/').collect();
        Repo {
            owner: parts[0].to_string(),
            name: parts.get(1).unwrap_or(&"").to_string(),
            full_name: full_name.to_string(),
            default_branch: "main".to_string(),
            archived,
            visibility: Visibility::Private,
            topics: vec![],
            language: None,
            custom_properties: HashMap::new(),
        }
    }

    fn targets(repos: &str) -> TargetConfig {
        TargetConfig {
            repos: repos.to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![],
        }
    }

    #[test]
    fn include_matching() {
        let repos = vec![
            make_repo("org/alpha", false),
            make_repo("org/beta", false),
            make_repo("other/gamma", false),
        ];

        let result = filter_repos_basic(&repos, &targets("org/*")).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|r| r.owner == "org"));
    }

    #[test]
    fn exclude_filtering() {
        let repos = vec![
            make_repo("org/alpha", false),
            make_repo("org/beta", false),
            make_repo("org/alpha-fork", false),
        ];

        let t = TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec!["org/alpha".to_string()],
            exclude_archived: true,
            filters: vec![],
        };

        let result = filter_repos_basic(&repos, &t).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|r| r.full_name != "org/alpha"));
    }

    #[test]
    fn archived_filtering() {
        let repos = vec![make_repo("org/active", false), make_repo("org/old", true)];

        let result = filter_repos_basic(&repos, &targets("org/*")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].full_name, "org/active");
    }

    #[test]
    fn archived_included_when_disabled() {
        let repos = vec![make_repo("org/active", false), make_repo("org/old", true)];

        let t = TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec![],
            exclude_archived: false,
            filters: vec![],
        };

        let result = filter_repos_basic(&repos, &t).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn combined_filters() {
        let repos = vec![
            make_repo("org/keep", false),
            make_repo("org/exclude-me", false),
            make_repo("org/archived", true),
            make_repo("other/nope", false),
        ];

        let t = TargetConfig {
            repos: "org/*".to_string(),
            exclude: vec!["org/exclude-*".to_string()],
            exclude_archived: true,
            filters: vec![],
        };

        let result = filter_repos_basic(&repos, &t).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].full_name, "org/keep");
    }

    #[test]
    fn invalid_glob_error() {
        let repos = vec![make_repo("org/a", false)];
        let t = TargetConfig {
            repos: "[invalid".to_string(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![],
        };

        let err = filter_repos_basic(&repos, &t).unwrap_err();
        assert!(err.to_string().contains("invalid include glob"));
    }

    #[tokio::test]
    async fn topic_filter() {
        let mut a = make_repo("org/a", false);
        a.topics = vec!["security".into(), "compliance".into()];
        let b = make_repo("org/b", false);

        let provider = MockProvider::new(vec![]);
        let t = TargetConfig {
            repos: "org/*".into(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![FilterConfig::Topic("security".into())],
        };

        let result = filter_repos(&[a, b], &t, &provider).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].full_name, "org/a");
    }

    #[tokio::test]
    async fn visibility_filter() {
        let mut pub_repo = make_repo("org/pub", false);
        pub_repo.visibility = Visibility::Public;
        let priv_repo = make_repo("org/priv", false);

        let provider = MockProvider::new(vec![]);
        let t = TargetConfig {
            repos: "org/*".into(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![FilterConfig::Visibility(Visibility::Public)],
        };

        let result = filter_repos(&[pub_repo, priv_repo], &t, &provider)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].full_name, "org/pub");
    }

    #[tokio::test]
    async fn has_file_filter_uses_cache() {
        let a = make_repo("org/a", false);
        let b = make_repo("org/b", false);
        let provider =
            MockProvider::new(vec![a.clone(), b.clone()]).with_file("org/a:Dockerfile", "FROM x");

        let t = TargetConfig {
            repos: "org/*".into(),
            exclude: vec![],
            exclude_archived: true,
            filters: vec![
                FilterConfig::HasFile("Dockerfile".into()),
                // Same path again — should be served from cache.
                FilterConfig::HasFile("Dockerfile".into()),
            ],
        };

        let result = filter_repos(&[a, b], &t, &provider).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].full_name, "org/a");
    }
}
