/// A database record returned from XDB operations.
pub struct DbRecord {
    pub id: String,
    pub collection: String,
    pub data: String,       // JSON string (may be empty when data_parsed is set)
    pub created_at: String, // ISO 8601
    pub updated_at: String,
    /// Pre-parsed data as serde_json::Value. When set, `db_record_to_object`
    /// uses this directly, skipping the `serde_json::from_str` parse of `data`.
    /// This avoids a redundant JSON.stringify → serde_json::from_str round-trip
    /// when the bridge can build the Value directly from the source (e.g. JsValue).
    pub data_parsed: Option<serde_json::Value>,
}

/// Sync connection status.
pub struct DbSyncStatus {
    pub connected: bool,
    pub peers: usize,
    pub room: String,
}

/// Trait for pluggable XDB database backends (e.g. SQLite on native).
pub trait DbBridge {
    fn query(&self, collection: &str) -> Vec<DbRecord>;
    fn create(&mut self, collection: &str, data: &str) -> DbRecord;
    fn update(&mut self, id: &str, data: &str) -> Option<DbRecord>;
    fn delete(&mut self, id: &str);
    fn hard_delete(&mut self, collection: &str, id: &str);
    fn get(&self, collection: &str, id: &str) -> Option<DbRecord>;
    fn start_sync(&mut self, room: &str);
    fn stop_sync(&mut self, room: Option<&str>);
    fn get_sync_status(&self, room: Option<&str>) -> DbSyncStatus;
    fn get_saved_sync_room(&self) -> Option<String>;
}
