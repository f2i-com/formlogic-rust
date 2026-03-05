/// Trait for pluggable input/event state (provided by the host runtime).
pub trait InputBridge {
    /// Current mouse X position in logical pixels.
    fn get_mouse_x(&self) -> f64;
    /// Current mouse Y position in logical pixels.
    fn get_mouse_y(&self) -> f64;
    /// Whether the primary mouse button is currently pressed.
    fn is_mouse_down(&self) -> bool;
    /// Whether the primary mouse button was just pressed this frame.
    fn is_mouse_pressed(&self) -> bool;
    /// Whether the primary mouse button was just released this frame.
    fn is_mouse_released(&self) -> bool;
    /// Current scroll Y offset.
    fn get_scroll_y(&self) -> f64;
    /// Set the cursor icon. cursor is one of: "default", "pointer", "text",
    /// "grab", "grabbing", "move", "not-allowed", "crosshair", "col-resize",
    /// "row-resize", "ew-resize", "ns-resize".
    fn set_cursor(&mut self, cursor: &str);
    /// Get pending text input (characters typed since last frame). Returns empty if none.
    fn get_text_input(&self) -> String;
    /// Whether backspace was pressed this frame.
    fn is_backspace_pressed(&self) -> bool;
    /// Whether escape was pressed this frame.
    fn is_escape_pressed(&self) -> bool;
    /// Request a redraw on the next frame.
    fn request_redraw(&mut self);
    /// Elapsed time since app start in seconds (f64).
    fn get_elapsed_secs(&self) -> f64;
    /// Elapsed time since last page navigation in seconds.
    fn get_page_elapsed_secs(&self) -> f64;
    /// Delta time since last frame in seconds.
    fn get_delta_time(&self) -> f64;
    /// Get the currently focused input variable name, if any.
    fn get_focused_input(&self) -> Option<String>;
    /// Set the focused input variable name.
    fn set_focused_input(&mut self, var_name: Option<&str>);
    /// Check if a key is currently pressed. key is the key name (e.g. "w", "ArrowUp").
    fn is_key_down(&self, key: &str) -> bool;
}
