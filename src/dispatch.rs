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
pub trait Command: Send + Sync {
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
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
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
    ) -> Result<()> {
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

/// Builds and registers all built-in commands.
///
/// Called once at startup from `main()`.
pub fn register_commands() -> CommandRegistry {
    let mut registry = CommandRegistry::new();

    // Built-in commands will be registered here
    // TODO: Migrate each command to implement the Command trait

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_registers_and_retrieves() {
        struct TestCmd;
        impl Command for TestCmd {
            fn name(&self) -> &'static str {
                "test"
            }
            fn execute(
                &self,
                _config: &Config,
                _config_mut: &dyn ConfigMut,
                _client: Option<&HttpClient>,
                _args: Vec<String>,
            ) -> Result<()> {
                Ok(())
            }
        }

        let mut registry = CommandRegistry::new();
        registry.register(Box::new(TestCmd));
        assert!(registry.get("test").is_some());
        assert!(registry.get("nonexistent").is_none());
    }
}