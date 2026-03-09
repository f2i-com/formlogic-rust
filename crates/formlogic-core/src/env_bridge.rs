//! Environment bridge — allows FormLogic scripts to read environment variables.

/// Pluggable environment variable backend for server-side FormLogic.
pub trait EnvBridge {
    /// Get an environment variable by name. Returns None if not set.
    fn get(&self, name: &str) -> Option<String>;
    /// Get all environment variable names.
    fn keys(&self) -> Vec<String>;
}
