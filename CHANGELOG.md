# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.2] - 2026-07-07

### Fixed
- `!branch_protection`: the PUT request to GitHub always omitted any field a policy didn't explicitly set. GitHub's branch protection endpoint requires `required_status_checks`, `enforce_admins`, `required_pull_request_reviews`, and `restrictions` to all be present in the request body (nullable, but not omittable) — so every `apply` for this rule was rejected outright with a validation error. Now fetches the branch's current protection first and fills in anything the policy doesn't set from that, so unrelated settings survive instead of the whole request failing. Note: `restrictions` (push access restrictions) isn't modeled by multipush yet and is always sent as `null` — applying `!branch_protection` to a branch with restrictions configured outside of multipush will clear them.
- GitHub API error messages were reduced to a useless "GitHub" (e.g. `provider error: GitHub`) in every failure report, because `octocrab::Error`'s default `Display` doesn't include the response body. Errors now surface the actual message from GitHub (permission denied, validation failure, etc.) instead.

## [0.4.1] - 2026-07-07

### Fixed
- `custom_property` filter: fall back to fetching each repo's custom properties individually when the org-wide batch endpoint is rejected for lacking the organization-level "Custom properties" permission — a fine-grained token with only the *repository*-level "Custom properties" permission now works instead of the filter silently matching zero repos.
- Custom-properties fetch failures are now logged at `error` (visible at default verbosity) instead of `warn` (only visible with `-v`), since a failure here means a `custom_property` filter can silently match nothing.

## [0.4.0] - 2026-07-07

### Added
- `custom_property` filter (`!custom_property`): target repos by [GitHub custom property](https://docs.github.com/en/organizations/managing-organization-settings/managing-custom-properties-for-repositories-in-your-organization) key/value, with an optional `negate` flag to select repos where the property does *not* match (including where it's unset). Property values are fetched in one batched, paginated call per org.

## [0.3.0] - 2026-06-11

### Added
- `ensure_autolink` rule (`!ensure_autolink`): ensure a GitHub autolink reference exists on a repo — e.g. link `JIRA-123` references in issues, PRs, and commits to an external tracker such as Jira. Remediated via direct GitHub API calls (no PR); reconciliation is keyed by `key_prefix` and replaces drifted links in place. Reported under a new *Autolink updates* section in the table and markdown formatters.

## [0.1.0] - 2026-05-28

### Added
- Initial release.
- `check` command audits an org against declarative YAML policies via the GitHub API (no cloning).
- `apply` command opens one PR per non-compliant repo with deterministic branch names (`multipush/{policy}`); re-running updates the existing PR in place.
- Built-in rules: `EnsureFile`, `FileAbsent`, `EnsureJsonKey`, repo settings, branch protection.
- Built-in recipes: CODEOWNERS, SECURITY.md, LICENSE, .editorconfig, .gitignore, Dependabot.
- Output formatters: table, JSON, markdown, SARIF.
- `list-repos` command for target preview.
- `validate` command for policy schema checks.

[Unreleased]: https://github.com/JackuB/multipush/compare/v0.4.2...HEAD
[0.4.2]: https://github.com/JackuB/multipush/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/JackuB/multipush/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/JackuB/multipush/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/JackuB/multipush/compare/v0.1.0...v0.3.0
[0.1.0]: https://github.com/JackuB/multipush/releases/tag/v0.1.0
