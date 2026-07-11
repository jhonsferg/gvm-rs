//! Entry point for the `gvm` binary.
//!
//! Parses the command-line interface, loads the user configuration, and
//! dispatches to the appropriate command handler. No business logic lives
//! here; every command is implemented in its own submodule under
//! [`commands`].

mod archive;
mod cli;
mod commands;
mod config;
mod fs;
mod http;
mod lock;
mod remote;
mod shell;
mod toolchain;
mod user_version;
mod version;

use clap::Parser;
use colored::Colorize;

use cli::{Cli, Command};
use config::Config;
use http::HttpClient;

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {e:#}", "error:".red().bold());
        std::process::exit(1);
    }
}

/// Parses the CLI arguments, loads configuration, and runs the selected command.
///
/// # Errors
///
/// Returns an error if configuration cannot be loaded or if the command fails.
fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;

    // Create HTTP client with verbosity and retry settings from CLI
    let retries = match &cli.command {
        Command::Build { download, .. } => download.retries,
        Command::Install { download, .. } => download.retries,
        Command::Upgrade { download, .. } => download.retries,
        _ => 3,
    };
    let client = HttpClient::new(cli.verbose, retries)?;

    match cli.command {
        Command::Build {
            version,
            force,
            no_cgo,
            bootstrap,
            env_vars,
            ..
        } => commands::build::run(
            &config,
            &client,
            &version,
            force,
            no_cgo,
            bootstrap.as_deref(),
            &env_vars,
        ),
        Command::Install { version, force, .. } => {
            commands::install::run(&config, &client, &version, force)
        }
        Command::Use { version } => commands::use_version::run(&config, &version),
        Command::Default { version } => commands::default::run(&config, &version),
        Command::Local { version } => commands::local_version::run(&config, &version),
        Command::Uninstall { version } => commands::uninstall::run(&config, &version),
        Command::List => commands::list::run(&config),
        Command::ListRemote { all } => commands::list_remote::run(&config, &client, all),
        Command::Current => commands::current::run(&config),
        Command::Path { version } => commands::path::run(&config, version.as_deref()),
        Command::Env { shell } => commands::env::run(&config, shell.as_deref()),
        Command::Setup { shell, reset } => commands::setup::run(shell.as_deref(), reset),
        Command::Exec { version, args } => commands::exec::run(&config, &version, &args),
        Command::Doctor { shell } => commands::doctor::run(&config, shell.as_deref()),
        Command::Completions { shell } => commands::completions::run(&shell),
        Command::Upgrade { force, .. } => commands::upgrade::run(&client, force),
        Command::Implode { force } => commands::implode::run(&config, force),
        Command::Outdated => commands::outdated::run(&config, &client),
        Command::Prune {
            force,
            dry_run,
            scan_dir,
        } => commands::prune::run(&config, force, dry_run, scan_dir.as_deref()),
        Command::Shell {
            version,
            unset,
            shell,
        } => commands::shell::run(&config, version.as_deref(), unset, shell.as_deref()),
    }
}
