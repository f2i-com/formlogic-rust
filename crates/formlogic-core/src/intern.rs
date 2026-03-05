use rustc_hash::FxHashMap;
use std::rc::Rc;

pub struct Interner {
    strings: Vec<Rc<str>>,
    ids: FxHashMap<Rc<str>, u32>,
}

impl Interner {
    fn new() -> Self {
        Self {
            strings: Vec::new(),
            ids: FxHashMap::default(),
        }
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.ids.get(s) {
            return id;
        }
        let rc: Rc<str> = Rc::from(s);
        let id = self.strings.len() as u32;
        self.strings.push(Rc::clone(&rc));
        self.ids.insert(rc, id);
        id
    }

    fn intern_rc(&mut self, s: &Rc<str>) -> u32 {
        if let Some(&id) = self.ids.get(&**s) {
            return id;
        }
        let id = self.strings.len() as u32;
        self.strings.push(Rc::clone(s));
        self.ids.insert(Rc::clone(s), id);
        id
    }

    fn resolve(&self, id: u32) -> Rc<str> {
        Rc::clone(&self.strings[id as usize])
    }
}

thread_local! {
    static INTERNER: std::cell::RefCell<Interner> = std::cell::RefCell::new(Interner::new());
}

/// Intern a string slice, returning a u32 symbol ID.
/// Identical strings always return the same ID.
#[inline]
pub fn intern(s: &str) -> u32 {
    INTERNER.with(|interner| interner.borrow_mut().intern(s))
}

/// Intern an existing `Rc<str>`, returning a u32 symbol ID.
/// O(1) if already interned.
#[inline]
pub fn intern_rc(s: &Rc<str>) -> u32 {
    INTERNER.with(|interner| interner.borrow_mut().intern_rc(s))
}

/// Resolve a symbol ID back to its canonical `Rc<str>`.
/// Panics if id is out of range (should never happen for valid symbol IDs).
#[inline]
pub fn resolve(id: u32) -> Rc<str> {
    INTERNER.with(|interner| interner.borrow().resolve(id))
}

/// Intern a string slice, returning the canonical `Rc<str>`.
/// Convenience for callers that need the Rc back (e.g. Object::String constants).
#[inline]
pub fn intern_str(s: &str) -> Rc<str> {
    INTERNER.with(|interner| {
        let mut i = interner.borrow_mut();
        let id = i.intern(s);
        Rc::clone(&i.strings[id as usize])
    })
}
