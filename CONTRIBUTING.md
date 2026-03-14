# Contributing to multipush

## Dev setup

1. Install the [Rust toolchain](https://rustup.rs/)
2. Clone the repo and build:

```sh
git clone https://github.com/your-org/multipush.git
cd multipush
cargo build --workspace
```

## Running tests

```sh
cargo test --workspace
```

Integration tests use [wiremock](https://crates.io/crates/wiremock) to mock the GitHub API. Fixtures live in `tests/fixtures/`.

## Linting

```sh
cargo clippy --workspace -- -D warnings
```

## Adding a new rule

1. Create a new file in `crates/multipush-rules/src/` (e.g. `my_rule.rs`)
2. Implement the `Rule` trait from `multipush-core`:

```rust
use async_trait::async_trait;
use multipush_core::rule::{Rule, RuleContext, RuleResult};

pub struct MyRule { /* config fields */ }

#[async_trait]
impl Rule for MyRule {
    fn rule_type(&self) -> &str { "my_rule" }
    fn description(&self) -> String { "Description here".into() }
    async fn evaluate(&self, ctx: &RuleContext<'_>) -> anyhow::Result<RuleResult> {
        // Implementation
    }
}
```

3. Add a variant to `RuleDefinition` in `crates/multipush-core/src/config/rules.rs`
4. Add a config struct with serde `Deserialize` in the same file
5. Register the rule in the `create_rule` function in `crates/multipush-rules/src/lib.rs`
6. Wire it into `crates/multipush-cli/src/registry.rs`

## Adding a new recipe

1. Create a YAML file in `crates/multipush-core/src/recipe/builtins/` (e.g. `my-recipe.yml`)

```yaml
name: my-recipe
description: "What this recipe does"
params:
  my_param:
    type: string
    required: true
    description: "Param description"
rules:
  - !ensure_file
    path: some-file.txt
    content: "{{ my_param }}"
```

2. Add the file to the `include_str!` list in `crates/multipush-core/src/recipe/builtin.rs`

## Adding a new provider

1. Create a new crate (e.g. `multipush-provider-gitea`)
2. Implement the `Provider` trait from `multipush-core`
3. Add the crate as a dependency in `crates/multipush-cli/Cargo.toml`
4. Register the provider in `crates/multipush-cli/src/registry.rs`

## PR guidelines

- Keep PRs focused on a single change
- Add tests for new rules and recipes
- Run `cargo clippy --workspace -- -D warnings` before submitting
- Run `cargo test --workspace` and ensure all tests pass
