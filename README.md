# multipush

*Leeloo Dallas, multipush.*

Declarative policy-as-code for repository governance. Define what your repos should look like in YAML, and multipush checks compliance and opens PRs to fix violations — all through the API, no cloning required.

## What it's for

Once an organization has more than a handful of repositories, keeping them consistent becomes its own job. Every repo should have a CODEOWNERS file. Every Node project should declare a license. Every public repo needs a SECURITY.md. Every default branch should require reviews. Multiplied across hundreds of repos and dozens of teams, this drift is everywhere — and nobody owns it.

multipush is the tool for that work. You write your standards as YAML policies once. multipush evaluates them against every repo in your org via API, reports what's out of compliance, and — when you're ready — opens PRs that fix the gaps. The same config in CI gives you a continuously enforced baseline.

## What you can do with it

- **Audit an org for compliance.** Read-only `check` produces a per-repo report in table, JSON, markdown, or SARIF. Wire it into CI and treat drift as a build failure.
- **Roll out a new standard.** Declare the desired file, key, or repo setting; `apply` opens one PR per repo with the fix.
- **Govern repo settings, not just files.** Toggle `has_wiki`, enforce squash-merge, require branch protection — through the same policy file.
- **Stay idempotent.** Branch names are deterministic (`multipush/{policy}`). Re-running `apply` updates the existing PR in place — no force-pushes, no duplicates, no lost review comments.
- **Bootstrap fast with recipes.** Built-in templates for CODEOWNERS, SECURITY.md, LICENSE, .editorconfig, .gitignore, Dependabot.

## When to reach for it

multipush fits when:

- You manage many repos and the policies you care about can be expressed declaratively — "this file should exist", "this JSON key should equal X", "this setting should be on".
- You want a single source of truth for org standards that lives in a repo, gets reviewed in PRs, and runs in CI.
- You want remediation to flow through normal code review (PRs), not silent automation.

It's not the right tool if you need to run arbitrary scripts across repos, or if your governance needs are fully covered by GitHub's branch protection and rulesets alone.

## How it's best used

The natural workflow is two-phase:

1. **Continuous `check` in CI.** A scheduled job (nightly or weekly) runs `multipush check --fail-on error` against your org and fails loudly when drift appears. The report is your compliance dashboard.
2. **On-demand `apply` for rollout.** When you ship a new policy, run `apply --dry-run` first to see the blast radius, then `apply --max-prs N` to open PRs in waves until you're clean.

Keep policies in a repo, review changes through PRs, and gate them behind `multipush validate` in CI. The same config drives both the auditor and the fixer — there is no second source of truth to drift.

## Quick start

### Install

Pick the option that matches how you'll use multipush:

**As a GitHub Action** — runs in CI, no host install needed. Best fit if your policies live in a repo and your audits are scheduled:

```yaml
- uses: JackuB/multipush@v0
  with:
    config: multipush.yml
    token: ${{ secrets.MULTIPUSH_TOKEN }}
```

See [GitHub Actions](#github-actions) for the full input reference.

**Shell installer** (Linux, macOS, WSL):

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/JackuB/multipush/releases/latest/download/multipush-cli-installer.sh | sh
```

**PowerShell installer** (Windows):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/JackuB/multipush/releases/latest/download/multipush-cli-installer.ps1 | iex"
```

**Prebuilt binaries**: download the archive for your platform from [Releases](https://github.com/JackuB/multipush/releases) and put `multipush` on your `PATH`. Builds are available for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`.

**From source** (requires a Rust toolchain):

```sh
git clone https://github.com/JackuB/multipush
cd multipush
cargo install --path crates/multipush-cli
```

### Create a config

```yaml
# multipush.yml
provider:
  type: github
  org: my-org
  token: ${GITHUB_TOKEN}

policies:
  - name: require-readme
    description: Every repository should have a README
    severity: error
    targets:
      repos: "my-org/*"
    rules:
      - !ensure_file
        path: README.md
```

### Check compliance

```sh
multipush check -c multipush.yml
```

### Fix violations

```sh
# Preview what would happen
multipush apply --dry-run -c multipush.yml

# Create PRs for failing repos
multipush apply -c multipush.yml
```

## Configuration reference

Configuration is YAML. Environment variables are supported with `${VAR}` and `${VAR:-default}` syntax.

### `provider`

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `type` | `github` \| `gitea` | Yes | — | Provider type |
| `org` | string | Yes | — | Organization name |
| `token` | string | Yes | — | API token |
| `base_url` | string | No | — | Custom API base URL (for Gitea or GitHub Enterprise) |

```yaml
provider:
  type: github
  org: ${GITHUB_ORG}
  token: ${GITHUB_TOKEN}
```

### `defaults`

Optional. Sets defaults applied to all policies.

#### `defaults.targets`

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `repos` | string (glob) | Yes | — | Glob pattern for repo matching (e.g. `"my-org/*"`) |
| `exclude` | list of strings | No | `[]` | Glob patterns to exclude |
| `exclude_archived` | bool | No | `true` | Skip archived repos |
| `filters` | list | No | `[]` | Additional filters (see [Filters](#filters)) |

#### `defaults.apply`

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `pr_prefix` | string | No | `"multipush"` | Branch prefix for PRs (branch: `{prefix}/{policy-name}`) |
| `commit_author` | string | No | — | Git commit author name |
| `pr_labels` | list of strings | No | `[]` | Labels to add to PRs |
| `pr_draft` | bool | No | `false` | Create PRs as drafts |
| `existing_pr` | `skip` \| `update` \| `recreate` | No | `update` | Strategy when an open PR is already present on the branch |
| `auto_merge` | bool | No | `false` | Enable GitHub auto-merge on created PRs. Equivalent to `apply --auto-merge`. Requires the repo to allow auto-merge |

```yaml
defaults:
  targets:
    repos: "my-org/*"
    exclude:
      - "my-org/legacy-*"
    exclude_archived: true
  apply:
    pr_prefix: multipush
    pr_labels:
      - automation
      - governance
    pr_draft: false
    existing_pr: update
```

### `policies`

Each policy defines a set of rules to evaluate against target repositories.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `name` | string | Yes | — | Unique policy name |
| `description` | string | No | — | Human-readable description |
| `severity` | `info` \| `warning` \| `error` | No | `error` | Policy severity level |
| `targets` | object | Yes | — | Target repositories (same fields as `defaults.targets`) |
| `rules` | list | Yes | — | Rule definitions |

### `targets`

Each policy (or recipe) must specify a `targets` block. Per-policy targets override defaults.

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `repos` | string or list of strings (globs) | Yes | — | One glob, or several globs OR'd together |
| `exclude` | list of strings | No | `[]` | Glob patterns to exclude |
| `exclude_archived` | bool | No | `true` | Skip archived repos |
| `filters` | list | No | `[]` | Additional filters |

#### Targeting an explicit set of repos

`repos` accepts either a single glob or a list of globs — a repo is included
when **any** glob matches. There are three useful forms:

```yaml
# 1. A single org-wide glob (most common).
targets:
  repos: "acme/*"

# 2. An explicit list. Each entry is itself a glob, so you can mix
#    exact names and wildcards.
targets:
  repos:
    - acme/api-gateway
    - acme/web-frontend
    - acme/worker-*

# 3. Brace-expansion shorthand for a small, fixed set on one line.
targets:
  repos: "acme/{api-gateway,web-frontend,worker-pool}"
```

To carve repos out of a wider match, combine with `exclude`:

```yaml
targets:
  repos: "acme/*"
  exclude:
    - "acme/legacy-*"
    - "acme/sandbox"
```

### Filters

> **Note:** Filters are parsed and validated but not yet evaluated at runtime. They will be fully functional in a future release.

Filters use YAML tags to specify their type:

```yaml
targets:
  repos: "my-org/*"
  filters:
    - !has_file package.json
    - !topic nodejs
    - !visibility public
```

| Filter | Argument | Description |
|---|---|---|
| `!has_file` | file path | Only repos containing this file |
| `!topic` | topic name | Only repos with this GitHub topic |
| `!visibility` | `public` \| `private` \| `internal` | Only repos with this visibility |

## Rules

Rules use YAML tags to specify their type. Each rule is prefixed with `!` in the config.

| Rule | Tag | Remediates via | Description |
|---|---|---|---|
| ensure_file | `!ensure_file` | PR | Ensure a file exists (in any of several locations), optionally satisfying a content predicate |
| ensure_json_key | `!ensure_json_key` | PR | Ensure a key exists in a JSON file |
| ensure_yaml_key | `!ensure_yaml_key` | PR | Ensure a key exists in a YAML file |
| file_matches | `!file_matches` | — (report only) | Check file content against a regex pattern |
| file_absent | `!file_absent` | PR | Ensure a file does **not** exist; remediates by deleting it |
| repo_settings | `!repo_settings` | Direct PATCH | Enforce repository-level toggles (issues, wiki, merge strategies, auto-delete on merge, …) |
| branch_protection | `!branch_protection` | Direct PATCH | Enforce branch protection on a specific branch (defaults to the repo's default branch) |

Rules whose remediation type is "Direct PATCH" don't open PRs — multipush calls the GitHub API directly during `apply`. The corresponding rows show up in the table's *Repo settings updates* and *Branch protection updates* sections rather than the PR table.

### `!ensure_file`

`ensure_file` separates **where** the file may live, **what to create** when it
is missing, and **what an existing file must satisfy**.

| Param | Type | Required | Description |
|---|---|---|---|
| `path` | string | One of `path`/`paths` | Single file path to check |
| `paths` | list of strings | One of `path`/`paths` | Candidate paths, checked in priority order (any-of); first entry is the canonical creation location |
| `default_content` | string | No | Body written when the file is **missing**. Only seeds creation — never enforced on existing files |
| `must_contain` | string | No | Predicate: an existing file must contain this substring |
| `must_match` | string (regex) | No | Predicate: an existing file must match this regular expression |
| `must_equal` | string | No | Predicate: an existing file must equal this exactly (drift is overwritten); also used as the creation body |

At most one predicate (`must_contain` / `must_match` / `must_equal`) may be set.
With no predicate, the file only has to exist. `default_content` is redundant
with `must_equal` (which governs creation itself), so setting both is rejected.

Empty or whitespace-only values for `default_content` / `must_contain` /
`must_match` / `must_equal` are normalized to "unset" — recipe params that
expand to an empty string don't silently turn the field off (`must_contain: ""`
would otherwise match every file).

`default_content` is also normalized to end with **exactly one trailing
newline**. You can omit `\n` in configs and recipe params, and multipush will
add it; doubled newlines collapse to one; recipe substitution that folds a
literal `\n` into a space (a YAML quirk inside double-quoted strings) is
corrected. `must_equal` is left verbatim so the equality predicate stays
honest — mutating it would loop forever.

```yaml
# Simplest: the file just has to exist; create it from default_content if absent.
- !ensure_file
  path: CODEOWNERS
  default_content: "* @platform-team"
```

**Multiple locations.** GitHub accepts some files (CODEOWNERS, LICENSE) in more
than one place. List them in `paths`; the rule passes if the file exists at
**any** of them, and creates the first entry when none exist.

```yaml
- !ensure_file
  paths:
    - CODEOWNERS
    - .github/CODEOWNERS
    - docs/CODEOWNERS
  default_content: "* @platform-team"
```

**Discovery without auto-fix.** Omit `default_content` (and `must_equal`) to
get a pure existence check. multipush flags every repo missing the file but
won't open PRs — useful as a baseline layer alongside team-specific policies
that know what content to write. Failing rows surface as `Report only` in the
apply table and don't consume `--max-prs` budget.

```yaml
# Baseline: every repo must have CODEOWNERS somewhere. No content known.
- !ensure_file
  paths:
    - CODEOWNERS
    - .github/CODEOWNERS
    - docs/CODEOWNERS
```

**Content predicate plus a valid created file.** Because the check
(`must_contain`) and the creation body (`default_content`) are separate fields,
you can require a marker *and* still create a complete, valid file when the file
is absent. An existing file that fails the predicate is reported with no
auto-fix (multipush won't rewrite a hand-authored file).

```yaml
# Infra repos must list @acme/ops as an owner.
- !ensure_file
  paths:
    - CODEOWNERS
    - .github/CODEOWNERS
    - docs/CODEOWNERS
  default_content: "* @acme/ops"     # created when the file is missing
  must_contain: "@acme/ops"          # existing files must mention ops somewhere
```

### `!ensure_json_key`

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | Yes | — | JSON file path |
| `key` | string | Yes | — | Dot-separated key path (e.g. `a.b.c`) |
| `value` | any | No | — | Expected value |
| `mode` | `check_only` \| `enforce` | No | `check_only` | Whether to remediate |

```yaml
- !ensure_json_key
  path: package.json
  key: engines.node
  value: ">=18"
  mode: enforce
```

### `!ensure_yaml_key`

Same parameters as `!ensure_json_key`, but for YAML files.

```yaml
- !ensure_yaml_key
  path: .github/settings.yml
  key: repository.allow_squash_merge
  value: true
  mode: enforce
```

### `!file_matches`

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | Yes | — | File path to check |
| `pattern` | string | Yes | — | Regex pattern to match |

```yaml
- !file_matches
  path: README.md
  pattern: "^# .+"
```

### `!file_absent`

The mirror of `ensure_file`: assert that a file is **gone**. When the file
exists, remediation is a delete-file PR.

| Param | Type | Required | Default | Description |
|---|---|---|---|---|
| `path` | string | Yes | — | File path that must not exist |

```yaml
# Drop the legacy build config.
- !file_absent
  path: .travis.yml
```

### `!repo_settings`

Enforce repository-level toggles. Each set field becomes part of a single
`PATCH /repos/{owner}/{repo}` call during `apply` — no PR is opened. Unset
fields are left alone. The `apply` table reports these under *Repo settings
updates*.

| Param | Type | Description |
|---|---|---|
| `has_issues` | bool | Enable/disable the Issues tab |
| `has_wiki` | bool | Enable/disable the Wiki tab |
| `has_projects` | bool | Enable/disable Projects |
| `allow_merge_commit` | bool | Allow standard merge commits |
| `allow_squash_merge` | bool | Allow squash merge |
| `allow_rebase_merge` | bool | Allow rebase merge |
| `delete_branch_on_merge` | bool | Auto-delete head branches after merge |
| `allow_auto_merge` | bool | Allow GitHub's auto-merge button |
| `default_branch` | string | Rename the default branch (rare; doesn't migrate refs) |

```yaml
# Tidy up branch lists across the org.
- !repo_settings
  delete_branch_on_merge: true
  allow_auto_merge: true
```

### `!branch_protection`

Enforce branch protection on a single branch. Like `!repo_settings`, this
remediates via a direct API PATCH (no PR). The `apply` table reports these
under *Branch protection updates*.

| Param | Type | Description |
|---|---|---|
| `branch` | string | Branch to protect. Defaults to the repo's default branch when omitted |
| `required_status_checks` | object | `{strict: bool, contexts: [string]}` |
| `required_pull_request_reviews` | object | `{required_approving_review_count: int, dismiss_stale_reviews: bool, require_code_owner_reviews: bool}` |
| `enforce_admins` | bool | Include admins in the protection rules |
| `required_linear_history` | bool | Require a linear history (no merge commits) |
| `allow_force_pushes` | bool | Permit force-pushes |
| `allow_deletions` | bool | Permit branch deletion |

```yaml
- !branch_protection
  required_pull_request_reviews:
    required_approving_review_count: 1
    require_code_owner_reviews: true
  enforce_admins: true
  required_linear_history: true
```

## Recipes

Recipes are reusable policy templates with configurable parameters. Use them in policies with the `recipe:` field.

| Recipe | Description | Required Params | Optional Params |
|---|---|---|---|
| `codeowners` | Ensure CODEOWNERS file | — | `default_content`, `must_contain` |
| `security-md` | Ensure SECURITY.md | `contact_email` | — |
| `license` | Ensure LICENSE file | — | `license_type` (default: `MIT`), `author` |
| `editorconfig` | Ensure .editorconfig | — | `indent_style` (default: `space`), `indent_size` (default: `2`) |
| `gitignore` | Ensure .gitignore | `template` | — |
| `dependabot` | Ensure Dependabot config | `ecosystem` | `schedule` (default: `weekly`) |

### Recipe syntax

```yaml
policies:
  - recipe: codeowners
    params:
      default_content: "* @platform-team"
      must_contain: "@platform-team"
    targets:
      repos: "my-org/*"
```

Recipes expand into regular rules at load time. You can override `name`, `description`, `severity`, and `targets` on a recipe policy.

### Layering recipes

`codeowners` is designed to layer: omit both params for a pure discovery
check, set `default_content` for auto-create, and set both to enforce that
existing files mention a particular team. A typical org policy file uses
one baseline that flags every missing CODEOWNERS, plus per-team policies
that auto-create the right content for the repos they own.

```yaml
policies:
  # 1. Baseline: every repo needs a CODEOWNERS *somewhere*. No content known
  #    here, so this layer only reports — no PRs opened, no max-prs budget
  #    consumed.
  - name: codeowners-everywhere
    severity: error
    targets:
      repos: "acme/*"
    recipe: codeowners

  # 2. Per-team: knows the content. Auto-creates missing files and FAILs any
  #    existing file that doesn't mention the team.
  - name: codeowners-portal
    severity: error
    targets:
      repos:
        - acme/portal
        - acme/portal-*
    recipe: codeowners
    params:
      default_content: "* @acme/portal"
      must_contain: "@acme/portal"
```

The recipe checks all three GitHub-honored locations (`CODEOWNERS`,
`.github/CODEOWNERS`, `docs/CODEOWNERS`); new files are created at the repo
root.

### Parameter values

| Recipe | Param | Accepted values |
|---|---|---|
| `license` | `license_type` | `MIT`, `Apache-2.0` |
| `gitignore` | `template` | `node`, `rust`, `python`, `java`, `go` |
| `dependabot` | `ecosystem` | `npm`, `cargo`, `pip`, `maven`, `gomod` |
| `dependabot` | `schedule` | `daily`, `weekly`, `monthly` |
| `editorconfig` | `indent_style` | `space`, `tab` |

## CLI commands

### `check`

Evaluate policies and report compliance (read-only).

```sh
multipush check -c config.yml
multipush check -c config.yml -f markdown --fail-on warning
multipush check -c config.yml -p require-readme -p require-license
multipush check -c config.yml --by-repo
```

| Flag | Description | Default |
|---|---|---|
| `-c, --config` | Config file or directory (repeatable) | auto-discovery |
| `-f, --format` | Output format (`table`, `markdown`) | `table` |
| `-p, --policy` | Run only named policies (repeatable) | all |
| `--by-repo` | Group findings by repository instead of by policy | — |
| `--fail-on` | Exit 1 if any result >= severity | `error` |
| `--concurrency` | Max concurrent repo evaluations | `10` |
| `--no-color` | Disable colors | — |
| `-v` | Verbosity (`-v` info, `-vv` debug, `-vvv` trace) | errors only |
| `-q, --quiet` | Suppress non-error output | — |

#### `--by-repo` output

The default view groups findings by policy (one table per policy). With
`--by-repo`, the same results are pivoted so each repository gets a single
block listing every policy that touched it. Useful when you want to answer
*"what's wrong with **this repo**?"* rather than *"who fails **this rule**?"*

```
acme/portal  — 2 policies (2 pass)
  ✓ codeowners-everywhere  File .github/CODEOWNERS exists
  ✓ codeowners-portal      File .github/CODEOWNERS contains required content

acme/portal-tennable-test  — 2 policies (2 fail)
  ✗ codeowners-everywhere  No file found at any of: CODEOWNERS, .github/CODEOWNERS
  ✗ codeowners-portal      No file found at any of: CODEOWNERS, .github/CODEOWNERS

Overview
────────
Policies:     2
Repositories: 2
Pass:         2
Fail:         2
Skip:         0
Error:        0
Success rate: 50.0%  (2 pass / 4 evaluated)
```

Glyphs: `✓` pass, `✗` fail, `•` skip, `!` error.

### `apply`

Apply remediations by creating/updating PRs.

```sh
multipush apply --dry-run -c config.yml
multipush apply -c config.yml --max-prs 5
```

| Flag | Description | Default |
|---|---|---|
| `-c, --config` | Config file or directory (repeatable) | auto-discovery |
| `--dry-run` | Preview changes without creating PRs | — |
| `--max-prs` | Max PRs to create | `10` |
| `-f, --format` | Output format | `table` |
| `-p, --policy` | Run only named policies (repeatable) | all |
| `--concurrency` | Max concurrent repo evaluations | `10` |
| `--auto-merge` | Enable auto-merge on created PRs (shortcut for `defaults.apply.auto_merge: true`) | — |
| `--policy-source-url` | URL of the repo where the policy YAML lives; inlined into PR bodies under *Source*. Defaults to auto-detection from `GITHUB_*` env vars when running in Actions | auto |
| `--fail-on` | Exit 1 if any result >= severity | `error` |
| `--no-color` | Disable colors | — |
| `-v` | Verbosity | errors only |
| `-q, --quiet` | Suppress non-error output | — |

#### PR body shape

PRs opened by `apply` follow a fixed shape:

```markdown
## Policy: codeowners-portal

Repos belonging to the Portal team

**Severity:** error

### Changes

- Create file .github/CODEOWNERS *(rule: `ensure_file`)*
  - `.github/CODEOWNERS` (create/update)

### Source

Policy defined in [acme/policies @ 992aaab](https://github.com/acme/policies/tree/992aaab123)
Opened by [workflow run (policy-enforcement)](https://github.com/acme/policies/actions/runs/8123456789)

---
*Created by [multipush](https://github.com/JackuB/multipush) — declarative policy-as-code for GitHub repos.*
```

The **Source** block is auto-populated when multipush detects it's running
under GitHub Actions (`GITHUB_ACTIONS=true`). It reads `GITHUB_SERVER_URL`,
`GITHUB_REPOSITORY`, `GITHUB_SHA`, `GITHUB_RUN_ID`, and `GITHUB_WORKFLOW` to
link reviewers back to the exact commit of the policy repo and the workflow
run that opened the PR. The block is suppressed entirely when no provenance
is available.

For runs outside Actions (cron, self-hosted, ad-hoc), pass
`--policy-source-url https://example.com/your/policies` to populate the link
manually.

The target repo's name is **not** repeated in the body — GitHub already
shows it in the PR header.

#### Handling existing branches and closed PRs

`apply` keys PRs by branch name (`{pr_prefix}/{policy-name}`, default
`multipush/{policy-name}`). When you re-run after closing a PR:

- If a PR is **open** on the branch, `defaults.apply.existing_pr` controls
  the behavior (`update` is the default).
- If a PR is **closed** but the branch still exists, multipush reuses the
  branch, pushes any new commits needed, and opens a fresh PR.
- If the branch is also gone, multipush creates everything from the base
  SHA.

This is idempotent: re-running with no policy changes either updates the
existing PR in place or no-ops. Reviewers don't lose comment history.

### `validate`

Validate config without connecting to any provider.

```sh
multipush validate -c config.yml
multipush validate -c dir/multipush.yml -c dir/policies/
```

### `list-rules`

List available rules and recipes.

```sh
multipush list-rules
multipush list-rules -v    # show recipe parameters
multipush list-rules -q    # names only
```

## Multi-file configuration

Split config across multiple files for better organization. The CLI `-c` flag accepts files or directories and merges them:

```sh
multipush check -c config/multipush.yml -c config/policies/
```

Merging behavior:
- Mappings merge deeply (later values override)
- `policies` arrays concatenate across files
- Duplicate policy names: last definition wins (with a warning)

### Auto-discovery

Without `-c`, multipush looks for config automatically:

1. `~/.config/multipush/config.yml` (global defaults)
2. `.multipush/multipush.yml` (project config)
3. `.multipush/policies/` (policy directory)

## GitHub Actions

The published action wraps the CLI so workflows don't have to install it. It works on `ubuntu-*`, `macos-*`, and `windows-*` runners.

### Recommended pattern: a central policy repo

The cleanest deployment is a single repo dedicated to org-wide policy — call it `policies`, `cml`, `repo-governance`, whatever. It holds the YAML configs and the workflows that run multipush. It contains no product code; its only job is to declare standards and audit/remediate every other repo against them.

```
policies/
├── configs/
│   ├── codeowners.yml
│   ├── licensing.yml
│   └── security-md.yml
└── .github/workflows/
    ├── check.yml      # scheduled audit + PR validation of policy changes
    └── apply.yml      # workflow_dispatch remediation
```

Why this works in practice:

- **Policy changes go through PR review in one place.** `check` runs on the PR against live org repos, so a malformed or overly broad policy never lands on `main`.
- **Apply is one button, scoped to one team.** Only people with write access to the policy repo can dispatch remediation; the rest of the org just sees the resulting PRs.
- **The audit history lives somewhere.** Run summaries on `check` form a compliance trail you can point auditors at.
- **No install anywhere.** The action pulls the binary into the runner per-run; nothing to maintain on dev machines or build images.

The examples below assume this layout.

### Minimal check

```yaml
name: Policy Check
on:
  schedule:
    - cron: "0 8 * * 1"  # Monday 8am UTC
  workflow_dispatch:

permissions:
  contents: read

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: JackuB/multipush@v0
        with:
          config: multipush.yml
          token: ${{ secrets.MULTIPUSH_TOKEN }}
```

### Action inputs

| Input | Required | Default | Description |
|---|---|---|---|
| `config` | yes | — | Path to a multipush config file or directory (passed as `-c`) |
| `command` | no | `check` | Subcommand: `check`, `apply`, `validate`, `list-repos`, `list-rules` |
| `args` | no | `""` | Extra arguments appended to the command (e.g. `--dry-run --max-prs 5`) |
| `token` | no | `${{ github.token }}` | Token used by multipush. Override for org-wide access |
| `version` | no | `latest` | multipush release to install (e.g. `0.2.2`) |

The action exposes `GITHUB_REPOSITORY`, `GITHUB_SHA`, `GITHUB_RUN_ID`, and
`GITHUB_WORKFLOW` to multipush, so PRs opened from a workflow automatically
link back to the policy repo at the commit that ran them. See
[PR body shape](#pr-body-shape).

### Version pinning

Three pin styles, in increasing strictness:

- **`@v0`** (major) — moves on every non-prerelease publish. Accepts new features and fixes, but breaking changes between minors of `0.x` will propagate. Lowest-maintenance, highest risk during the `0.x` series.
- **`@v0.1`** (minor) — moves on patches within `0.1.x`. Safe target for production policy repos that don't want minor-version churn.
- **`@v0.1.0`** (exact) — immutable. Reproducible, zero surprises, but you opt in to every update by hand.

The major and minor tags are force-updated automatically by `update-major-tag.yml` whenever a non-prerelease GitHub Release is published. You don't need to maintain them yourself.

### Token scope

The action defaults `token:` to the workflow's `GITHUB_TOKEN`, which is **scoped to the current repo only**. To audit or fix repos across an org, override `token:` with a Personal Access Token or GitHub App installation token that has:

- `repo` (or fine-grained: Contents read/write, Pull requests write, Administration read for branch protection)
- `read:org`

Store it as `secrets.MULTIPUSH_TOKEN` (or any name) and reference it via the `token:` input.

### Apply on demand

A `workflow_dispatch` job that defaults to dry-run and lets you flip a switch to actually open PRs:

```yaml
name: multipush apply
on:
  workflow_dispatch:
    inputs:
      dry_run:
        description: Preview without opening PRs
        type: boolean
        default: true

permissions:
  contents: read

jobs:
  apply:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: JackuB/multipush@v0
        with:
          config: multipush.yml
          command: apply
          args: ${{ inputs.dry_run && '--dry-run' || '--max-prs 5' }}
          token: ${{ secrets.MULTIPUSH_TOKEN }}
```

### Calling the binary directly

If you need to pipe output, set non-default env, or use a flag the action doesn't surface, install the binary yourself:

```yaml
- name: Install multipush
  run: |
    curl --proto '=https' --tlsv1.2 -LsSf \
      https://github.com/JackuB/multipush/releases/latest/download/multipush-cli-installer.sh | sh
    echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"

- name: Run multipush
  env:
    GITHUB_TOKEN: ${{ secrets.MULTIPUSH_TOKEN }}
  run: |
    multipush check -c multipush.yml -f markdown > report.md
```

## License

MIT
