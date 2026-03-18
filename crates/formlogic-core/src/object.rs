use std::cell::UnsafeCell;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
#[cfg(not(target_arch = "riscv32"))]
use std::sync::atomic::{AtomicU64, Ordering};

use indexmap::IndexMap;
use rustc_hash::FxHashMap;

use crate::intern;
use crate::value::Value;

/// A single-threaded cell that wraps `UnsafeCell` with a `borrow()`/`borrow_mut()` API,
/// eliminating `RefCell`'s runtime borrow-checking overhead.
///
/// # Safety
/// The VM is single-threaded (WASM). Callers must ensure no aliasing mutable references exist.
pub struct VmCell<T>(UnsafeCell<T>);

impl<T: fmt::Debug> fmt::Debug for VmCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VmCell")
            .field(unsafe { &*self.0.get() })
            .finish()
    }
}

impl<T: Clone> Clone for VmCell<T> {
    fn clone(&self) -> Self {
        Self(UnsafeCell::new(unsafe { &*self.0.get() }.clone()))
    }
}

impl<T> VmCell<T> {
    pub fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[inline(always)]
    pub fn borrow(&self) -> &T {
        // SAFETY: VM is single-threaded; no concurrent mutable access.
        unsafe { &*self.0.get() }
    }

    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn borrow_mut(&self) -> &mut T {
        // SAFETY: VM is single-threaded; caller ensures no aliasing references.
        unsafe { &mut *self.0.get() }
    }

    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

#[derive(Clone, Debug)]
pub enum HashKey {
    Sym(u32),
    Int(i64),
    Float(u64),
    Bool(bool),
    Null,
    Undefined,
    Other(String),
}

fn canonical_float_bits(bits: u64) -> u64 {
    let value = f64::from_bits(bits);
    if value == 0.0 {
        0.0f64.to_bits()
    } else if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        bits
    }
}

fn float_bits_to_i64_exact(bits: u64) -> Option<i64> {
    let value = f64::from_bits(bits);
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some(0);
    }
    if value.trunc() == value && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        Some(value as i64)
    } else {
        None
    }
}

impl PartialEq for HashKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (HashKey::Sym(a), HashKey::Sym(b)) => a == b,
            (HashKey::Int(a), HashKey::Int(b)) => a == b,
            (HashKey::Float(a), HashKey::Float(b)) => a == b,
            (HashKey::Int(a), HashKey::Float(b)) | (HashKey::Float(b), HashKey::Int(a)) => {
                float_bits_to_i64_exact(*b) == Some(*a)
            }
            (HashKey::Bool(a), HashKey::Bool(b)) => a == b,
            (HashKey::Null, HashKey::Null) => true,
            (HashKey::Undefined, HashKey::Undefined) => true,
            (HashKey::Other(a), HashKey::Other(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for HashKey {}

impl Hash for HashKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            HashKey::Sym(id) => {
                0u8.hash(state);
                id.hash(state);
            }
            HashKey::Int(v) => {
                1u8.hash(state);
                v.hash(state);
            }
            HashKey::Float(bits) => {
                if let Some(i) = float_bits_to_i64_exact(*bits) {
                    1u8.hash(state);
                    i.hash(state);
                } else {
                    2u8.hash(state);
                    bits.hash(state);
                }
            }
            HashKey::Bool(v) => {
                3u8.hash(state);
                v.hash(state);
            }
            HashKey::Null => {
                4u8.hash(state);
            }
            HashKey::Undefined => {
                5u8.hash(state);
            }
            HashKey::Other(s) => {
                6u8.hash(state);
                s.hash(state);
            }
        }
    }
}

impl fmt::Display for HashKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HashKey::Sym(id) => write!(f, "{}", intern::resolve(*id)),
            HashKey::Int(v) => write!(f, "{}", v),
            HashKey::Float(bits) => write!(f, "{}", f64::from_bits(*bits)),
            HashKey::Bool(v) => write!(f, "{}", v),
            HashKey::Null => write!(f, "null"),
            HashKey::Undefined => write!(f, "undefined"),
            HashKey::Other(s) => write!(f, "{}", s),
        }
    }
}

impl HashKey {
    pub fn from_string(s: &str) -> Self {
        HashKey::Sym(intern::intern(s))
    }

    pub fn from_owned_string(s: String) -> Self {
        HashKey::Sym(intern::intern(&s))
    }

    pub fn from_sym(id: u32) -> Self {
        HashKey::Sym(id)
    }

    pub fn from_int(v: i64) -> Self {
        HashKey::Int(v)
    }

    pub fn from_float(v: f64) -> Self {
        HashKey::Float(canonical_float_bits(v.to_bits()))
    }

    pub fn from_bool(v: bool) -> Self {
        HashKey::Bool(v)
    }

    pub fn as_str_value(&self) -> Option<Rc<str>> {
        match self {
            HashKey::Sym(id) => Some(intern::resolve(*id)),
            _ => None,
        }
    }

    pub fn display_key(&self) -> String {
        match self {
            HashKey::Sym(id) => intern::resolve(*id).to_string(),
            HashKey::Int(v) => v.to_string(),
            HashKey::Float(bits) => f64::from_bits(*bits).to_string(),
            HashKey::Bool(v) => v.to_string(),
            HashKey::Null => "null".to_string(),
            HashKey::Undefined => "undefined".to_string(),
            HashKey::Other(s) => s.clone(),
        }
    }

    pub fn is_numeric_index(&self) -> Option<u32> {
        match self {
            HashKey::Int(v) if *v >= 0 => Some(*v as u32),
            HashKey::Float(bits) => float_bits_to_i64_exact(*bits).and_then(|v| {
                if v >= 0 {
                    u32::try_from(v).ok()
                } else {
                    None
                }
            }),
            HashKey::Sym(id) => intern::resolve(*id).parse::<u32>().ok(),
            _ => None,
        }
    }
}

#[cfg(not(target_arch = "riscv32"))]
static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(not(target_arch = "riscv32"))]
fn next_object_id() -> u64 {
    NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed)
}

// riscv32 (zkVM) is single-threaded — use a simple Cell counter instead of atomics.
#[cfg(target_arch = "riscv32")]
fn next_object_id() -> u64 {
    use std::cell::Cell;
    thread_local! {
        static NEXT_OBJECT_ID: Cell<u64> = Cell::new(1);
    }
    NEXT_OBJECT_ID.with(|c| {
        let id = c.get();
        c.set(id + 1);
        id
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ObjectType {
    Integer,
    Float,
    Boolean,
    Null,
    Undefined,
    ReturnValue,
    Error,
    Function,
    String,
    Builtin,
    Module,
    Array,
    Hash,
    CompiledFunction,
    Closure,
    Promise,
    Symbol,
    Accessor,
    PropertyDescriptor,
    SuperObject,
    Iterator,
    Cell,
    Regexp,
    Map,
    Set,
    WeakMap,
    WeakSet,
    Class,
    Instance,
    BoundMethod,
    SuperRef,
    Generator,
}

#[derive(Clone, Debug)]
pub enum Object {
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Null,
    Undefined,
    String(Rc<str>),
    RegExp(Box<RegExpObject>),
    Map(Box<MapObject>),
    Set(Box<SetObject>),
    Array(Rc<VmCell<Vec<Value>>>),
    Hash(Rc<VmCell<HashObject>>),
    CompiledFunction(Box<CompiledFunctionObject>),
    Class(Box<ClassObject>),
    BuiltinFunction(Box<BuiltinFunctionObject>),
    Instance(Box<InstanceObject>),
    BoundMethod(Box<BoundMethodObject>),
    SuperRef(Box<SuperRefObject>),
    Promise(Box<PromiseObject>),
    Generator(Rc<VmCell<GeneratorObject>>),
    /// A unique symbol value with an optional description and a unique ID.
    Symbol(u32, Option<Rc<str>>),
    ReturnValue(Box<Object>),
    Error(Box<ErrorObject>),
}

#[derive(Clone, Debug)]
pub struct ErrorObject {
    pub name: Rc<str>,
    pub message: Rc<str>,
}

#[derive(Clone, Debug)]
#[repr(u8)]
pub enum BuiltinFunction {
    MathAbs,
    MathFloor,
    MathCeil,
    MathRound,
    MathMin,
    MathMax,
    MathPow,
    MathSqrt,
    MathTrunc,
    MathSign,
    MathRandom,
    MathLog,
    MathLog2,
    MathCbrt,
    MathSin,
    MathCos,
    MathTan,
    MathExp,
    MathLog10,
    MathAtan2,
    MathHypot,
    MathImul,
    MathClz32,
    MathFround,
    StringCtor,
    NumberCtor,
    StringFromCharCode,
    StringCharAt,
    StringSplit,
    StringIncludes,
    StringSlice,
    StringToUpperCase,
    StringToLowerCase,
    StringTrim,
    StringStartsWith,
    StringEndsWith,
    StringIndexOf,
    StringLastIndexOf,
    StringSubstring,
    StringRepeat,
    StringPadStart,
    StringPadEnd,
    StringCharCodeAt,
    StringReplace,
    StringReplaceAll,
    NumberToFixed,
    NumberToPrecision,
    NumberToString,
    ParseInt,
    ParseFloat,
    IsNaN,
    IsFinite,
    NumberIsNaN,
    NumberIsFinite,
    NumberIsInteger,
    NumberIsSafeInteger,
    ObjectKeys,
    ObjectValues,
    ObjectEntries,
    ObjectFromEntries,
    ObjectHasOwn,
    ObjectIs,
    ObjectAssign,
    ObjectFreeze,
    ObjectCreate,
    ArrayOf,
    HashHasOwnProperty,
    JsonStringify,
    JsonParse,
    PromiseResolve,
    PromiseReject,
    ArrayMap,
    ArrayForEach,
    ArrayFlatMap,
    ArrayFlat,
    ArrayReverse,
    ArraySome,
    ArrayEvery,
    ArrayFindIndex,
    ArrayIndexOf,
    ArrayLastIndexOf,
    ArrayPop,
    ArrayPush,
    ArraySort,
    ArrayFilter,
    ArrayReduce,
    ArrayReduceRight,
    ArrayFind,
    ArrayIncludes,
    ArrayJoin,
    ArrayToString,
    ArrayValueOf,
    ArraySlice,
    ArrayAt,
    ArrayToSorted,
    ArrayWith,
    ArrayKeys,
    ArrayValues,
    ArrayEntries,
    ArrayShift,
    ArrayUnshift,
    ArraySplice,
    ArrayConcat,
    ArrayFrom,
    ArrayIsArray,
    ArrayFill,
    ArrayCopyWithin,
    ArrayFindLast,
    ArrayFindLastIndex,
    ArrayToReversed,
    RegExpCtor,
    RegExpTest,
    RegExpExec,
    StringMatch,
    StringMatchAll,
    StringSearch,
    StringConcat,
    StringTrimStart,
    StringTrimEnd,
    StringFromCodePoint,
    MapCtor,
    MapSet,
    MapGet,
    MapHas,
    MapDelete,
    MapClear,
    MapKeys,
    MapValues,
    MapEntries,
    MapForEach,
    SetCtor,
    SetAdd,
    SetHas,
    SetDelete,
    SetClear,
    SetKeys,
    SetValues,
    SetEntries,
    SetForEach,
    SymbolCtor,
    GeneratorNext,
    GeneratorReturn,
    GeneratorThrow,
    LocalStorageGetItem,
    LocalStorageSetItem,
    LocalStorageRemoveItem,
    LocalStorageClear,
    DateNow,
    DateToISOString,
    DateGetTime,
    DateGetHours,
    DateGetMinutes,
    DateGetSeconds,
    DateGetMilliseconds,
    DateGetFullYear,
    DateGetMonth,
    DateGetDate,
    DateGetDay,
    DateToLocaleDateString,
    DateToLocaleTimeString,
    DateToLocaleString,
    DateToString,
    DateValueOf,
    DbQuery,
    DbCreate,
    DbUpdate,
    DbDelete,
    DbHardDelete,
    DbGet,
    DbStartSync,
    DbStopSync,
    DbGetSyncStatus,
    DbGetSavedSyncRoom,

    // ── HTTP bridge ──
    HttpGet,
    HttpPost,
    HttpPut,
    HttpDelete,

    // ── FS bridge ──
    FsReadFile,
    FsWriteFile,
    FsAppendFile,
    FsExists,
    FsListDir,
    FsDeleteFile,
    FsMkdir,

    // ── Env bridge ──
    EnvGet,
    EnvKeys,
    EnvLog,

    // ── Draw bridge ──
    DrawRect,
    DrawRoundedRect,
    DrawCircle,
    DrawEllipse,
    DrawLine,
    DrawPath,
    DrawText,
    DrawImage,
    DrawLinearGradient,
    DrawRadialGradient,
    DrawShadow,
    DrawPushClip,
    DrawPopClip,
    DrawPushTransform,
    DrawPopTransform,
    DrawPushOpacity,
    DrawPopOpacity,
    DrawArc,
    DrawMeasureText,
    DrawGetViewportWidth,
    DrawGetViewportHeight,

    // ── Layout bridge ──
    LayoutCreateNode,
    LayoutUpdateStyle,
    LayoutSetChildren,
    LayoutComputeLayout,
    LayoutGetLayout,
    LayoutRemoveNode,
    LayoutClear,

    // ── Input bridge ──
    InputGetMouseX,
    InputGetMouseY,
    InputIsMouseDown,
    InputIsMousePressed,
    InputIsMouseReleased,
    InputGetScrollY,
    InputSetCursor,
    InputGetTextInput,
    InputIsBackspacePressed,
    InputIsEscapePressed,
    InputRequestRedraw,
    InputGetElapsedSecs,
    InputGetPageElapsedSecs,
    InputGetDeltaTime,
    InputGetFocusedInput,
    InputSetFocusedInput,
    InputIsKeyDown,

    // ── Window event bridge ──
    WindowAddEventListener,
    WindowRemoveEventListener,
    EventPreventDefault,
    EventStopPropagation,

    // ── URI encoding ──
    EncodeURIComponent,
    DecodeURIComponent,
    EncodeURI,
    DecodeURI,

    // ── Generic host call bridge ──
    /// host.call(kind, argsArray, callback) — queues an async call to the host.
    HostCall,
    /// host.callSync(kind, argsArray) — synchronous host call, returns JSON result.
    HostCallSync,

    // ── Error constructor ──
    ErrorConstructor,
    StringAt,
    StringCodePointAt,
    StringNormalize,
    StringRaw,
    StructuredClone,
}

#[derive(Clone, Debug)]
pub struct BuiltinFunctionObject {
    pub function: BuiltinFunction,
    pub receiver: Option<Object>,
}

#[derive(Clone, Debug)]
pub struct CompiledFunctionObject {
    pub instructions: Rc<Vec<u8>>,
    pub constants: Rc<Vec<Object>>,
    pub num_locals: usize,
    pub num_parameters: usize,
    pub rest_parameter_index: Option<usize>,
    pub takes_this: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub num_cache_slots: u16,
    pub max_stack_depth: u16,
    /// Maximum number of registers used by this function.
    pub register_count: u16,
    /// Persistent inline property cache shared across all invocations of this
    /// function.  Entries are `(shape_version, slot_index)` indexed by
    /// `cache_slot`.  `Rc` ensures all clones of this function (including
    /// `BoundMethod` wrappers) share the same warm cache.
    pub inline_cache: Rc<VmCell<Vec<(u32, u32)>>>,
    /// Global slot indices that inner closures need to capture at creation time.
    /// Set by the compiler for functions that contain captured parameters.
    pub closure_captures: Vec<u16>,
    /// Captured global slot values, snapshotted at closure creation time.
    /// Set by the VM's MakeClosure opcode.
    pub captured_values: Vec<(u16, Value)>,
}

#[derive(Clone, Debug)]
pub struct PromiseObject {
    pub settled: PromiseState,
}

#[derive(Clone, Debug)]
pub struct RegExpObject {
    pub pattern: String,
    pub flags: String,
}

#[derive(Clone, Debug)]
pub struct MapObject {
    pub entries: Rc<VmCell<Vec<(HashKey, Value)>>>,
    pub indices: Rc<VmCell<FxHashMap<HashKey, usize>>>,
}

impl Default for MapObject {
    fn default() -> Self {
        Self {
            entries: Rc::new(VmCell::new(Vec::new())),
            indices: Rc::new(VmCell::new(FxHashMap::default())),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SetObject {
    pub entries: Rc<VmCell<Vec<HashKey>>>,
    pub indices: Rc<VmCell<FxHashMap<HashKey, usize>>>,
}

impl Default for SetObject {
    fn default() -> Self {
        Self {
            entries: Rc::new(VmCell::new(Vec::new())),
            indices: Rc::new(VmCell::new(FxHashMap::default())),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PromiseState {
    Fulfilled(Box<Object>),
    Rejected(Box<Object>),
}

/// State of a generator: Created (not yet started), Suspended (yielded),
/// or Completed (returned or threw).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneratorState {
    Created,
    Suspended,
    Completed,
}

/// A generator object created by calling a `function*`.
///
/// Holds the compiled function, the saved execution state (IP, locals),
/// and the generator's current state.  The VM resumes execution from the
/// saved IP when `.next()` is called.
#[derive(Clone, Debug)]
pub struct GeneratorObject {
    /// The compiled generator function body.
    pub function: CompiledFunctionObject,
    /// Saved locals (variables) — populated after the first `yield`.
    pub locals: Vec<Object>,
    /// Saved instruction pointer — where to resume.
    pub saved_ip: usize,
    /// Arguments passed to the generator function on initial call.
    pub args: Vec<Value>,
    /// The receiver (`this`) if any.
    pub receiver: Option<Value>,
    /// Current state.
    pub state: GeneratorState,
}

/// One level of super methods/getters/setters for multi-level inheritance chains.
pub type SuperLevel = (
    FxHashMap<String, CompiledFunctionObject>,
    FxHashMap<String, CompiledFunctionObject>,
    FxHashMap<String, CompiledFunctionObject>,
);

#[derive(Clone, Debug)]
pub struct ClassObject {
    pub name: String,
    pub parent_chain: Vec<String>,
    pub constructor: Option<CompiledFunctionObject>,
    pub methods: FxHashMap<String, CompiledFunctionObject>,
    pub static_methods: FxHashMap<String, CompiledFunctionObject>,
    pub getters: FxHashMap<String, CompiledFunctionObject>,
    pub setters: FxHashMap<String, CompiledFunctionObject>,
    pub super_methods: FxHashMap<String, CompiledFunctionObject>,
    pub super_getters: FxHashMap<String, CompiledFunctionObject>,
    pub super_setters: FxHashMap<String, CompiledFunctionObject>,
    /// Chain of ancestor super levels for multi-level inheritance.
    /// `super_constructor_chain[0]` = grandparent methods, `[1]` = great-grandparent, etc.
    pub super_constructor_chain: Vec<SuperLevel>,
    /// Instance field initializers: `(name, initializer_fn)`.
    /// Each initializer is a zero-arg `takes_this=true` function whose return
    /// value is the field's initial value.  Executed in order during `new`.
    pub field_initializers: Vec<(String, CompiledFunctionObject)>,
    /// Static initializers run once at class definition time, in order.
    pub static_initializers: Vec<StaticInitializer>,
    /// Static field values, populated at class-definition time by the
    /// `OpInitClass` / `InitClass` opcode.
    pub static_fields: FxHashMap<String, Object>,
}

/// A static initializer — either a field assignment or a static block.
#[derive(Clone, Debug)]
pub enum StaticInitializer {
    /// `static name = expr;` — thunk returns the value to assign.
    Field {
        name: String,
        thunk: CompiledFunctionObject,
    },
    /// `static { ... }` — thunk is executed for side effects.
    Block { thunk: CompiledFunctionObject },
}

#[derive(Clone, Debug)]
pub struct InstanceObject {
    pub class_name: String,
    pub parent_chain: Vec<String>,
    pub fields: FxHashMap<String, Object>,
    pub methods: FxHashMap<String, CompiledFunctionObject>,
    pub getters: FxHashMap<String, CompiledFunctionObject>,
    pub setters: FxHashMap<String, CompiledFunctionObject>,
    pub super_methods: FxHashMap<String, CompiledFunctionObject>,
    pub super_getters: FxHashMap<String, CompiledFunctionObject>,
    pub super_setters: FxHashMap<String, CompiledFunctionObject>,
    /// Remaining ancestor chain for multi-level super() constructor calls.
    pub super_constructor_chain: Vec<SuperLevel>,
}

#[derive(Clone, Debug)]
pub struct BoundMethodObject {
    pub function: CompiledFunctionObject,
    pub receiver: Box<Object>,
}

#[derive(Clone, Debug)]
pub struct SuperRefObject {
    pub receiver: Box<Object>,
    pub methods: FxHashMap<String, CompiledFunctionObject>,
    pub getters: FxHashMap<String, CompiledFunctionObject>,
    pub setters: FxHashMap<String, CompiledFunctionObject>,
    /// Remaining ancestor chain so nested super() calls resolve correctly.
    pub constructor_chain: Vec<SuperLevel>,
}

#[derive(Clone, Debug)]
pub struct HashObject {
    pub id: u64,
    pub shape_version: u32,
    pub pairs: IndexMap<HashKey, Value>,
    pub values: Vec<Value>,
    pub str_slots: FxHashMap<u32, usize>,
    /// True when `values` has been updated via `set_value_at_slot_unchecked`
    /// without syncing the corresponding entries in `pairs`.
    /// Call `sync_pairs_if_dirty()` before reading from `pairs`.
    pub pairs_dirty: bool,
    /// Backing store for heap-type Values created outside the VM's main heap
    /// (e.g., compiler-constructed builtin globals). Migrated to VM heap on
    /// first access via `migrate_local_objects`. Empty for VM-created hashes.
    pub local_objects: Vec<Object>,
    /// Getter accessor functions defined on this object (e.g. `{ get x() { ... } }`).
    pub getters: Option<Box<FxHashMap<String, CompiledFunctionObject>>>,
    /// Setter accessor functions defined on this object (e.g. `{ set x(v) { ... } }`).
    pub setters: Option<Box<FxHashMap<String, CompiledFunctionObject>>>,
    /// True when Object.freeze() has been called on this object.
    pub frozen: bool,
}

impl Default for HashObject {
    fn default() -> Self {
        Self {
            id: next_object_id(),
            shape_version: 1,
            pairs: IndexMap::<HashKey, Value>::new(),
            values: Vec::new(),
            str_slots: FxHashMap::default(),
            pairs_dirty: false,
            local_objects: Vec::new(),
            getters: None,
            setters: None,
            frozen: false,
        }
    }
}

impl HashObject {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            id: next_object_id(),
            shape_version: 1,
            pairs: IndexMap::<HashKey, Value>::with_capacity(capacity),
            values: Vec::with_capacity(capacity),
            str_slots: {
                let mut m = FxHashMap::default();
                m.reserve(capacity);
                m
            },
            pairs_dirty: false,
            local_objects: Vec::new(),
            getters: None,
            setters: None,
            frozen: false,
        }
    }

    pub fn bump_shape_version(&mut self) {
        self.shape_version = self.shape_version.wrapping_add(1);
        if self.shape_version == 0 {
            self.shape_version = 1;
        }
    }

    pub fn insert_pair(&mut self, key: HashKey, value: Value) -> bool {
        if let HashKey::Sym(id) = &key {
            if let Some(&slot) = self.str_slots.get(id) {
                self.values[slot] = value;
                self.pairs[slot] = value;
                return false;
            }
        }

        if let Some(slot) = self.pairs.get_index_of(&key) {
            self.values[slot] = value;
            self.pairs[slot] = value;
            return false;
        }

        let sym_id = match &key {
            HashKey::Sym(id) => Some(*id),
            _ => None,
        };
        self.pairs.insert(key, value);
        self.values.push(value);
        if let Some(id) = sym_id {
            self.str_slots.insert(id, self.values.len() - 1);
        }
        let is_new = true;
        if is_new {
            self.bump_shape_version();
        }
        is_new
    }

    pub fn remove_pair(&mut self, key: &HashKey) -> Option<Value> {
        let Some(slot) = self.pairs.get_index_of(key) else {
            return None;
        };

        // Sync before removal since shift_remove returns the value.
        self.sync_pairs_if_dirty();

        let removed_sym = self.pairs.get_index(slot).and_then(|(k, _)| match k {
            HashKey::Sym(id) => Some(*id),
            _ => None,
        });

        let removed = self.pairs.shift_remove(key);
        self.values.remove(slot);

        if let Some(id) = removed_sym {
            self.str_slots.remove(&id);
        }
        for idx in self.str_slots.values_mut() {
            if *idx > slot {
                *idx -= 1;
            }
        }

        if removed.is_some() {
            self.bump_shape_version();
        }
        removed
    }

    #[inline(always)]
    pub fn get_value_at_slot(&self, slot: usize) -> Option<Value> {
        self.values.get(slot).copied()
    }

    #[inline(always)]
    pub unsafe fn get_value_at_slot_unchecked(&self, slot: usize) -> Value {
        *self.values.get_unchecked(slot)
    }

    pub fn ordered_keys_ref(&self) -> Vec<HashKey> {
        self.pairs.keys().cloned().collect()
    }

    pub fn ordered_keys(&self) -> Vec<HashKey> {
        self.pairs.keys().cloned().collect()
    }

    /// Get by symbol ID — reads from `values` via `str_slots`.
    #[inline(always)]
    pub fn get_by_sym(&self, sym: u32) -> Option<Value> {
        self.str_slots
            .get(&sym)
            .and_then(|&slot| self.values.get(slot).copied())
    }

    /// Get by string key — interns to u32 then looks up via `str_slots`.
    pub fn get_by_str(&self, s: &str) -> Option<Value> {
        let sym = intern::intern(s);
        self.get_by_sym(sym)
    }

    /// Get by string key that reads from contiguous value slots.
    pub fn get_value_by_str(&self, s: &str) -> Option<Value> {
        self.get_by_str(s)
    }

    /// Update/insert by symbol ID while preserving fast slot layout.
    pub fn set_by_sym(&mut self, sym: u32, value: Value) {
        if self.frozen {
            return;
        }
        self.insert_pair(HashKey::Sym(sym), value);
    }

    /// Update/insert by `Rc<str>` key — interns to u32.
    pub fn set_by_str(&mut self, s: Rc<str>, value: Value) {
        let sym = intern::intern_rc(&s);
        self.insert_pair(HashKey::Sym(sym), value);
    }

    /// Write directly to a known slot index. Only valid when the slot was
    /// previously validated via inline cache (same shape_version).
    /// Only updates `values[slot]`; `pairs` is NOT synced here for performance.
    /// Callers must ensure `sync_pairs_if_dirty()` is called before reading
    /// values from `pairs`.
    #[inline(always)]
    pub unsafe fn set_value_at_slot_unchecked(&mut self, slot: usize, value: Value) {
        *self.values.get_unchecked_mut(slot) = value;
        self.pairs_dirty = true;
    }

    /// Sync stale `pairs` values from `values` vec.
    /// Only needs to be called before operations that READ VALUES from `pairs`.
    #[inline(never)]
    pub fn sync_pairs(&mut self) {
        if !self.pairs_dirty {
            return;
        }
        for (i, (_, v)) in self.pairs.iter_mut().enumerate() {
            if i < self.values.len() {
                *v = self.values[i]; // Value is Copy
            }
        }
        self.pairs_dirty = false;
    }

    /// Conditionally sync pairs if dirty. Inline hint for the fast check.
    #[inline(always)]
    pub fn sync_pairs_if_dirty(&mut self) {
        if self.pairs_dirty {
            self.sync_pairs();
        }
    }

    /// Contains check by string key — interns and checks `str_slots`.
    pub fn contains_str(&self, s: &str) -> bool {
        let sym = intern::intern(s);
        self.str_slots.contains_key(&sym)
    }

    /// Check if this hash has any getter/setter accessors defined.
    #[inline(always)]
    pub fn has_accessors(&self) -> bool {
        self.getters.is_some() || self.setters.is_some()
    }

    /// Look up a getter by property name.
    #[inline]
    pub fn get_getter(&self, name: &str) -> Option<&CompiledFunctionObject> {
        self.getters.as_ref().and_then(|g| g.get(name))
    }

    /// Look up a setter by property name.
    #[inline]
    pub fn get_setter(&self, name: &str) -> Option<&CompiledFunctionObject> {
        self.setters.as_ref().and_then(|s| s.get(name))
    }

    /// Define a getter accessor on this object.
    pub fn define_getter(&mut self, name: String, func: CompiledFunctionObject) {
        self.getters
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
            .insert(name, func);
    }

    /// Define a setter accessor on this object.
    pub fn define_setter(&mut self, name: String, func: CompiledFunctionObject) {
        self.setters
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
            .insert(name, func);
    }

    /// Insert a key-value pair where the value is an Object. Primitives are
    /// converted to Value inline; heap types are stored in `local_objects`
    /// and referenced by index. Use this when no VM heap is available
    /// (e.g., compiler-constructed builtin globals).
    pub fn insert_pair_obj(&mut self, key: HashKey, obj: Object) {
        let val = match &obj {
            Object::Integer(v) => Value::from_i64(*v),
            Object::Float(v) => Value::from_f64(*v),
            Object::Boolean(v) => Value::from_bool(*v),
            Object::Null => Value::NULL,
            Object::Undefined => Value::UNDEFINED,
            _ => {
                let idx = self.local_objects.len() as u32;
                self.local_objects.push(obj);
                Value::from_heap(idx)
            }
        };
        self.insert_pair(key, val);
    }

    /// Migrate `local_objects` to the VM's main heap and remap all Value
    /// heap indices. Called once during VM constant loading. After this,
    /// `local_objects` is empty and all Values reference the VM heap.
    pub fn migrate_local_objects(&mut self, heap: &mut crate::value::Heap) {
        if self.local_objects.is_empty() {
            return;
        }
        // Track actual heap index for each local object (nested migrations may
        // insert additional objects, so a simple base+offset doesn't work).
        let mut index_map = Vec::with_capacity(self.local_objects.len());
        for obj in self.local_objects.drain(..) {
            // Recursively migrate nested hashes before allocating
            if let Object::Hash(ref inner_rc) = obj {
                inner_rc.borrow_mut().migrate_local_objects(heap);
            }
            let idx = heap.alloc(obj);
            index_map.push(idx);
        }
        for val in self.values.iter_mut() {
            if val.is_heap() {
                *val = Value::from_heap(index_map[val.heap_index() as usize]);
            }
        }
        for (_, val) in self.pairs.iter_mut() {
            if val.is_heap() {
                *val = Value::from_heap(index_map[val.heap_index() as usize]);
            }
        }
    }
}

impl Object {
    pub fn object_type(&self) -> ObjectType {
        match self {
            Object::Integer(_) => ObjectType::Integer,
            Object::Float(_) => ObjectType::Float,
            Object::Boolean(_) => ObjectType::Boolean,
            Object::Null => ObjectType::Null,
            Object::Undefined => ObjectType::Undefined,
            Object::String(_) => ObjectType::String,
            Object::RegExp(_) => ObjectType::Regexp,
            Object::Map(_) => ObjectType::Map,
            Object::Set(_) => ObjectType::Set,
            Object::Array(_) => ObjectType::Array,
            Object::Hash(_) => ObjectType::Hash,
            Object::CompiledFunction(_) => ObjectType::CompiledFunction,
            Object::Class(_) => ObjectType::Class,
            Object::BuiltinFunction(_) => ObjectType::Builtin,
            Object::Instance(_) => ObjectType::Instance,
            Object::BoundMethod(_) => ObjectType::BoundMethod,
            Object::SuperRef(_) => ObjectType::SuperRef,
            Object::Promise(_) => ObjectType::Promise,
            Object::Generator(_) => ObjectType::Generator,
            Object::Symbol(_, _) => ObjectType::Symbol,
            Object::ReturnValue(_) => ObjectType::ReturnValue,
            Object::Error(_) => ObjectType::Error,
        }
    }

    pub fn inspect(&self) -> String {
        match self {
            Object::Integer(v) => v.to_string(),
            Object::Float(v) => v.to_string(),
            Object::Boolean(v) => v.to_string(),
            Object::Null => "null".to_string(),
            Object::Undefined => "undefined".to_string(),
            Object::String(v) => v.to_string(),
            Object::RegExp(re) => format!("/{}/{}", re.pattern, re.flags),
            Object::Map(_) => "[Map]".to_string(),
            Object::Set(_) => "[Set]".to_string(),
            Object::Array(items) => {
                let borrowed = items.borrow();
                let joined = borrowed
                    .iter()
                    .map(|x| x.inspect_inline())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", joined)
            }
            Object::Hash(h) => {
                let h = h.borrow();
                let body = h
                    .pairs
                    .keys()
                    .enumerate()
                    .map(|(i, k)| {
                        let v = h
                            .values
                            .get(i)
                            .map(|v| v.inspect_inline())
                            .unwrap_or_default();
                        format!("{}: {}", k, v)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{}}}", body)
            }
            Object::CompiledFunction(_) => "[CompiledFunction]".to_string(),
            Object::Class(c) => format!("[Class {}]", c.name),
            Object::BuiltinFunction(_) => "[BuiltinFunction]".to_string(),
            Object::Instance(i) => format!("[Instance {}]", i.class_name),
            Object::BoundMethod(_) => "[BoundMethod]".to_string(),
            Object::SuperRef(_) => "[SuperRef]".to_string(),
            Object::Promise(p) => match &p.settled {
                PromiseState::Fulfilled(v) => format!("[Promise fulfilled {}]", v.inspect()),
                PromiseState::Rejected(v) => format!("[Promise rejected {}]", v.inspect()),
            },
            Object::Generator(_) => "[Generator]".to_string(),
            Object::Symbol(id, desc) => match desc {
                Some(d) => format!("Symbol({})", d),
                None => format!("Symbol({})", id),
            },
            Object::ReturnValue(v) => v.inspect(),
            Object::Error(err) => format!("{}: {}", err.name, err.message),
        }
    }

    /// JavaScript-style ToString conversion for string concatenation.
    /// Differs from inspect() for arrays (no brackets) and objects ("[object Object]").
    /// NOTE: Array items are Value (NaN-boxed) so we use inspect_inline() for them,
    /// which doesn't require heap access. Nested heap arrays will show as "[ref]"
    /// but this is acceptable for the common case.
    pub fn to_js_string(&self) -> String {
        match self {
            Object::Array(items) => {
                let borrowed = items.borrow();
                borrowed
                    .iter()
                    .map(|x| {
                        if x.is_undefined() {
                            String::new()
                        } else if x.is_null() {
                            String::new()
                        } else {
                            x.inspect_inline()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            }
            Object::Hash(_) | Object::Instance(_) => "[object Object]".to_string(),
            Object::CompiledFunction(_) | Object::BuiltinFunction(_) | Object::BoundMethod(_) => {
                "function () { [native code] }".to_string()
            }
            _ => self.inspect(),
        }
    }
}

pub fn true_object() -> Object {
    Object::Boolean(true)
}

pub fn false_object() -> Object {
    Object::Boolean(false)
}

pub fn null_object() -> Object {
    Object::Null
}

pub fn undefined_object() -> Object {
    Object::Undefined
}

/// Create an `Object::Array` with reference semantics (Rc<VmCell<Vec<Object>>>).
pub fn make_array(items: Vec<Value>) -> Object {
    Object::Array(Rc::new(VmCell::new(items)))
}

/// Convenience: create an empty `Object::Array`.
pub fn make_empty_array() -> Object {
    Object::Array(Rc::new(VmCell::new(Vec::new())))
}

/// Create an `Object::Hash` with reference semantics (Rc<VmCell<HashObject>>).
pub fn make_hash(hash: HashObject) -> Object {
    Object::Hash(Rc::new(VmCell::new(hash)))
}

/// Extract `Vec<Object>` from an `Rc<VmCell<Vec<Object>>>`, avoiding clone when refcount is 1.
pub fn unwrap_array(rc: Rc<VmCell<Vec<Value>>>) -> Vec<Value> {
    match Rc::try_unwrap(rc) {
        Ok(cell) => cell.into_inner(),
        Err(rc) => rc.borrow().clone(),
    }
}
