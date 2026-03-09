//! File system bridge — allows FormLogic scripts to access files (scoped).

/// Pluggable file system backend for server-side FormLogic.
pub trait FsBridge {
    /// Read a file as a UTF-8 string.
    fn read_file(&self, path: &str) -> Result<String, String>;
    /// Write a UTF-8 string to a file (creates or overwrites).
    fn write_file(&mut self, path: &str, content: &str) -> Result<(), String>;
    /// Append a UTF-8 string to a file.
    fn append_file(&mut self, path: &str, content: &str) -> Result<(), String>;
    /// Check if a file or directory exists.
    fn exists(&self, path: &str) -> bool;
    /// List entries in a directory (file/dir names only).
    fn list_dir(&self, path: &str) -> Result<Vec<String>, String>;
    /// Delete a file.
    fn delete_file(&mut self, path: &str) -> Result<(), String>;
    /// Create a directory (and parents).
    fn mkdir(&mut self, path: &str) -> Result<(), String>;
}
