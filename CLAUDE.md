# multipush

Declarative policy-as-code for repository governance. Written in Rust.

## What this tool does

Users declare desired repository state in YAML policies. multipush evaluates
those policies against real repositories via API (no cloning), reports
compliance, and can open PRs to fix violations.

## Architecture

Cargo workspace with 5 crates:

- multipush-core: Traits (Provider, Rule, Formatter), config types, engine
- multipush-provider-github: GitHub API implementation via octocrab
- multipush-rules: Built-in rule implementations (EnsureFile, EnsureJsonKey, etc.)
- multipush-formatters: Output formatters (table, JSON, markdown, SARIF)
- multipush-cli: Binary entry point, clap CLI, registry wiring

## Key Design Principles

1. API-first: Check mode never clones. Uses GitHub Contents API.
2. Compiled-in plugins: No dynamic loading. New rules/providers = implement trait + register.
3. Registry pattern: registry.rs in CLI crate wires all implementations at compile time.
4. Composable config: Multiple YAML files merge additively.
5. Idempotent apply: Running apply twice does not create duplicate PRs.

## Code Conventions

- anyhow::Result in CLI/binary code, thiserror in library crates
- async_trait for async traits (until Rust AFIT stabilizes)
- tracing for logging (never println! or eprintln!)
- All provider methods take &self (providers are Send + Sync)
- Rule evaluation must be safe to call concurrently for different repos
- globset for pattern matching (not glob)
- serde_yml for YAML (not serde_yaml which is deprecated)

## Testing

- Unit tests next to code (#[cfg(test)] mod tests)
- Integration tests in tests/
- wiremock for mocking GitHub API
- Fixtures in tests/fixtures/

## Required after every piece of work

Before reporting a task as done, run all of the following from the workspace
root and confirm each passes clean. CI enforces these — failures here will
break the build.

    cargo fmt --all --check
    cargo clippy --workspace -- -D warnings
    cargo test --workspace

If `cargo fmt --all --check` reports diffs, run `cargo fmt --all` to fix them,
then re-run the check. Do not skip this step, even for one-line edits — the
fmt check runs in CI on every commit.

## Build and Run

    cargo build --workspace
    cargo test --workspace
    cargo clippy --workspace -- -D warnings
    cargo fmt --all --check
    cargo run -p multipush-cli -- check -c examples/basic.yml
    cargo run -p multipush-cli -- apply --dry-run -c examples/basic.yml
