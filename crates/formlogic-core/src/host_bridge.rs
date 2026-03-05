/// Pending host call: represents an async operation the VM wants the host to perform.
/// The VM is synchronous, so it queues these and the host drains + resolves them
/// after each VM function call.
#[derive(Clone, Debug)]
pub struct PendingHostCall {
    /// Unique ID for matching callback resolution.
    pub id: u32,
    /// Kind of host call — an opaque string the host interprets (e.g. "net.fetch").
    pub kind: String,
    /// Serialized arguments (JSON strings, URLs, etc.).
    pub args: Vec<String>,
}
