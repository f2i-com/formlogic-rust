use std::collections::HashMap;

use crate::token::{default_operators, TokenType};

#[derive(Clone, Debug)]
pub struct FormLogicConfig {
    pub max_instructions: Option<u64>,
    pub max_wall_time_ms: Option<u64>,
    pub await_timeout_ms: Option<u64>,
    pub enable_vm_profiling: bool,
    pub operators: HashMap<&'static str, TokenType>,
}

impl Default for FormLogicConfig {
    fn default() -> Self {
        Self {
            max_instructions: Some(100_000_000),
            max_wall_time_ms: Some(86_400_000),
            await_timeout_ms: Some(30_000),
            enable_vm_profiling: false,
            operators: default_operators().into_iter().collect(),
        }
    }
}
