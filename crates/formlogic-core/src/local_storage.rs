/// Trait for pluggable localStorage backends (e.g. SQLite on native, IndexedDB on web).
pub trait LocalStorageBridge {
    fn get_item(&self, key: &str) -> Option<String>;
    fn set_item(&mut self, key: &str, value: &str);
    fn remove_item(&mut self, key: &str);
    fn clear(&mut self);
}
