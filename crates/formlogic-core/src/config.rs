use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use indexmap::IndexMap;

use crate::token::{default_operators, TokenType};

/// Synchronous host call handler. The VM calls this during execution when
/// a script invokes `host.callSync(kind, argsArray)`. The handler receives
/// the call kind and serialized arguments, and returns a JSON string result
/// (or an error). This enables the host to provide synchronous external
/// operations (e.g., GPU compute) without pausing/resuming the VM.
pub type SyncHostCallFn = Arc<dyn Fn(&str, &[String]) -> Result<String, String> + Send + Sync>;

#[derive(Clone)]
pub struct FormLogicConfig {
    pub max_instructions: Option<u64>,
    pub max_wall_time_ms: Option<u64>,
    pub await_timeout_ms: Option<u64>,
    /// Maximum number of heap objects (strings, arrays, hashes, closures, etc.).
    pub max_heap_objects: Option<usize>,
    /// Maximum heap memory in bytes. Checked alongside max_heap_objects.
    /// Prevents OOM from scripts that grow existing objects (e.g., array.push in a loop).
    pub max_heap_bytes: Option<usize>,
    /// External abort flag — checked in the instruction loop alongside other limits.
    /// When set to `true`, execution terminates immediately with an error.
    /// Used by the host to kill stuck executions after an outer timeout fires.
    pub abort_flag: Option<Arc<AtomicBool>>,
    /// Synchronous host call handler for external operations (GPU compute, etc.).
    /// Called inline during VM execution — must return quickly to avoid timeout.
    pub sync_host_call: Option<SyncHostCallFn>,
    pub enable_vm_profiling: bool,
    pub operators: IndexMap<&'static str, TokenType>,
}

impl std::fmt::Debug for FormLogicConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormLogicConfig")
            .field("max_instructions", &self.max_instructions)
            .field("max_wall_time_ms", &self.max_wall_time_ms)
            .field("sync_host_call", &self.sync_host_call.is_some())
            .finish()
    }
}

impl Default for FormLogicConfig {
    fn default() -> Self {
        Self {
            max_instructions: Some(100_000_000),
            max_wall_time_ms: Some(5_000),
            await_timeout_ms: Some(30_000),
            max_heap_objects: Some(100_000),
            max_heap_bytes: Some(64 * 1024 * 1024),
            abort_flag: None,
            sync_host_call: None,
            enable_vm_profiling: false,
            operators: default_operators().into_iter().collect(),
        }
    }
}
