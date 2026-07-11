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
mod dispatch;
mod fs;
mod http;
mod lock;
mod profile;
mod remote;
mod resolver;
mod shell;
mod tempdir;
mod toolchain;
mod user_version;
mod version;

use clap::Parser;
use colored::Colorize;

use cli::{Cli, Command};
use config::{Config, ConfigMut};
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

    // Extract command name and args from clap Command enum
    let (cmd_name, args) = dispatch::command_to_name_and_args(&cli.command);

    // Dispatch through registry
    dispatch::dispatch(
        &config,
        &config as &dyn config::ConfigMut,
        &client,
        cmd_name,
        args,
    )
}

/// Converts a clap Command enum to a command name string and arguments vector.
fn command_to_name_and_args(cmd: &Command) -> (String, Vec<String>) {
    use Command::*;

    // Helper to extract DownloadArgs fields
    fn extract_download_args(download: &cli::DownloadArgs) -> Vec<String> {
        let mut args = Vec::new();
        if download.retries != 3 {
            args.push("--retries".to_string());
            args.push(download.retries.to_string());
        }
        args
    }

    match cmd {
        Build {
            version,
            force,
            no_cgo,
            bootstrap,
            env_vars,
            download,
        } => {
            let mut args = vec![version.clone()];
            if *force { args.push("--force".to_string()); }
            if *no_cgo { args.push("--no-cgo".to_string()); }
            if let Some(b) = bootstrap { args.push("--bootstrap".to_string()); args.push(b.clone()); }
            for e in env_vars { args.push("--env".to_string()); args.push(e.clone()); }
            args.extend(extract_download_args(download));
            ("build".to_string(), args)
        }
        Install { version, force, download } => {
            let mut args = vec![version.clone()];
            if *force { args.push("--force".to_string()); }
            args.extend(extract_download_args(download));
            ("install".to_string(), args)
        }
        Use { version } => ("use".to_string(), vec![version.clone()]),
        Default { version } => ("default".to_string(), vec![version.clone()]),
        Local { version } => ("local".to_string(), vec![version.clone()]),
        Uninstall { version } => ("uninstall".to_string(), vec![version.clone()]),
        List => ("list".to_string(), vec![]),
        ListRemote { all } => {
            let mut args = vec![];
            if *all { args.push("--all".to_string()); }
            ("list-remote".to_string(), args)
        }
        Current => ("current".to_string(), vec![]),
        Path { version } => {
            let mut args = vec![];
            if let Some(v) = version { args.push(v.clone()); }
            ("path".to_string(), args)
        }
        Env { shell } => {
            let mut args = vec![];
            if let Some(s) = shell { args.push(s.clone()); }
            ("env".to_string(), args)
        }
        Setup { shell, reset } => {
            let mut args = vec![];
            if let Some(s) = shell { args.push("--shell".to_string()); args.push(s.clone()); }
            if *reset { args.push("--reset".to_string()); }
            ("setup".to_string(), args)
        }
        Exec { version, args: cmd_args } => {
            let mut args = vec![version.clone()];
            args.extend(cmd_args.clone());
            ("exec".to_string(), args)
        }
        Doctor { shell } => {
            let mut args = vec![];
            if let Some(s) = shell { args.push("--shell".to_string()); args.push(s.clone()); }
            ("doctor".to_string(), args)
        }
        Completions { shell } => ("completions".to_string(), vec![shell.clone()]),
        Upgrade { force, download: _ } => {
            let mut args = vec![];
            if *force { args.push("--force".to_string()); }
            ("upgrade".to_string(), args)
        }
        Implode { force } => {
            let mut args = vec![];
            if *force { args.push("--force".to_string()); }
            ("implode".to_string(), args)
        }
        Outdated => ("outdated".to_string(), vec![]),
        Prune { force, dry_run, scan_dir } => {
            let mut args = vec![];
            if *force { args.push("--force".to_string()); }
            if *dry_run { args.push("--dry-run".to_string()); }
            if let Some(s) = scan_dir { args.push("--scan-dir".to_string()); args.push(s.clone()); }
            ("prune".to_string(), args)
        }
        Shell { version, unset, shell } => {
            let mut args = vec![];
            if let Some(v) = version { args.push(v.clone()); }
            if *unset { args.push("--unset".to_string()); }
            if let Some(s) = shell { args.push("--shell".to_string()); args.push(s.clone()); }
            ("shell".to_string(), args)
        }
    }
}

// Helper to extract download args
fn extract_download_args(download: &cli::DownloadArgs) -> Vec<String> {
    let mut args = Vec::new();
    if download.retries != 3 {
        args.push("--retries".to_string());
        args.push(download.retries.to_string());
    }
    args
}