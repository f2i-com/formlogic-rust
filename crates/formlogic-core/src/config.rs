use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use indexmap::IndexMap;

use crate::token::{default_operators, TokenType};

#[derive(Clone, Debug)]
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
    pub enable_vm_profiling: bool,
    pub operators: IndexMap<&'static str, TokenType>,
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
            enable_vm_profiling: false,
            operators: default_operators().into_iter().collect(),
        }
    }
}
