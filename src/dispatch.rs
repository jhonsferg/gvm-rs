//! Command dispatch trait and registry.
//!
//! Provides an extensible command system that follows the Open/Closed Principle -
//! adding new commands doesn't require modifying `main.rs`.

use anyhow::Result;
use std::collections::HashMap;

use crate::{
    config::{Config, ConfigMut},
    http::HttpClient,
};

/// Trait for all gvm commands.
///
/// Each command implements this trait to provide its name, description,
/// and execution logic. Commands are registered in the global registry
/// at startup via [`register_commands`].
pub trait Command: Send + Sync + std::fmt::Debug {
    /// Command name (e.g., "install", "use", "build").
    fn name(&self) -> &'static str;

    /// Short description for help text.
    fn description(&self) -> &'static str {
        ""
    }

    /// Whether this command requires an HTTP client.
    fn needs_http(&self) -> bool {
        true
    }

    /// Executes the command.
    ///
    /// # Arguments
    /// - `config`: Read-only access to configuration (paths, etc.)
    /// - `config_mut`: Mutable access for commands that need to create directories
    /// - `client`: HTTP client for network operations (None if `needs_http` is false)
    /// - `args`: Command-specific arguments parsed by clap
    fn execute(
        &self,
        config: &Config,
        config_mut: &dyn ConfigMut,
        client: Option<&HttpClient>,
        args: Vec<String>,
    ) -> Result<()>;
}

/// Global command registry.
///
/// Thread-safe registry that stores all registered commands.
/// Initialized once at startup via [`register_commands`].
#[derive(Debug)]
pub struct CommandRegistry {
    commands: std::collections::HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            commands: std::collections::HashMap::new(),
        }
    }

    /// Registers a command in the registry.
    pub fn register(&mut self, command: Box<dyn Command>) {
        let name = command.name().to_string();
        self.commands.insert(name, command);
    }

    /// Gets a command by name.
    pub fn get(&self, name: &str) -> Option<&dyn Command> {
        self.commands.get(name).map(|c| c.as_ref())
    }

    /// Returns all registered command names, sorted.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.commands.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Dispatches a command by name.
    ///
    /// # Arguments
    /// - `name`: Command name to dispatch
    /// - `config`: Read-only config reference
    /// - `config_mut`: Mutable config reference (for directory creation)
    /// - `client`: HTTP client (optional)
    /// - `args`: Command arguments
    ///
    /// # Errors
    /// Returns an error if the command is not found or execution fails.
    pub fn dispatch(
        &self,
        name: &str,
        config: &Config,
        config_mut: &dyn ConfigMut,
        client: &HttpClient,
        args: Vec<String>,
    ) -> anyhow::Result<()> {
        let command = self.commands.get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown command: {}", name))?;
        let client_opt = if command.needs_http() { Some(client) } else { None };
        command.execute(config, config_mut, client_opt, args)
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global command registry instance.
///
/// Initialized by [`register_commands`] at startup.
static COMMAND_REGISTRY: std::sync::OnceLock<CommandRegistry> = std::sync::OnceLock::new();

/// Returns the global command registry.
pub fn registry() -> &'static CommandRegistry {
    COMMAND_REGISTRY.get().expect("Command registry not initialized")
}

/// Builds the complete command registry with all built-in commands.
pub fn build_registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();

    // Built-in commands will be registered here
    // TODO: Register commands when they implement the Command trait

    registry
}

/// Registers all commands in the global registry.
pub fn register_commands() {
    let registry = build_registry();
    COMMAND_REGISTRY.set(registry).expect("Registry already initialized");
}

/// Converts a clap Command enum to a command name string and arguments vector.
pub fn command_to_name_and_args(cmd: &crate::cli::Command) -> (String, Vec<String>) {
    use crate::cli::Command;

    // Helper to extract DownloadArgs fields
    fn extract_download_args(download: &crate::cli::DownloadArgs) -> Vec<String> {
        let mut args = Vec::new();
        if download.retries != 3 {
            args.push("--retries".to_string());
            args.push(download.retries.to_string());
        }
        args
    }

    use crate::cli::Command::*;

    match cmd {
        Build { version, force, no_cgo, bootstrap, env_vars, download } => {
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

// Helper to extract DownloadArgs fields - implemented inline to avoid circular dependency
impl crate::cli::DownloadArgs {
    fn into_vec(self) -> Vec<String> {
        let mut args = Vec::new();
        if self.retries != 3 {
            args.push("--retries".to_string());
            args.push(self.retries.to_string());
        }
        args
    }
}

/// Free function to dispatch a command using the built-in registry.
/// This is the main entry point for command dispatch from main.rs.
pub fn dispatch(
    config: &crate::config::Config,
    config_mut: &dyn crate::config::ConfigMut,
    client: &crate::http::HttpClient,
    name: String,
    args: Vec<String>,
) -> anyhow::Result<()> {
    let registry = build_registry();
    registry.dispatch(&name, config, config_mut, client, args)
}