use formlogic_core::db_bridge::{DbBridge, DbRecord, DbSyncStatus};
use formlogic_core::engine::{FormLogicEngine, ScriptState};
use formlogic_core::local_storage::LocalStorageBridge;
use formlogic_core::object::{
    BuiltinFunction, BuiltinFunctionObject, HashKey, Object, PromiseState,
};
use formlogic_core::value::{obj_into_val, val_to_obj, Heap, Value};
use js_sys::{Array, Function, Object as JsObject, Reflect};
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// Install panic hook so panics are logged to the browser console with stack traces.
#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

// ============================================================================
// JS-backed DbBridge
// ============================================================================

/// Convert a JsValue directly to a serde_json::Value, avoiding V8's JSON.stringify.
/// Used by JsDbBridge to skip the JSON string intermediary when building DbRecords.
/// Returns Err if the depth limit is exceeded (likely circular reference or
/// excessively nested data) — callers should abort the DB transaction rather
/// than silently truncating user data to null.
fn jsvalue_to_serde_json(val: &JsValue, depth: usize) -> Result<serde_json::Value, String> {
    if val.is_null() || val.is_undefined() {
        return Ok(serde_json::Value::Null);
    }
    if let Some(b) = val.as_bool() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Some(n) = val.as_f64() {
        return Ok(serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null));
    }
    if let Some(s) = val.as_string() {
        return Ok(serde_json::Value::String(s));
    }
    if depth >= 32 {
        return Err(
            "Maximum JSON serialization depth (32) exceeded. Data may contain circular references or is too deeply nested.".to_string()
        );
    }
    if let Ok(arr) = val.clone().dyn_into::<Array>() {
        let mut vec = Vec::with_capacity(arr.length() as usize);
        for i in 0..arr.length() {
            vec.push(jsvalue_to_serde_json(&arr.get(i), depth + 1)?);
        }
        return Ok(serde_json::Value::Array(vec));
    }
    if val.is_object() {
        if let Ok(keys) = JsObject::keys(&JsObject::from(val.clone())).dyn_into::<Array>() {
            let mut map = serde_json::Map::new();
            let len = keys.length();
            if len > 500 {
                return Err(format!(
                    "Database record object exceeds maximum property limit (500). Found {} properties.",
                    len
                ));
            }
            for i in 0..len {
                if let Some(key) = keys.get(i).as_string() {
                    let child = Reflect::get(val, &JsValue::from_str(&key))
                        .unwrap_or(JsValue::UNDEFINED);
                    map.insert(key, jsvalue_to_serde_json(&child, depth + 1)?);
                }
            }
            return Ok(serde_json::Value::Object(map));
        }
    }
    Ok(serde_json::Value::Null)
}

/// A DbBridge implementation that delegates to a JavaScript object.
/// The JS object must have methods: query, create, update, hardDelete, get,
/// startSync, stopSync, getSyncStatus, getSavedSyncRoom, delete.
struct JsDbBridge {
    obj: JsValue,
}

impl JsDbBridge {
    fn call_method(&self, name: &str, args: &[JsValue]) -> JsValue {
        self.call_method_inner(name, args, false)
    }

    /// Like `call_method`, but propagates errors via `wasm_bindgen::throw_str`
    /// so that write failures surface to the VM instead of silently losing data.
    fn call_method_write(&self, name: &str, args: &[JsValue]) -> JsValue {
        self.call_method_inner(name, args, true)
    }

    fn call_method_inner(&self, name: &str, args: &[JsValue], throw_on_error: bool) -> JsValue {
        let func = Reflect::get(&self.obj, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED);
        if let Ok(f) = func.dyn_into::<Function>() {
            let result = match args.len() {
                0 => f.call0(&self.obj),
                1 => f.call1(&self.obj, &args[0]),
                2 => f.call2(&self.obj, &args[0], &args[1]),
                3 => f.call3(&self.obj, &args[0], &args[1], &args[2]),
                _ => f.call0(&self.obj),
            };
            match result {
                Ok(v) => v,
                Err(e) => {
                    let msg = format!("[FormLogic DB Bridge] {}: {:?}", name, e);
                    web_sys::console::error_1(&JsValue::from_str(&msg));
                    if throw_on_error {
                        wasm_bindgen::throw_str(&msg);
                    }
                    JsValue::UNDEFINED
                }
            }
        } else {
            JsValue::UNDEFINED
        }
    }

    fn jsvalue_to_record(&self, val: &JsValue) -> Option<DbRecord> {
        if val.is_undefined() || val.is_null() {
            return None;
        }
        let id = Reflect::get(val, &JsValue::from_str("id"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let collection = Reflect::get(val, &JsValue::from_str("collection"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let created_at = Reflect::get(val, &JsValue::from_str("created_at"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        let updated_at = Reflect::get(val, &JsValue::from_str("updated_at"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        // Convert data field directly to serde_json::Value, skipping V8's
        // JSON.stringify. The VM's db_record_to_object will use data_parsed
        // directly, avoiding a redundant stringify → from_str round-trip.
        let data_val = Reflect::get(val, &JsValue::from_str("data")).unwrap_or(JsValue::UNDEFINED);
        let data_parsed = if data_val.is_undefined() || data_val.is_null() {
            None
        } else {
            match jsvalue_to_serde_json(&data_val, 0) {
                Ok(v) => Some(v),
                Err(e) => {
                    web_sys::console::error_1(&JsValue::from_str(
                        &format!("[FormLogic DB] Serialization failed, aborting record: {}", e),
                    ));
                    return None;
                }
            }
        };
        Some(DbRecord {
            id,
            collection,
            data: String::new(),
            created_at,
            updated_at,
            data_parsed,
        })
    }
}

impl DbBridge for JsDbBridge {
    fn query(&self, collection: &str) -> Result<Vec<DbRecord>, String> {
        let result = self.call_method("query", &[JsValue::from_str(collection)]);
        let mut records = Vec::new();
        if let Ok(arr) = result.dyn_into::<Array>() {
            for i in 0..arr.length() {
                let item = arr.get(i);
                if let Some(record) = self.jsvalue_to_record(&item) {
                    records.push(record);
                }
            }
        }
        Ok(records)
    }

    fn create(&mut self, collection: &str, data: &str) -> Result<DbRecord, String> {
        // Parse the JSON string back to a JS object for the JS bridge
        let data_js = js_sys::JSON::parse(data).unwrap_or(JsValue::UNDEFINED);
        let result = self.call_method_write("create", &[JsValue::from_str(collection), data_js]);
        self.jsvalue_to_record(&result).ok_or_else(|| "db.create returned null".to_string())
    }

    fn update(&mut self, id: &str, data: &str) -> Result<Option<DbRecord>, String> {
        let data_js = js_sys::JSON::parse(data).unwrap_or(JsValue::UNDEFINED);
        let result = self.call_method_write("update", &[JsValue::from_str(id), data_js]);
        Ok(self.jsvalue_to_record(&result))
    }

    fn delete(&mut self, id: &str) -> Result<(), String> {
        self.call_method_write("delete", &[JsValue::from_str(id)]);
        Ok(())
    }

    fn hard_delete(&mut self, collection: &str, id: &str) -> Result<(), String> {
        self.call_method_write(
            "hardDelete",
            &[JsValue::from_str(collection), JsValue::from_str(id)],
        );
        Ok(())
    }

    fn get(&self, collection: &str, id: &str) -> Result<Option<DbRecord>, String> {
        let result = self.call_method(
            "get",
            &[JsValue::from_str(collection), JsValue::from_str(id)],
        );
        Ok(self.jsvalue_to_record(&result))
    }

    fn start_sync(&mut self, room: &str) {
        self.call_method("startSync", &[JsValue::from_str(room)]);
    }

    fn stop_sync(&mut self, room: Option<&str>) {
        let arg = room.map(JsValue::from_str).unwrap_or(JsValue::UNDEFINED);
        self.call_method("stopSync", &[arg]);
    }

    fn get_sync_status(&self, room: Option<&str>) -> DbSyncStatus {
        let arg = room.map(JsValue::from_str).unwrap_or(JsValue::UNDEFINED);
        let result = self.call_method("getSyncStatus", &[arg]);
        if result.is_undefined() || result.is_null() {
            return DbSyncStatus {
                connected: false,
                peers: 0,
                room: String::new(),
            };
        }
        let connected = Reflect::get(&result, &JsValue::from_str("connected"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let peers = Reflect::get(&result, &JsValue::from_str("peers"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as usize;
        let room_str = Reflect::get(&result, &JsValue::from_str("room"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        DbSyncStatus {
            connected,
            peers,
            room: room_str,
        }
    }

    fn get_saved_sync_room(&self) -> Option<String> {
        let result = self.call_method("getSavedSyncRoom", &[]);
        if result.is_undefined() || result.is_null() {
            None
        } else {
            result.as_string()
        }
    }
}

// ============================================================================
// JS-backed LocalStorageBridge
// ============================================================================

struct JsLocalStorageBridge {
    obj: JsValue,
}

impl JsLocalStorageBridge {
    fn call_method(&self, name: &str, args: &[JsValue]) -> JsValue {
        let func = Reflect::get(&self.obj, &JsValue::from_str(name)).unwrap_or(JsValue::UNDEFINED);
        if let Ok(f) = func.dyn_into::<Function>() {
            match args.len() {
                0 => f.call0(&self.obj).unwrap_or(JsValue::UNDEFINED),
                1 => f.call1(&self.obj, &args[0]).unwrap_or(JsValue::UNDEFINED),
                2 => f.call2(&self.obj, &args[0], &args[1]).unwrap_or(JsValue::UNDEFINED),
                _ => f.call0(&self.obj).unwrap_or(JsValue::UNDEFINED),
            }
        } else {
            JsValue::UNDEFINED
        }
    }
}

impl LocalStorageBridge for JsLocalStorageBridge {
    fn get_item(&self, key: &str) -> Option<String> {
        let result = self.call_method("getItem", &[JsValue::from_str(key)]);
        if result.is_undefined() || result.is_null() {
            None
        } else {
            result.as_string()
        }
    }

    fn set_item(&mut self, key: &str, value: &str) {
        self.call_method(
            "setItem",
            &[JsValue::from_str(key), JsValue::from_str(value)],
        );
    }

    fn remove_item(&mut self, key: &str) {
        self.call_method("removeItem", &[JsValue::from_str(key)]);
    }

    fn clear(&mut self) {
        self.call_method("clear", &[]);
    }
}

// ============================================================================
// JS→Object conversion
// ============================================================================

/// Unified maximum recursion depth for JS↔Object conversions.
/// Both directions use the same limit to prevent asymmetric depth mismatches
/// where objects created in one direction cannot survive a round-trip.
const MAX_SANDBOX_DEPTH: usize = 10;

/// Check if a JS object is a browser/DOM object that should not be deeply converted.
/// DOM Events, Nodes, and Window have deep/circular structures that cause exponential blowup.
fn is_browser_object(val: &JsValue) -> bool {
    // Functions should not be converted to hashes
    if val.is_function() {
        return true;
    }
    // DOM Node: has numeric nodeType property
    if let Ok(nt) = Reflect::get(val, &JsValue::from_str("nodeType")) {
        if nt.as_f64().is_some() {
            return true;
        }
    }
    // DOM Event: has bubbles property
    if let Ok(b) = Reflect::get(val, &JsValue::from_str("bubbles")) {
        if b.as_bool().is_some() {
            return true;
        }
    }
    // Window: has document and location
    if Reflect::has(val, &JsValue::from_str("document")).unwrap_or(false)
        && Reflect::has(val, &JsValue::from_str("location")).unwrap_or(false)
        && Reflect::has(val, &JsValue::from_str("navigator")).unwrap_or(false)
    {
        return true;
    }
    false
}

/// Returns true if the Object type can survive a JS round-trip.
/// Types that `object_to_jsvalue_depth` converts to JsValue::NULL are NOT
/// serializable and must be protected from overwrite during state sync.
fn is_js_serializable(obj: &Object) -> bool {
    !matches!(
        obj,
        Object::Class(_)
            | Object::CompiledFunction(_)
            | Object::BoundMethod(_)
            | Object::BuiltinFunction(_)
            | Object::Instance(_)
            | Object::Generator(_)
            | Object::SuperRef(_)
    )
}

/// Convert a JsValue to a FormLogic Object, allocating on the given heap.
fn jsvalue_to_object(val: &JsValue, heap: &mut Heap, depth: usize) -> Object {
    if val.is_null() {
        return Object::Null;
    }
    if val.is_undefined() {
        return Object::Undefined;
    }
    if let Some(b) = val.as_bool() {
        return Object::Boolean(b);
    }
    if let Some(n) = val.as_f64() {
        if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
            return Object::Integer(n as i64);
        }
        return Object::Float(n);
    }
    if let Some(s) = val.as_string() {
        return Object::String(s.into());
    }
    if depth >= MAX_SANDBOX_DEPTH {
        return Object::Null;
    }
    // Skip browser/DOM objects — they have deep/circular structures
    if is_browser_object(val) {
        return Object::Null;
    }
    // Array
    if let Ok(arr) = val.clone().dyn_into::<Array>() {
        let items: Vec<formlogic_core::value::Value> = (0..arr.length())
            .map(|i| {
                let child = jsvalue_to_object(&arr.get(i), heap, depth + 1);
                obj_into_val(child, heap)
            })
            .collect();
        let arr_rc = formlogic_core::object::VmCell::new(items);
        return Object::Array(Rc::new(arr_rc));
    }
    // Plain object
    if val.is_object() {
        let js_obj: &JsValue = val;
        if let Ok(keys) = JsObject::keys(&JsObject::from(js_obj.clone())).dyn_into::<Array>() {
            let mut hash = formlogic_core::object::HashObject::default();
            let len = keys.length();
            for i in 0..len {
                if let Some(key) = keys.get(i).as_string() {
                    if key == "__proto__" || key == "constructor" || key == "prototype" {
                        continue;
                    }
                    let child_val =
                        Reflect::get(js_obj, &JsValue::from_str(&key)).unwrap_or(JsValue::UNDEFINED);
                    let child_obj = jsvalue_to_object(&child_val, heap, depth + 1);
                    let child = obj_into_val(child_obj, heap);
                    hash.insert_pair(
                        formlogic_core::object::HashKey::from_owned_string(key),
                        child,
                    );
                }
            }
            let hash_rc = formlogic_core::object::VmCell::new(hash);
            return Object::Hash(Rc::new(hash_rc));
        }
    }
    Object::Null
}

// ============================================================================
// WasmFormLogicEngine
// ============================================================================

#[wasm_bindgen]
pub struct WasmFormLogicEngine {
    inner: FormLogicEngine,
    state: Option<ScriptState>,
    db_bridge_js: Option<JsValue>,
    ls_bridge_js: Option<JsValue>,
}

#[wasm_bindgen]
impl WasmFormLogicEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmFormLogicEngine {
        WasmFormLogicEngine {
            inner: FormLogicEngine::default(),
            state: None,
            db_bridge_js: None,
            ls_bridge_js: None,
        }
    }

    pub fn eval(&self, source: &str) -> Result<JsValue, JsValue> {
        let out = self.inner.eval(source).map_err(|e| JsValue::from_str(&e))?;
        Ok(object_to_jsvalue(&out))
    }

    #[wasm_bindgen(js_name = evalInspect)]
    pub fn eval_inspect(&self, source: &str) -> Result<String, JsValue> {
        let out = self.inner.eval(source).map_err(|e| JsValue::from_str(&e))?;
        Ok(out.inspect())
    }

    /// Store a JS database bridge object for later use during init_script.
    #[wasm_bindgen(js_name = setDbBridge)]
    pub fn set_db_bridge(&mut self, bridge: JsValue) {
        self.db_bridge_js = Some(bridge);
    }

    /// Store a JS localStorage bridge object for later use during init_script.
    #[wasm_bindgen(js_name = setLocalStorageBridge)]
    pub fn set_local_storage_bridge(&mut self, bridge: JsValue) {
        self.ls_bridge_js = Some(bridge);
    }

    /// Compile, set up bridges, and run the script.
    /// Returns the symbol map as a JS object: { name: { index, scope } }.
    #[wasm_bindgen(js_name = initScript)]
    pub fn init_script(&mut self, source: &str) -> Result<JsValue, JsValue> {
        // 1. Compile (no execution yet)
        let mut script_state = self
            .inner
            .compile_script(source)
            .map_err(|e| JsValue::from_str(&e))?;

        // 2. Attach bridges before running top-level code
        if let Some(db_js) = self.db_bridge_js.take() {
            script_state.set_db(Box::new(JsDbBridge { obj: db_js }));
        }
        if let Some(ls_js) = self.ls_bridge_js.take() {
            script_state.set_local_storage(Box::new(JsLocalStorageBridge { obj: ls_js }));
        }

        // 3. Run top-level code (executes `let window = {};` etc.)
        script_state
            .run_init()
            .map_err(|e| JsValue::from_str(&e))?;

        // 3b. Inject addEventListener/removeEventListener on the window global
        //     AFTER run_init so that `let window = {};` has been executed.
        //     _init() is called later by the JS runtime, so these are available.
        if let Ok(Object::Hash(window_hash)) = script_state.get_global("window") {
            let h = window_hash.borrow_mut();

            let add_fn = Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::WindowAddEventListener,
                receiver: None,
            }));
            let add_val = obj_into_val(add_fn, script_state.heap_mut());
            let sym_add = formlogic_core::intern::intern("addEventListener");
            h.insert_pair(HashKey::Sym(sym_add), add_val);

            let rem_fn = Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::WindowRemoveEventListener,
                receiver: None,
            }));
            let rem_val = obj_into_val(rem_fn, script_state.heap_mut());
            let sym_rem = formlogic_core::intern::intern("removeEventListener");
            h.insert_pair(HashKey::Sym(sym_rem), rem_val);
        }

        // 4. Build symbol map
        let symbol_map = self.build_symbol_map(&script_state);

        self.state = Some(script_state);

        Ok(symbol_map)
    }

    /// Get a global variable by its slot index. Returns a JS value.
    #[wasm_bindgen(js_name = getGlobalByIndex)]
    pub fn get_global_by_index(&self, index: u32) -> JsValue {
        if let Some(ref state) = self.state {
            let obj = state.get_global_by_index(index as u16);
            object_to_jsvalue_with_heap(&obj, &state.vm().heap)
        } else {
            JsValue::UNDEFINED
        }
    }

    /// Set a global variable by its slot index.
    /// Non-serializable types cannot survive a JS round-trip (object_to_jsvalue
    /// converts them to JsValue::NULL), so we skip the write to preserve the VM value.
    #[wasm_bindgen(js_name = setGlobalByIndex)]
    pub fn set_global_by_index(&mut self, index: u32, value: JsValue) {
        if let Some(ref mut state) = self.state {
            let current = state.get_global_by_index(index as u16);
            if !is_js_serializable(&current) {
                return;
            }
            let obj = jsvalue_to_object(&value, state.heap_mut(), 0);
            state.set_global_by_index(index as u16, obj);
        }
    }

    /// Call a named function with JS array arguments. Returns the result as a JS value.
    #[wasm_bindgen(js_name = callFunction)]
    pub fn call_function(&mut self, name: &str, args: JsValue) -> Result<JsValue, JsValue> {
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No script loaded"))?;

        // Convert JS args array to Vec<Object>
        let vm_args = if args.is_undefined() || args.is_null() {
            vec![]
        } else if let Ok(arr) = args.dyn_into::<Array>() {
            (0..arr.length())
                .map(|i| jsvalue_to_object(&arr.get(i), state.heap_mut(), 0))
                .collect()
        } else {
            vec![]
        };

        let result = state
            .call_function(name, &vm_args)
            .map_err(|e| JsValue::from_str(&e))?;

        Ok(object_to_jsvalue_with_heap(&result, &state.vm().heap))
    }

    /// Get the symbol map as a JS object.
    #[wasm_bindgen(js_name = getSymbolMap)]
    pub fn get_symbol_map(&self) -> JsValue {
        if let Some(ref state) = self.state {
            self.build_symbol_map(state)
        } else {
            JsObject::new().into()
        }
    }

    /// Get multiple globals at once, returning a JS array of values.
    /// `indices` is a JS array of u32 slot indices.
    /// Returns a JS array of the same length with the corresponding values.
    #[wasm_bindgen(js_name = getGlobalsBatch)]
    pub fn get_globals_batch(&self, indices: JsValue) -> JsValue {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Array::new().into(),
        };
        let idx_arr = match indices.dyn_into::<Array>() {
            Ok(a) => a,
            Err(_) => return Array::new().into(),
        };
        let heap = &state.vm().heap;
        let result = Array::new_with_length(idx_arr.length());
        for i in 0..idx_arr.length() {
            let index = idx_arr.get(i).as_f64().unwrap_or(0.0) as u16;
            let obj = state.get_global_by_index(index);
            result.set(i, object_to_jsvalue_with_heap(&obj, heap));
        }
        result.into()
    }

    /// Set multiple globals at once.
    /// `indices` is a JS array of u32 slot indices.
    /// `values` is a JS array of the same length with corresponding values.
    /// Non-serializable types (Class, CompiledFunction, BoundMethod, BuiltinFunction)
    /// are protected from overwrite, same as setGlobalByIndex.
    #[wasm_bindgen(js_name = setGlobalsBatch)]
    pub fn set_globals_batch(&mut self, indices: JsValue, values: JsValue) {
        let state = match self.state.as_mut() {
            Some(s) => s,
            None => return,
        };
        let idx_arr = match indices.dyn_into::<Array>() {
            Ok(a) => a,
            Err(_) => return,
        };
        let val_arr = match values.dyn_into::<Array>() {
            Ok(a) => a,
            Err(_) => return,
        };
        let len = std::cmp::min(idx_arr.length(), val_arr.length());
        for i in 0..len {
            let index = idx_arr.get(i).as_f64().unwrap_or(0.0) as u16;
            let current = state.get_global_by_index(index);
            if !is_js_serializable(&current) {
                continue;
            }
            let val_js = val_arr.get(i);
            let obj = jsvalue_to_object(&val_js, state.heap_mut(), 0);
            state.set_global_by_index(index, obj);
        }
    }

    /// Evaluate an expression in the script's global context.
    /// The expression has access to all script variables and functions.
    #[wasm_bindgen(js_name = evalInContext)]
    pub fn eval_in_context(&mut self, expr: &str) -> Result<JsValue, JsValue> {
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No script loaded"))?;
        let out = state
            .eval_in_context(expr)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(object_to_jsvalue_with_heap(&out, &state.vm().heap))
    }

    /// Given an array of global slot indices, return an array of only those
    /// that have been written since the last `clearDirty()` call.
    /// Used by the JS runtime to skip deepEqual on unchanged state variables.
    #[wasm_bindgen(js_name = getDirtyGlobals)]
    pub fn get_dirty_globals(&self, indices: JsValue) -> JsValue {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Array::new().into(),
        };
        let idx_arr = match indices.dyn_into::<Array>() {
            Ok(a) => a,
            Err(_) => return Array::new().into(),
        };
        let result = Array::new();
        for i in 0..idx_arr.length() {
            let index = idx_arr.get(i).as_f64().unwrap_or(0.0) as u16;
            if state.is_global_dirty(index) {
                result.push(&JsValue::from_f64(index as f64));
            }
        }
        result.into()
    }

    /// Clear all dirty bits. Call after syncing VM state to React.
    #[wasm_bindgen(js_name = clearDirty)]
    pub fn clear_dirty(&self) {
        if let Some(ref state) = self.state {
            state.clear_dirty();
        }
    }

    /// Return an array of event type strings that have registered listeners.
    /// E.g. ["keydown", "keyup", "blur"]
    #[wasm_bindgen(js_name = getEventListenerTypes)]
    pub fn get_event_listener_types(&self) -> JsValue {
        let state = match self.state.as_ref() {
            Some(s) => s,
            None => return Array::new().into(),
        };
        let arr = Array::new();
        for key in state.vm().event_listeners.keys() {
            arr.push(&JsValue::from_str(key));
        }
        arr.into()
    }

    /// Dispatch an event to all registered handlers for the given event type.
    /// `event_type` is e.g. "keydown". `event_obj` is a JS object that will be
    /// passed as the first argument to each handler (converted to a VM Hash).
    /// Returns the number of handlers invoked.
    #[wasm_bindgen(js_name = dispatchEvent)]
    pub fn dispatch_event(&mut self, event_type: &str, event_obj: JsValue) -> Result<u32, JsValue> {
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| JsValue::from_str("No script loaded"))?;

        // Get handler Values for this event type
        let handlers: Vec<Value> = match state.vm().event_listeners.get(event_type) {
            Some(h) => h.clone(),
            None => return Ok(0),
        };

        // Convert the JS event object to a VM Object
        let event_arg = jsvalue_to_object(&event_obj, state.heap_mut(), 0);

        // Inject preventDefault() and stopPropagation() as no-op builtins so
        // .logic handlers can call e.preventDefault() without error.
        if let Object::Hash(ref hash_rc) = event_arg {
            let pd = Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::EventPreventDefault,
                receiver: None,
            }));
            let sp = Object::BuiltinFunction(Box::new(BuiltinFunctionObject {
                function: BuiltinFunction::EventStopPropagation,
                receiver: None,
            }));
            let pd_val = obj_into_val(pd, state.heap_mut());
            let sp_val = obj_into_val(sp, state.heap_mut());
            let hash = hash_rc.borrow_mut();
            hash.insert_pair(
                HashKey::from_owned_string("preventDefault".to_string()),
                pd_val,
            );
            hash.insert_pair(
                HashKey::from_owned_string("stopPropagation".to_string()),
                sp_val,
            );
        }

        let mut count = 0u32;
        for handler in &handlers {
            match state.call_value(*handler, &[event_arg.clone()]) {
                Ok(_) => { count += 1; }
                Err(e) => {
                    // Log the error so developers can debug their .logic scripts,
                    // but don't break the event loop — other handlers should still run.
                    let err_msg = format!("[FormLogic Event Error] handler for '{}': {:?}", event_type, e);
                    web_sys::console::error_1(&JsValue::from_str(&err_msg));
                }
            }
        }

        Ok(count)
    }
    /// Drain all pending host calls queued by `host.call()` during VM execution.
    /// Returns a JS array of `{ id: number, kind: string, args: string[] }`.
    #[wasm_bindgen(js_name = drainPendingHostCalls)]
    pub fn drain_pending_host_calls(&mut self) -> JsValue {
        let state = match self.state.as_mut() {
            Some(s) => s,
            None => return Array::new().into(),
        };
        let calls = state.drain_pending_host_calls();
        let arr = Array::new();
        for call in calls {
            let obj = JsObject::new();
            let _ = Reflect::set(&obj, &JsValue::from_str("id"), &JsValue::from_f64(call.id as f64));
            let _ = Reflect::set(&obj, &JsValue::from_str("kind"), &JsValue::from_str(&call.kind));
            let args_arr = Array::new();
            for arg in &call.args {
                args_arr.push(&JsValue::from_str(arg));
            }
            let _ = Reflect::set(&obj, &JsValue::from_str("args"), &args_arr.into());
            arr.push(&obj.into());
        }
        arr.into()
    }

    /// Resolve a pending host callback by its call ID. The `result` JsValue is
    /// converted to a VM Object and passed to the stored callback function.
    #[wasm_bindgen(js_name = resolveHostCallback)]
    pub fn resolve_host_callback(&mut self, call_id: u32, result: JsValue) -> Result<(), JsValue> {
        let state = self.state.as_mut()
            .ok_or_else(|| JsValue::from_str("No script loaded"))?;
        let obj = jsvalue_to_object(&result, state.heap_mut(), 0);
        state.resolve_host_callback(call_id, obj)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(())
    }
}

impl WasmFormLogicEngine {
    fn build_symbol_map(&self, state: &ScriptState) -> JsValue {
        let result = JsObject::new();
        for (name, &slot) in state.globals_table() {
            let entry = JsObject::new();
            let _ = Reflect::set(
                &entry,
                &JsValue::from_str("index"),
                &JsValue::from_f64(slot as f64),
            );
            // Determine scope: check if the value at this slot is a function
            let obj = state.get_global_by_index(slot);
            let scope = match obj {
                Object::CompiledFunction(_)
                | Object::Class(_)
                | Object::BoundMethod(_)
                | Object::BuiltinFunction(_) => "function",
                _ => "variable",
            };
            let _ = Reflect::set(
                &entry,
                &JsValue::from_str("scope"),
                &JsValue::from_str(scope),
            );
            let _ = Reflect::set(&result, &JsValue::from_str(name), &entry);
        }
        result.into()
    }
}

/// Detect host bridge usage in .logic source code using the Rust lexer.
/// Returns a JS array of reason strings (e.g. ["uses db bridge", "uses window bridge"]).
/// Uses proper lexical analysis instead of brittle regex, correctly handling
/// comments, string literals, template literals, and regex literals.
#[wasm_bindgen(js_name = detectHostBridges)]
pub fn detect_host_bridges(source: &str) -> JsValue {
    use formlogic_core::config::FormLogicConfig;
    use formlogic_core::lexer::Lexer;
    use formlogic_core::token::TokenType;

    let config = FormLogicConfig::default();
    let mut lexer = Lexer::new(source, config);
    let mut reasons: Vec<&str> = Vec::new();
    let mut found_db = false;
    let mut found_window = false;
    let mut found_navigator = false;
    let mut found_local_storage = false;
    let mut found_host = false;

    // Buffer: keep track of the previous token to detect `identifier.` patterns
    let mut prev_literal = String::new();
    let mut prev_was_ident = false;

    loop {
        let tok = lexer.next_token();
        if tok.token_type == TokenType::Eof {
            break;
        }

        if tok.token_type == TokenType::Dot && prev_was_ident {
            match prev_literal.as_str() {
                "db" if !found_db => {
                    found_db = true;
                    reasons.push("uses db bridge (synchronous host access)");
                }
                "window" if !found_window => {
                    found_window = true;
                    reasons.push("uses window bridge/event APIs");
                }
                "navigator" if !found_navigator => {
                    found_navigator = true;
                    reasons.push("uses navigator bridge APIs");
                }
                "localStorage" if !found_local_storage => {
                    found_local_storage = true;
                    reasons.push("uses localStorage bridge");
                }
                "host" if !found_host => {
                    found_host = true;
                    reasons.push("uses host.call bridge (async host access)");
                }
                _ => {}
            }
        }

        prev_was_ident = tok.token_type == TokenType::Ident;
        if prev_was_ident {
            prev_literal = tok.literal;
        }

        // Early exit if all bridges found
        if found_db && found_window && found_navigator && found_local_storage && found_host {
            break;
        }
    }

    let arr = Array::new();
    for reason in reasons {
        arr.push(&JsValue::from_str(reason));
    }
    arr.into()
}

#[wasm_bindgen]
pub fn wasm_engine_info() -> String {
    "formlogic-wasm 0.1.0".to_string()
}

impl Default for WasmFormLogicEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Object → JsValue conversion
// ============================================================================

/// Re-use the unified sandbox depth for Object → JsValue conversion.
const MAX_OBJ_DEPTH: usize = MAX_SANDBOX_DEPTH;

fn object_to_jsvalue(value: &Object) -> JsValue {
    let heap = Heap::default();
    object_to_jsvalue_depth(value, &heap, 0)
}

fn object_to_jsvalue_with_heap(value: &Object, heap: &Heap) -> JsValue {
    object_to_jsvalue_depth(value, heap, 0)
}

fn object_to_jsvalue_depth(value: &Object, heap: &Heap, depth: usize) -> JsValue {
    match value {
        Object::Integer(v) => JsValue::from_f64(*v as f64),
        Object::Float(v) => JsValue::from_f64(*v),
        Object::Boolean(v) => JsValue::from_bool(*v),
        Object::Null => JsValue::NULL,
        Object::Undefined => JsValue::UNDEFINED,
        Object::String(v) => JsValue::from_str(&v),
        Object::Array(items) => {
            if depth >= MAX_OBJ_DEPTH {
                return JsValue::NULL;
            }
            let arr = Array::new();
            for item in items.borrow().iter() {
                let obj = val_to_obj(*item, heap);
                arr.push(&object_to_jsvalue_depth(&obj, heap, depth + 1));
            }
            arr.into()
        }
        Object::Hash(hash) => {
            if depth >= MAX_OBJ_DEPTH {
                return JsValue::NULL;
            }
            let obj = JsObject::new();
            let h = hash.borrow_mut();
            h.sync_pairs_if_dirty();
            for (k, v) in &h.pairs {
                let v_obj = val_to_obj(*v, heap);
                let _ = Reflect::set(
                    &obj,
                    &JsValue::from_str(&k.display_key()),
                    &object_to_jsvalue_depth(&v_obj, heap, depth + 1),
                );
            }
            obj.into()
        }
        Object::Promise(p) => {
            let obj = JsObject::new();
            match &p.settled {
                PromiseState::Fulfilled(v) => {
                    let _ = Reflect::set(
                        &obj,
                        &JsValue::from_str("status"),
                        &JsValue::from_str("fulfilled"),
                    );
                    let _ = Reflect::set(
                        &obj,
                        &JsValue::from_str("value"),
                        &object_to_jsvalue_depth(v, heap, depth + 1),
                    );
                }
                PromiseState::Rejected(v) => {
                    let _ = Reflect::set(
                        &obj,
                        &JsValue::from_str("status"),
                        &JsValue::from_str("rejected"),
                    );
                    let _ = Reflect::set(
                        &obj,
                        &JsValue::from_str("reason"),
                        &object_to_jsvalue_depth(v, heap, depth + 1),
                    );
                }
            }
            obj.into()
        }
        Object::Error(err) => {
            let obj = JsObject::new();
            let _ = Reflect::set(
                &obj,
                &JsValue::from_str("name"),
                &JsValue::from_str(&err.name),
            );
            let _ = Reflect::set(
                &obj,
                &JsValue::from_str("message"),
                &JsValue::from_str(&err.message),
            );
            obj.into()
        }
        Object::ReturnValue(v) => object_to_jsvalue_depth(v, heap, depth),
        // Non-data types — return null instead of calling inspect() which could be expensive
        Object::CompiledFunction(_) => JsValue::NULL,
        _ => JsValue::NULL,
    }
}
