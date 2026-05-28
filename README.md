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

```sh
cargo install multipush-cli
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
| `existing_pr` | `skip` \| `update` \| `recreate` | No | `update` | Strategy when a PR already exists |

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
| `repos` | string (glob) | Yes | — | Glob pattern for repo matching |
| `exclude` | list of strings | No | `[]` | Glob patterns to exclude |
| `exclude_archived` | bool | No | `true` | Skip archived repos |
| `filters` | list | No | `[]` | Additional filters |

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

| Rule | Tag | Description |
|---|---|---|
| ensure_file | `!ensure_file` | Ensure a file exists (in any of several locations), optionally satisfying a content predicate |
| ensure_json_key | `!ensure_json_key` | Ensure a key exists in a JSON file |
| ensure_yaml_key | `!ensure_yaml_key` | Ensure a key exists in a YAML file |
| file_matches | `!file_matches` | Check file content against a regex pattern |

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

```yaml
# Simplest: the file just has to exist; create it from default_content if absent.
- !ensure_file
  path: CODEOWNERS
  default_content: "* @platform-team\n"
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
  default_content: "* @platform-team\n"
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
  default_content: "* @acme/ops\n"   # created when the file is missing
  must_contain: "@acme/ops"           # existing files must mention ops somewhere
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

## Recipes

Recipes are reusable policy templates with configurable parameters. Use them in policies with the `recipe:` field.

| Recipe | Description | Required Params | Optional Params |
|---|---|---|---|
| `codeowners` | Ensure CODEOWNERS file | `default_owner` | `mode` (default: `create_if_missing`) |
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
      default_owner: "@platform-team"
    targets:
      repos: "my-org/*"
```

Recipes expand into regular rules at load time. You can override `name`, `description`, `severity`, and `targets` on a recipe policy.

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
```

| Flag | Description | Default |
|---|---|---|
| `-c, --config` | Config file or directory (repeatable) | auto-discovery |
| `-f, --format` | Output format (`table`, `markdown`) | `table` |
| `-p, --policy` | Run only named policies (repeatable) | all |
| `--fail-on` | Exit 1 if any result >= severity | `error` |
| `--concurrency` | Max concurrent repo evaluations | `10` |
| `--no-color` | Disable colors | — |
| `-v` | Verbosity (`-v` info, `-vv` debug, `-vvv` trace) | errors only |
| `-q, --quiet` | Suppress non-error output | — |

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
| `--fail-on` | Exit 1 if any result >= severity | `error` |
| `--no-color` | Disable colors | — |
| `-v` | Verbosity | errors only |
| `-q, --quiet` | Suppress non-error output | — |

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

      - name: Install multipush
        run: cargo install multipush-cli

      - name: Check policies
        env:
          GITHUB_TOKEN: ${{ secrets.MULTIPUSH_TOKEN }}
          GITHUB_ORG: my-org
        run: multipush check -c multipush.yml

      - name: Apply remediations
        if: github.event_name == 'workflow_dispatch'
        env:
          GITHUB_TOKEN: ${{ secrets.MULTIPUSH_TOKEN }}
          GITHUB_ORG: my-org
        run: multipush apply -c multipush.yml
```

## License

MIT
