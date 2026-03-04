mod registry;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::error;

use multipush_core::config::load_config;
use multipush_core::engine::evaluate;
use multipush_core::formatter::RepoOutcome;
use multipush_core::model::Severity;

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
        /// Config file path
        #[arg(short, long)]
        config: PathBuf,

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
    }
}

fn run_check(
    config_path: PathBuf,
    format: String,
    policy_filter: Vec<String>,
    no_color: bool,
    fail_on: Severity,
) -> Result<ExitCode> {
    let mut config =
        load_config(&config_path).context("failed to load config")?;

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
