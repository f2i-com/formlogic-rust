//! HTTP bridge — allows FormLogic scripts to make HTTP requests.

/// Result of an HTTP request.
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub ok: bool,
}

/// Pluggable HTTP backend for server-side FormLogic.
pub trait HttpBridge {
    /// Perform a GET request.
    fn get(&self, url: &str) -> Result<HttpResponse, String>;
    /// Perform a POST request with a body.
    fn post(&self, url: &str, body: &str, content_type: &str) -> Result<HttpResponse, String>;
    /// Perform a PUT request with a body.
    fn put(&self, url: &str, body: &str, content_type: &str) -> Result<HttpResponse, String>;
    /// Perform a DELETE request.
    fn delete(&self, url: &str) -> Result<HttpResponse, String>;
}
