use rustc_hash::FxHashMap;

use crate::object::Object;

#[derive(Clone, Debug, Default)]
pub struct Bytecode {
    pub instructions: Vec<u8>,
    pub constants: Vec<Object>,
    pub line_number_table: Vec<(usize, usize)>,
    pub num_cache_slots: u16,
    pub max_stack_depth: u16,
    /// Maximum number of registers used by this function/program.
    pub register_count: u16,
    /// Maps global variable names to their slot indices.
    pub globals_table: FxHashMap<String, u16>,
}

impl Bytecode {
    pub fn new(
        instructions: Vec<u8>,
        constants: Vec<Object>,
        line_number_table: Vec<(usize, usize)>,
    ) -> Self {
        Self {
            instructions,
            constants,
            line_number_table,
            num_cache_slots: 0,
            max_stack_depth: 0,
            register_count: 0,
            globals_table: FxHashMap::default(),
        }
    }

    pub fn with_cache_slots(
        instructions: Vec<u8>,
        constants: Vec<Object>,
        line_number_table: Vec<(usize, usize)>,
        num_cache_slots: u16,
        max_stack_depth: u16,
        register_count: u16,
    ) -> Self {
        Self {
            instructions,
            constants,
            line_number_table,
            num_cache_slots,
            max_stack_depth,
            register_count,
            globals_table: FxHashMap::default(),
        }
    }
}
