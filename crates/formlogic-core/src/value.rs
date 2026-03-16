use rustc_hash::FxHashMap;
use std::rc::Rc;

use crate::object::Object;

// ── NaN-boxing layout ───────────────────────────────────────────────────
//
// IEEE-754 double:  sign(1) exponent(11) mantissa(52)
// A NaN has exponent = 0x7FF and mantissa != 0.
// We use the "quiet NaN" space (bit 51 set) plus bits 48-50 for tags.
//
// Tag bits [50:48] inside the mantissa of a quiet NaN:
//   000  = actual f64 (not a NaN-boxed value; passes is_f64 check)
//   001  = i32       payload = bits [31:0] (sign-extended in payload)
//   010  = bool      payload bit 0: 0=false, 1=true
//   011  = null
//   100  = undefined
//   101  = symbol    payload = u32 symbol id
//   110  = heap ptr  payload = u32 heap index
//   111  = (reserved)
//
// Quiet NaN prefix: 0x7FF8_0000_0000_0000
// Tag mask:  bits [50:48] → shift right 48, mask 0x7

const QNAN: u64 = 0x7FF8_0000_0000_0000;
const TAG_SHIFT: u64 = 48;
const TAG_MASK: u64 = 0x7;
#[allow(dead_code)]
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF; // lower 48 bits

const TAG_I32: u64 = 1;
const TAG_BOOL: u64 = 2;
const TAG_NULL: u64 = 3;
const TAG_UNDEFINED: u64 = 4;
const TAG_SYMBOL: u64 = 5;
const TAG_HEAP: u64 = 6;
const TAG_ISTR: u64 = 7; // inline string (≤5 bytes stored directly in payload)

/// A NaN-boxed value: 8 bytes, Copy, stores either an f64 inline or a
/// tagged payload (i32, bool, null, undefined, symbol id, heap index).
#[derive(Clone, Copy)]
pub struct Value(u64);

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_f64() {
            write!(f, "Value(f64({}))", self.as_f64())
        } else if self.is_i32() {
            write!(f, "Value(i32({}))", unsafe { self.as_i32_unchecked() })
        } else if self.is_bool() {
            write!(f, "Value(bool({}))", unsafe { self.as_bool_unchecked() })
        } else if self.is_null() {
            write!(f, "Value(null)")
        } else if self.is_undefined() {
            write!(f, "Value(undefined)")
        } else if self.is_symbol() {
            write!(f, "Value(symbol({}))", self.as_symbol())
        } else if self.is_heap() {
            write!(f, "Value(heap({}))", self.heap_index())
        } else if self.is_inline_str() {
            let (buf, len) = self.inline_str_buf();
            let s = std::str::from_utf8(&buf[..len]).unwrap_or("<invalid>");
            write!(f, "Value(istr({:?}))", s)
        } else {
            write!(f, "Value(unknown({:#018x}))", self.0)
        }
    }
}

impl PartialEq for Value {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        // Bit-exact equality. For f64 NaN != NaN (correct for strict equal).
        // For boxed values the bits encode identity.
        self.0 == other.0
    }
}

impl Eq for Value {}

impl Value {
    pub const NULL: Value = Value(QNAN | (TAG_NULL << TAG_SHIFT));
    pub const UNDEFINED: Value = Value(QNAN | (TAG_UNDEFINED << TAG_SHIFT));
    pub const TRUE: Value = Value(QNAN | (TAG_BOOL << TAG_SHIFT) | 1);
    pub const FALSE: Value = Value(QNAN | (TAG_BOOL << TAG_SHIFT));

    #[inline(always)]
    pub fn bits(self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub fn from_bits(bits: u64) -> Self {
        Value(bits)
    }

    // ── Constructors ────────────────────────────────────────────────────

    #[inline(always)]
    pub fn from_f64(v: f64) -> Self {
        Value(v.to_bits())
    }

    #[inline(always)]
    pub fn from_i32(v: i32) -> Self {
        // Store the i32 as a u32 in the lower 32 bits of the payload.
        Value(QNAN | (TAG_I32 << TAG_SHIFT) | (v as u32 as u64))
    }

    /// Convert an i64 to a Value. If it fits in i32 range, store as i32 tag.
    /// Otherwise store as f64.
    #[inline(always)]
    pub fn from_i64(v: i64) -> Self {
        if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
            Self::from_i32(v as i32)
        } else {
            Self::from_f64(v as f64)
        }
    }

    #[inline(always)]
    pub fn from_bool(v: bool) -> Self {
        if v {
            Self::TRUE
        } else {
            Self::FALSE
        }
    }

    #[inline(always)]
    pub fn from_symbol(id: u32) -> Self {
        Value(QNAN | (TAG_SYMBOL << TAG_SHIFT) | id as u64)
    }

    #[inline(always)]
    pub fn from_heap(index: u32) -> Self {
        Value(QNAN | (TAG_HEAP << TAG_SHIFT) | index as u64)
    }

    /// Create an inline string Value from bytes. Returns None if > 5 bytes.
    /// Layout: tag=7, bits[47:40]=length, bits[39:0]=bytes (big-endian order).
    #[inline(always)]
    pub fn from_inline_str(s: &[u8]) -> Option<Value> {
        let len = s.len();
        if len > 5 {
            return None;
        }
        let mut payload: u64 = (len as u64) << 40;
        // Pack bytes: byte[0] at bits 39:32, byte[1] at bits 31:24, etc.
        let mut i = 0;
        while i < len {
            payload |= (s[i] as u64) << ((4 - i) * 8);
            i += 1;
        }
        Some(Value(QNAN | (TAG_ISTR << TAG_SHIFT) | payload))
    }

    /// Sentinel inline string for empty string "".
    pub const EMPTY_STR: Value = Value(QNAN | (TAG_ISTR << TAG_SHIFT));

    // ── Tag checks ──────────────────────────────────────────────────────

    /// Returns true if this value is an unboxed f64 (including actual NaN).
    /// All NaN-boxed values have the QNAN prefix (bits 62:52 = 0x7FF,
    /// bit 51 = 1) AND a non-zero tag in bits [50:48].
    /// Anything that doesn't match that pattern is a plain f64.
    #[inline(always)]
    pub fn is_f64(self) -> bool {
        // A tagged value has: (bits & QNAN) == QNAN && tag_bits != 0.
        // So is_f64 = NOT tagged = either not-QNAN, or tag_bits == 0.
        !self.is_tagged()
    }

    /// Internal: is this a NaN-boxed tagged value (i32, bool, null, etc.)?
    #[inline(always)]
    fn is_tagged(self) -> bool {
        // Tags 1-7 produce upper-16-bit prefixes 0x7FF9..0x7FFF.
        // A single subtract+compare replaces the old multi-operation check.
        let prefix = (self.0 >> 48) as u16;
        prefix.wrapping_sub(0x7FF9) <= 6
    }

    #[inline(always)]
    pub fn is_i32(self) -> bool {
        (self.0 & (QNAN | (TAG_MASK << TAG_SHIFT))) == (QNAN | (TAG_I32 << TAG_SHIFT))
    }

    /// Check if both values are i32 in a single branch-free operation.
    /// XOR each against the i32 tag, OR together, mask → zero iff both match.
    #[inline(always)]
    pub fn both_i32(a: Value, b: Value) -> bool {
        const I32_SIG: u64 = QNAN | (TAG_I32 << TAG_SHIFT);
        const MASK: u64 = QNAN | (TAG_MASK << TAG_SHIFT);
        ((a.0 ^ I32_SIG) | (b.0 ^ I32_SIG)) & MASK == 0
    }

    /// Try to extract an i32 from this Value.
    /// Handles both i32-tagged values (direct extract) and f64 values that
    /// are losslessly representable as i32 (e.g. 0.0, 255.0).
    #[inline(always)]
    pub fn try_as_i32(self) -> Option<i32> {
        if self.is_i32() {
            Some(unsafe { self.as_i32_unchecked() })
        } else if self.is_f64() {
            let f = self.as_f64();
            let i = f as i32;
            if i as f64 == f {
                Some(i)
            } else {
                None
            }
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn is_bool(self) -> bool {
        (self.0 & (QNAN | (TAG_MASK << TAG_SHIFT))) == (QNAN | (TAG_BOOL << TAG_SHIFT))
    }

    #[inline(always)]
    pub fn is_null(self) -> bool {
        self.0 == Self::NULL.0
    }

    #[inline(always)]
    pub fn is_undefined(self) -> bool {
        self.0 == Self::UNDEFINED.0
    }

    #[inline(always)]
    pub fn is_symbol(self) -> bool {
        (self.0 & (QNAN | (TAG_MASK << TAG_SHIFT))) == (QNAN | (TAG_SYMBOL << TAG_SHIFT))
    }

    #[inline(always)]
    pub fn is_heap(self) -> bool {
        (self.0 & (QNAN | (TAG_MASK << TAG_SHIFT))) == (QNAN | (TAG_HEAP << TAG_SHIFT))
    }

    /// True if this value is an inline string (≤5 bytes stored in payload).
    #[inline(always)]
    pub fn is_inline_str(self) -> bool {
        (self.0 & (QNAN | (TAG_MASK << TAG_SHIFT))) == (QNAN | (TAG_ISTR << TAG_SHIFT))
    }

    /// Length of the inline string (0-5). Caller must check `is_inline_str()`.
    #[inline(always)]
    pub fn inline_str_len(self) -> usize {
        ((self.0 >> 40) & 0xFF) as usize
    }

    /// Extract inline string bytes into a fixed-size buffer.
    /// Returns (buffer, length). Valid string is `&buf[..len]`.
    /// Caller must check `is_inline_str()`.
    #[inline(always)]
    pub fn inline_str_buf(self) -> ([u8; 5], usize) {
        let len = self.inline_str_len();
        let buf = [
            ((self.0 >> 32) & 0xFF) as u8,
            ((self.0 >> 24) & 0xFF) as u8,
            ((self.0 >> 16) & 0xFF) as u8,
            ((self.0 >> 8) & 0xFF) as u8,
            (self.0 & 0xFF) as u8,
        ];
        (buf, len)
    }

    /// True if this value is a number (i32 or f64).
    #[inline(always)]
    pub fn is_number(self) -> bool {
        self.is_i32() || self.is_f64()
    }

    // ── Extractors ──────────────────────────────────────────────────────

    /// Extract i32. Caller must have checked `is_i32()`.
    ///
    /// # Safety
    /// The caller must ensure `self.is_i32()` returns true.
    #[inline(always)]
    pub unsafe fn as_i32_unchecked(self) -> i32 {
        self.0 as u32 as i32
    }

    /// Extract f64 (only valid when `is_f64()` is true).
    #[inline(always)]
    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// Extract bool. Caller must have checked `is_bool()`.
    ///
    /// # Safety
    /// The caller must ensure `self.is_bool()` returns true.
    #[inline(always)]
    pub unsafe fn as_bool_unchecked(self) -> bool {
        (self.0 & 1) != 0
    }

    /// Extract symbol id. Caller must have checked `is_symbol()`.
    #[inline(always)]
    pub fn as_symbol(self) -> u32 {
        self.0 as u32
    }

    /// Extract heap index. Caller must have checked `is_heap()`.
    #[inline(always)]
    pub fn heap_index(self) -> u32 {
        self.0 as u32
    }

    // ── Numeric coercion ────────────────────────────────────────────────

    /// Convert to f64 for arithmetic. i32 → f64 widening, f64 → identity.
    /// For non-numeric values returns NaN (matching JS `Number(x)` for
    /// undefined) or 0.0 for null/false, etc. But those cases should be
    /// handled by the slow path; this is for the fast i32/f64 path.
    #[inline(always)]
    pub fn to_number(self) -> f64 {
        if self.is_i32() {
            unsafe { self.as_i32_unchecked() as f64 }
        } else {
            self.as_f64()
        }
    }

    // ── Truthiness ──────────────────────────────────────────────────────

    /// Fast truthiness check for the common inline types (bool, i32, f64,
    /// null, undefined). For heap objects, returns true (all objects are
    /// truthy in JS). If you need string emptiness checks, use
    /// `is_truthy_full`.
    #[inline(always)]
    pub fn is_truthy(self) -> bool {
        if self.is_bool() {
            return unsafe { self.as_bool_unchecked() };
        }
        if self.is_i32() {
            return unsafe { self.as_i32_unchecked() } != 0;
        }
        if self.is_null() || self.is_undefined() {
            return false;
        }
        if self.is_f64() {
            let v = self.as_f64();
            return v != 0.0 && !v.is_nan();
        }
        if self.is_inline_str() {
            return self.inline_str_len() > 0;
        }
        // Heap objects (strings, arrays, hashes, etc.) — need Heap to check
        // string emptiness. Conservatively return true here.
        true
    }

    /// Quick inspect without heap access. For inline types, returns the
    /// proper string. For heap objects, returns a placeholder.
    pub fn inspect_inline(self) -> String {
        if self.is_i32() {
            let mut buf = itoa::Buffer::new();
            return buf.format(unsafe { self.as_i32_unchecked() }).to_string();
        }
        if self.is_f64() {
            let v = self.as_f64();
            if v.is_nan() {
                return "NaN".to_string();
            }
            if v.is_infinite() {
                return if v > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                };
            }
            if v.fract() == 0.0 && v.abs() < i64::MAX as f64 {
                let mut buf = itoa::Buffer::new();
                return buf.format(v as i64).to_string();
            }
            return format!("{}", v);
        }
        if self.is_bool() {
            return if unsafe { self.as_bool_unchecked() } {
                "true".to_string()
            } else {
                "false".to_string()
            };
        }
        if self.is_null() {
            return "null".to_string();
        }
        if self.is_undefined() {
            return "undefined".to_string();
        }
        if self.is_inline_str() {
            let (buf, len) = self.inline_str_buf();
            return std::str::from_utf8(&buf[..len]).unwrap_or("").to_string();
        }
        // Heap object — caller should use val_inspect for full output
        "[ref]".to_string()
    }

    /// Full truthiness check that also handles heap-allocated strings
    /// (empty string is falsy in JS).
    #[inline(always)]
    pub fn is_truthy_full(self, heap: &Heap) -> bool {
        if self.is_inline_str() {
            return self.inline_str_len() > 0;
        }
        if self.is_heap() {
            let obj = heap.get(self.heap_index());
            match obj {
                Object::String(s) => !s.is_empty(),
                _ => true,
            }
        } else {
            self.is_truthy()
        }
    }
}

// ── Heap ────────────────────────────────────────────────────────────────

/// The heap stores all non-inline Objects (strings, arrays, hashes, etc.).
/// Values reference heap objects by index (u32).
pub struct Heap {
    pub objects: Vec<Object>,
    /// Free list: indices of slots nulled by GC, reused by alloc() before growing.
    free_list: Vec<u32>,
    /// Maps Rc raw pointer address → heap index for O(1) lookup during GC mark.
    /// Populated for Rc-based types (Hash, Array, Generator) on alloc().
    rc_index: FxHashMap<usize, u32>,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    pub fn new() -> Self {
        Self {
            objects: Vec::with_capacity(256),
            free_list: Vec::new(),
            rc_index: FxHashMap::default(),
        }
    }

    /// Number of live objects on the heap (total slots minus free-list size).
    #[inline]
    pub fn allocated_count(&self) -> usize {
        self.objects.len() - self.free_list.len()
    }

    /// Estimated total memory usage in bytes.
    /// Uses object count * average size as a conservative estimate.
    /// Avoids borrowing Rc<VmCell<...>> internals which requires unsafe.
    pub fn estimated_memory_bytes(&self) -> usize {
        // Each Object slot is ~128 bytes (enum + Rc + backing data).
        // Strings average ~64 bytes, arrays ~256 bytes, hashes ~512 bytes.
        // Conservative: count all objects at 256 bytes average.
        self.objects.len() * 256
    }

    /// Allocate a new heap slot, store the object, return the index.
    /// Reuses GC-freed slots from the free list before growing the Vec.
    #[inline]
    pub fn alloc(&mut self, obj: Object) -> u32 {
        let idx = if let Some(idx) = self.free_list.pop() {
            self.objects[idx as usize] = obj;
            idx
        } else {
            let idx = self.objects.len() as u32;
            self.objects.push(obj);
            idx
        };
        // Register Rc pointer for O(1) GC mark lookup
        self.register_rc(idx);
        idx
    }

    /// If the object at `idx` is an Rc-based type, register its pointer in rc_index.
    #[inline]
    fn register_rc(&mut self, idx: u32) {
        match &self.objects[idx as usize] {
            Object::Hash(rc) => { self.rc_index.insert(Rc::as_ptr(rc) as usize, idx); }
            Object::Array(rc) => { self.rc_index.insert(Rc::as_ptr(rc) as usize, idx); }
            Object::Generator(rc) => { self.rc_index.insert(Rc::as_ptr(rc) as usize, idx); }
            _ => {}
        }
    }

    /// Remove the Rc pointer for the object at `idx` from rc_index.
    #[inline]
    pub fn unregister_rc(&mut self, idx: u32) {
        match &self.objects[idx as usize] {
            Object::Hash(rc) => { self.rc_index.remove(&(Rc::as_ptr(rc) as usize)); }
            Object::Array(rc) => { self.rc_index.remove(&(Rc::as_ptr(rc) as usize)); }
            Object::Generator(rc) => { self.rc_index.remove(&(Rc::as_ptr(rc) as usize)); }
            _ => {}
        }
    }

    /// Look up a heap index by Rc raw pointer address. O(1).
    #[inline]
    pub fn rc_lookup(&self, ptr: usize) -> Option<u32> {
        self.rc_index.get(&ptr).copied()
    }

    /// Allocate and return a Value::from_heap pointing to the new slot.
    #[inline]
    pub fn alloc_val(&mut self, obj: Object) -> Value {
        Value::from_heap(self.alloc(obj))
    }

    /// Get a reference to the object at `index`.
    #[inline(always)]
    pub fn get(&self, index: u32) -> &Object {
        debug_assert!((index as usize) < self.objects.len());
        unsafe { self.objects.get_unchecked(index as usize) }
    }

    /// Get a mutable reference to the object at `index`.
    #[inline(always)]
    pub fn get_mut(&mut self, index: u32) -> &mut Object {
        debug_assert!((index as usize) < self.objects.len());
        unsafe { self.objects.get_unchecked_mut(index as usize) }
    }

    /// Clear all objects but keep the allocated capacity for reuse.
    #[inline]
    pub fn reset(&mut self) {
        self.objects.clear();
        self.free_list.clear();
        self.rc_index.clear();
    }

    /// Add a freed slot index to the free list (called during GC sweep).
    #[inline]
    pub fn add_free(&mut self, idx: u32) {
        self.free_list.push(idx);
    }

    /// Clear the free list (called at start of GC sweep).
    #[inline]
    pub fn clear_free_list(&mut self) {
        self.free_list.clear();
    }

    /// Remove free-list entries beyond the given length (after truncation).
    pub fn trim_free_list(&mut self, max_idx: usize) {
        self.free_list.retain(|&idx| (idx as usize) < max_idx);
    }
}

// ── Conversion functions ────────────────────────────────────────────────

/// Convert an `&Object` to a `Value`, allocating on the heap if needed.
#[inline]
pub fn obj_to_val(obj: &Object, heap: &mut Heap) -> Value {
    match obj {
        Object::Integer(v) => Value::from_i64(*v),
        Object::Float(v) => Value::from_f64(*v),
        Object::Boolean(v) => Value::from_bool(*v),
        Object::Null => Value::NULL,
        Object::Undefined => Value::UNDEFINED,
        Object::String(s) => match Value::from_inline_str(s.as_bytes()) {
            Some(v) => v,
            None => heap.alloc_val(obj.clone()),
        },
        // Heap-allocated types: clone into heap
        other => heap.alloc_val(other.clone()),
    }
}

/// Convert an owned `Object` to a `Value`, allocating on the heap if needed.
/// Avoids the clone for heap types.
#[inline]
pub fn obj_into_val(obj: Object, heap: &mut Heap) -> Value {
    match obj {
        Object::Integer(v) => Value::from_i64(v),
        Object::Float(v) => Value::from_f64(v),
        Object::Boolean(v) => Value::from_bool(v),
        Object::Null => Value::NULL,
        Object::Undefined => Value::UNDEFINED,
        Object::String(ref s) => match Value::from_inline_str(s.as_bytes()) {
            Some(v) => v,
            None => heap.alloc_val(obj),
        },
        // Move into heap without cloning
        other => heap.alloc_val(other),
    }
}

/// Convert a `Value` back to an `Object`, reading from the heap if needed.
#[inline]
pub fn val_to_obj(val: Value, heap: &Heap) -> Object {
    if val.is_i32() {
        return Object::Integer(unsafe { val.as_i32_unchecked() } as i64);
    }
    if val.is_f64() {
        return Object::Float(val.as_f64());
    }
    if val.is_bool() {
        return Object::Boolean(unsafe { val.as_bool_unchecked() });
    }
    if val.is_null() {
        return Object::Null;
    }
    if val.is_undefined() {
        return Object::Undefined;
    }
    if val.is_inline_str() {
        let (buf, len) = val.inline_str_buf();
        // SAFETY: bytes were originally valid UTF-8 from an Rc<str>
        let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
        return Object::String(Rc::from(s));
    }
    if val.is_heap() {
        return heap.get(val.heap_index()).clone();
    }
    // Symbol or unknown — shouldn't happen in normal operation
    Object::Undefined
}

/// Convert a `Value` to a `Cow<Object>` — borrowed for heap objects (avoiding clone),
/// owned for primitives (which are constructed inline).
#[inline]
pub fn val_as_obj_ref(val: Value, heap: &Heap) -> std::borrow::Cow<'_, Object> {
    use std::borrow::Cow;
    if val.is_heap() {
        return Cow::Borrowed(heap.get(val.heap_index()));
    }
    Cow::Owned(if val.is_i32() {
        Object::Integer(unsafe { val.as_i32_unchecked() } as i64)
    } else if val.is_f64() {
        Object::Float(val.as_f64())
    } else if val.is_bool() {
        Object::Boolean(unsafe { val.as_bool_unchecked() })
    } else if val.is_inline_str() {
        let (buf, len) = val.inline_str_buf();
        let s = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
        Object::String(Rc::from(s))
    } else if val.is_null() {
        Object::Null
    } else {
        Object::Undefined
    })
}

/// Produce a debug/inspect string for a Value.
pub fn val_inspect(val: Value, heap: &Heap) -> String {
    if val.is_i32() {
        return format!("{}", unsafe { val.as_i32_unchecked() });
    }
    if val.is_f64() {
        let v = val.as_f64();
        if v.is_nan() {
            return "NaN".to_string();
        }
        if v.is_infinite() {
            return if v > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            };
        }
        return format!("{}", v);
    }
    if val.is_bool() {
        return if unsafe { val.as_bool_unchecked() } {
            "true".to_string()
        } else {
            "false".to_string()
        };
    }
    if val.is_null() {
        return "null".to_string();
    }
    if val.is_undefined() {
        return "undefined".to_string();
    }
    if val.is_inline_str() {
        let (buf, len) = val.inline_str_buf();
        return unsafe { std::str::from_utf8_unchecked(&buf[..len]) }.to_string();
    }
    if val.is_heap() {
        return heap.get(val.heap_index()).inspect();
    }
    "undefined".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn test_i32_roundtrip() {
        for v in [0, 1, -1, i32::MAX, i32::MIN, 42, -42] {
            let val = Value::from_i32(v);
            assert!(val.is_i32(), "from_i32({v}) should be i32");
            assert!(!val.is_f64(), "from_i32({v}) should not be f64");
            assert_eq!(unsafe { val.as_i32_unchecked() }, v);
        }
    }

    #[test]
    fn test_f64_roundtrip() {
        for v in [0.0, 1.5, -1.5, f64::INFINITY, f64::NEG_INFINITY, 1e100] {
            let val = Value::from_f64(v);
            assert!(val.is_f64(), "from_f64({v}) should be f64");
            assert!(!val.is_i32(), "from_f64({v}) should not be i32");
            assert_eq!(val.as_f64(), v);
        }
    }

    #[test]
    fn test_f64_nan() {
        let val = Value::from_f64(f64::NAN);
        assert!(val.is_f64(), "NaN should be f64");
        assert!(val.as_f64().is_nan());
    }

    #[test]
    fn test_bool() {
        assert!(Value::TRUE.is_bool());
        assert!(Value::FALSE.is_bool());
        assert!(unsafe { Value::TRUE.as_bool_unchecked() });
        assert!(!unsafe { Value::FALSE.as_bool_unchecked() });
    }

    #[test]
    fn test_null_undefined() {
        assert!(Value::NULL.is_null());
        assert!(!Value::NULL.is_undefined());
        assert!(Value::UNDEFINED.is_undefined());
        assert!(!Value::UNDEFINED.is_null());
    }

    #[test]
    fn test_heap() {
        let val = Value::from_heap(42);
        assert!(val.is_heap());
        assert_eq!(val.heap_index(), 42);
    }

    #[test]
    fn test_i64_promotion() {
        // Fits in i32
        let val = Value::from_i64(100);
        assert!(val.is_i32());
        assert_eq!(unsafe { val.as_i32_unchecked() }, 100);

        // Doesn't fit in i32 — becomes f64
        let val = Value::from_i64(i64::MAX);
        assert!(val.is_f64());
    }

    #[test]
    fn test_truthiness() {
        assert!(Value::TRUE.is_truthy());
        assert!(!Value::FALSE.is_truthy());
        assert!(!Value::NULL.is_truthy());
        assert!(!Value::UNDEFINED.is_truthy());
        assert!(Value::from_i32(1).is_truthy());
        assert!(!Value::from_i32(0).is_truthy());
        assert!(Value::from_f64(1.0).is_truthy());
        assert!(!Value::from_f64(0.0).is_truthy());
        assert!(!Value::from_f64(f64::NAN).is_truthy());
    }

    #[test]
    fn test_obj_val_roundtrip() {
        let mut heap = Heap::new();

        // Inline types
        let obj = Object::Integer(42);
        let val = obj_to_val(&obj, &mut heap);
        let back = val_to_obj(val, &heap);
        assert!(matches!(back, Object::Integer(42)));

        #[allow(clippy::approx_constant)]
        let pi_approx = 3.14;
        let obj = Object::Float(pi_approx);
        let val = obj_to_val(&obj, &mut heap);
        let back = val_to_obj(val, &heap);
        assert!(matches!(back, Object::Float(v) if (v - pi_approx).abs() < 1e-15));

        let obj = Object::Boolean(true);
        let val = obj_to_val(&obj, &mut heap);
        let back = val_to_obj(val, &heap);
        assert!(matches!(back, Object::Boolean(true)));

        let obj = Object::Null;
        let val = obj_to_val(&obj, &mut heap);
        let back = val_to_obj(val, &heap);
        assert!(matches!(back, Object::Null));

        // Short string → inline
        let obj = Object::String(Rc::from("abc"));
        let val = obj_to_val(&obj, &mut heap);
        assert!(val.is_inline_str(), "short string should be inline");
        let back = val_to_obj(val, &heap);
        assert!(matches!(back, Object::String(s) if &*s == "abc"));

        // Long string → heap
        let obj = Object::String(Rc::from("hello!"));
        let val = obj_to_val(&obj, &mut heap);
        assert!(val.is_heap(), "6-byte string should be on heap");
        let back = val_to_obj(val, &heap);
        assert!(matches!(back, Object::String(s) if &*s == "hello!"));
    }

    #[test]
    fn test_inline_str() {
        // Empty string
        let val = Value::from_inline_str(b"").unwrap();
        assert!(val.is_inline_str());
        assert_eq!(val.inline_str_len(), 0);
        assert!(!val.is_truthy(), "empty string should be falsy");
        assert_eq!(val, Value::EMPTY_STR);

        // 1-byte string
        let val = Value::from_inline_str(b"a").unwrap();
        assert!(val.is_inline_str());
        assert_eq!(val.inline_str_len(), 1);
        assert!(val.is_truthy(), "non-empty string should be truthy");
        let (buf, len) = val.inline_str_buf();
        assert_eq!(&buf[..len], b"a");

        // 5-byte string (max)
        let val = Value::from_inline_str(b"hello").unwrap();
        assert!(val.is_inline_str());
        assert_eq!(val.inline_str_len(), 5);
        let (buf, len) = val.inline_str_buf();
        assert_eq!(&buf[..len], b"hello");

        // 6-byte string → None
        assert!(Value::from_inline_str(b"hello!").is_none());

        // Equality: same bytes → same bits
        let a = Value::from_inline_str(b"abc").unwrap();
        let b = Value::from_inline_str(b"abc").unwrap();
        assert_eq!(a, b);

        // Inequality: different bytes → different bits
        let c = Value::from_inline_str(b"abd").unwrap();
        assert_ne!(a, c);

        // Not confused with other tags
        assert!(!val.is_i32());
        assert!(!val.is_f64());
        assert!(!val.is_heap());
        assert!(!val.is_bool());
        assert!(!val.is_null());
        assert!(!val.is_undefined());
    }

    #[test]
    fn test_inline_str_roundtrip_via_obj() {
        let mut heap = Heap::new();

        // Short string through obj_into_val
        let obj = Object::String(Rc::from("xy"));
        let val = obj_into_val(obj, &mut heap);
        assert!(val.is_inline_str());
        let back = val_to_obj(val, &heap);
        assert!(matches!(back, Object::String(s) if &*s == "xy"));
    }
}
