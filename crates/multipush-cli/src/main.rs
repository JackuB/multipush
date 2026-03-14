mod registry;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::error;

use multipush_core::config::{load_config, ConfigSource};
use multipush_core::engine::{evaluate, execute};
use multipush_core::formatter::RepoOutcome;
use multipush_core::model::Severity;
use multipush_core::recipe::builtin::builtin_recipes;

#[derive(Parser)]
#[command(name = "multipush", about = "Declarative repository governance")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Evaluate policies and report compliance
    Check {
        /// Config file or directory (repeatable)
        #[arg(short, long)]
        config: Vec<PathBuf>,

        /// Output format
        #[arg(short, long, default_value = "table")]
        format: String,

        /// Run only named policies (repeatable)
        #[arg(short, long)]
        policy: Vec<String>,

        /// Increase verbosity (-v = debug, -vv = trace)
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,

        /// Suppress output except errors
        #[arg(short, long)]
        quiet: bool,

        /// Disable colors
        #[arg(long)]
        no_color: bool,

        /// Exit 1 if any result >= severity
        #[arg(long, default_value = "error")]
        fail_on: Severity,
    },

    /// Validate config without connecting to providers
    Validate {
        /// Config file or directory (repeatable)
        #[arg(short, long)]
        config: Vec<PathBuf>,

        /// Increase verbosity (-v = debug, -vv = trace)
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,

        /// Suppress output except errors
        #[arg(short, long)]
        quiet: bool,
    },

    /// List available rules and recipes
    ListRules {
        /// Increase verbosity for more detail
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,

        /// Show only names
        #[arg(short, long)]
        quiet: bool,
    },

    /// Apply remediations by creating/updating PRs
    Apply {
        /// Config file or directory (repeatable)
        #[arg(short, long)]
        config: Vec<PathBuf>,

        /// Output format
        #[arg(short, long, default_value = "table")]
        format: String,

        /// Run only named policies (repeatable)
        #[arg(short, long)]
        policy: Vec<String>,

        /// Preview changes without creating PRs
        #[arg(long)]
        dry_run: bool,

        /// Maximum number of PRs to create
        #[arg(long, default_value = "10")]
        max_prs: usize,

        /// Increase verbosity (-v = debug, -vv = trace)
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,

        /// Suppress output except errors
        #[arg(short, long)]
        quiet: bool,

        /// Disable colors
        #[arg(long)]
        no_color: bool,

        /// Exit 1 if any result >= severity
        #[arg(long, default_value = "error")]
        fail_on: Severity,
    },
}

fn init_tracing(verbose: u8, quiet: bool) {
    use tracing_subscriber::EnvFilter;

    let level = if quiet {
        "error"
    } else {
        match verbose {
            0 => "warn",
            1 => "debug",
            _ => "trace",
        }
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Check {
            config,
            format,
            policy,
            verbose,
            quiet,
            no_color,
            fail_on,
        } => {
            init_tracing(verbose, quiet);

            match run_check(config, format, policy, no_color, fail_on) {
                Ok(code) => code,
                Err(e) => {
                    error!("{e:#}");
                    ExitCode::from(2)
                }
            }
        }
        Command::ListRules { verbose, quiet } => {
            match run_list_rules(verbose, quiet) {
                Ok(code) => code,
                Err(e) => {
                    error!("{e:#}");
                    ExitCode::from(2)
                }
            }
        }
        Command::Validate {
            config,
            verbose,
            quiet,
        } => {
            init_tracing(verbose, quiet);

            match run_validate(config) {
                Ok(code) => code,
                Err(e) => {
                    error!("{e:#}");
                    ExitCode::from(2)
                }
            }
        }
        Command::Apply {
            config,
            format,
            policy,
            dry_run,
            max_prs,
            verbose,
            quiet,
            no_color,
            fail_on,
        } => {
            init_tracing(verbose, quiet);

            match run_apply(config, format, policy, dry_run, max_prs, no_color, fail_on) {
                Ok(code) => code,
                Err(e) => {
                    error!("{e:#}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

fn paths_to_sources(paths: &[PathBuf]) -> Vec<ConfigSource> {
    paths
        .iter()
        .map(|p| {
            if p.is_dir() {
                ConfigSource::Directory(p.clone())
            } else {
                ConfigSource::FilePath(p.clone())
            }
        })
        .collect()
}

fn run_check(
    config_paths: Vec<PathBuf>,
    format: String,
    policy_filter: Vec<String>,
    no_color: bool,
    fail_on: Severity,
) -> Result<ExitCode> {
    let sources = paths_to_sources(&config_paths);
    let mut config = load_config(&sources).context("failed to load config")?;

    if !policy_filter.is_empty() {
        config.policies.retain(|p| policy_filter.contains(&p.name));
        if config.policies.is_empty() {
            bail!(
                "no policies matched filter: {}",
                policy_filter.join(", ")
            );
        }
    }

    let provider = registry::create_provider(&config.provider)?;
    let formatter = registry::create_formatter(&format, no_color)?;

    let rt = tokio::runtime::Runtime::new()?;
    let report = rt.block_on(evaluate(&config, provider.as_ref(), registry::create_rules))?;

    let output = formatter.format(&report)?;
    if !output.is_empty() {
        println!("{output}");
    }

    let has_failure = report.results.iter().any(|pr| {
        pr.severity >= fail_on
            && pr.repo_results.iter().any(|rr| {
                matches!(
                    rr.outcome,
                    RepoOutcome::Fail { .. } | RepoOutcome::Error { .. }
                )
            })
    });

    if has_failure {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn run_apply(
    config_paths: Vec<PathBuf>,
    format: String,
    policy_filter: Vec<String>,
    dry_run: bool,
    max_prs: usize,
    no_color: bool,
    fail_on: Severity,
) -> Result<ExitCode> {
    let sources = paths_to_sources(&config_paths);
    let mut config = load_config(&sources).context("failed to load config")?;

    if !policy_filter.is_empty() {
        config.policies.retain(|p| policy_filter.contains(&p.name));
        if config.policies.is_empty() {
            bail!(
                "no policies matched filter: {}",
                policy_filter.join(", ")
            );
        }
    }

    let provider = registry::create_provider(&config.provider)?;
    let formatter = registry::create_formatter(&format, no_color)?;

    let rt = tokio::runtime::Runtime::new()?;
    let report = rt.block_on(evaluate(&config, provider.as_ref(), registry::create_rules))?;
    let apply_report = rt.block_on(execute(&report, &config, provider.as_ref(), dry_run, max_prs))?;

    let output = formatter.format_apply(&apply_report)?;
    if !output.is_empty() {
        println!("{output}");
    }

    let has_failure = apply_report.report.results.iter().any(|pr| {
        pr.severity >= fail_on
            && pr.repo_results.iter().any(|rr| {
                matches!(
                    rr.outcome,
                    RepoOutcome::Fail { .. } | RepoOutcome::Error { .. }
                )
            })
    });

    if has_failure {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn run_validate(config_paths: Vec<PathBuf>) -> Result<ExitCode> {
    let sources = paths_to_sources(&config_paths);
    match load_config(&sources) {
        Ok(config) => {
            let policy_count = config.policies.len();
            let rule_count: usize = config.policies.iter().map(|p| p.rules.len()).sum();
            let provider_type = format!("{:?}", config.provider.provider_type).to_lowercase();
            println!(
                "Config is valid: {} {}, {} {}, provider: {}, org: {}",
                policy_count,
                if policy_count == 1 {
                    "policy"
                } else {
                    "policies"
                },
                rule_count,
                if rule_count == 1 { "rule" } else { "rules" },
                provider_type,
                config.provider.org,
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            error!("{e}");
            Ok(ExitCode::from(1))
        }
    }
}

fn run_list_rules(verbose: u8, quiet: bool) -> Result<ExitCode> {
    let rules = [
        ("ensure_file", "Ensure a file exists with optional content matching"),
        ("ensure_json_key", "Ensure a key exists in a JSON file"),
        ("ensure_yaml_key", "Ensure a key exists in a YAML file"),
        ("file_matches", "Check file content against a regex pattern"),
    ];

    if quiet {
        for (name, _) in &rules {
            println!("{name}");
        }
        let recipes = builtin_recipes()?;
        for recipe in &recipes {
            println!("{}", recipe.name);
        }
        return Ok(ExitCode::SUCCESS);
    }

    println!("Rules:");
    for (name, desc) in &rules {
        println!("  {name:<20}{desc}");
    }

    let recipes = builtin_recipes()?;
    println!("\nRecipes:");
    for recipe in &recipes {
        println!("  {:<20}{}", recipe.name, recipe.description);

        if verbose > 0 {
            println!("    Parameters:");
            for (name, def) in &recipe.params {
                let req = if def.required {
                    "required".to_string()
                } else if let Some(d) = &def.default {
                    format!("default: {d}")
                } else {
                    "optional".to_string()
                };

                let desc = def.description.as_deref().unwrap_or("");
                let enum_hint = def
                    .enum_values
                    .as_ref()
                    .map(|v| format!(" [{}]", v.join(", ")))
                    .unwrap_or_default();

                println!(
                    "      {:<16} {:<8} {:<28} {desc}{enum_hint}",
                    name, def.param_type, req
                );
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}
