use globset::{Glob, GlobSetBuilder};
use tracing::warn;

use crate::config::{FilterConfig, TargetConfig};
use crate::error::CoreError;
use crate::model::Repo;
use crate::Result;

pub fn filter_repos(repos: &[Repo], targets: &TargetConfig) -> Result<Vec<Repo>> {
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

    let result: Vec<Repo> = repos
        .iter()
        .filter(|r| include.is_match(&r.full_name))
        .filter(|r| !exclude.is_match(&r.full_name))
        .filter(|r| {
            if targets.exclude_archived && r.archived {
                return false;
            }
            true
        })
        .cloned()
        .collect();

    for filter in &targets.filters {
        match filter {
            FilterConfig::HasFile(path) => {
                warn!(filter = %path, "has_file filter not yet implemented, skipping");
            }
            FilterConfig::Topic(topic) => {
                warn!(filter = %topic, "topic filter not yet implemented, skipping");
            }
            FilterConfig::Visibility(vis) => {
                warn!(filter = ?vis, "visibility filter not yet implemented, skipping");
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Visibility;
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

        let result = filter_repos(&repos, &targets("org/*")).unwrap();
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

        let result = filter_repos(&repos, &t).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|r| r.full_name != "org/alpha"));
    }

    #[test]
    fn archived_filtering() {
        let repos = vec![make_repo("org/active", false), make_repo("org/old", true)];

        let result = filter_repos(&repos, &targets("org/*")).unwrap();
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

        let result = filter_repos(&repos, &t).unwrap();
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

        let result = filter_repos(&repos, &t).unwrap();
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

        let err = filter_repos(&repos, &t).unwrap_err();
        assert!(err.to_string().contains("invalid include glob"));
    }
}
