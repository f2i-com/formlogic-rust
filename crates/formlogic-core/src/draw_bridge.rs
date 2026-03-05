/// Trait for pluggable 2D drawing backends (e.g. vello on native).
/// All coordinates are in logical pixels (pre-DPI-scaling).
pub trait DrawBridge {
    // ── Primitives ──

    /// Draw a filled (optionally bordered) rectangle.
    fn draw_rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        fill: &str,
        border_radius: f64,
        border_width: f64,
        border_color: &str,
        opacity: f64,
    );

    /// Draw a filled rectangle with per-corner border radii [tl, tr, br, bl].
    fn draw_rounded_rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        radii: [f64; 4],
        fill: &str,
        opacity: f64,
    );

    /// Draw a filled circle.
    fn draw_circle(&mut self, cx: f64, cy: f64, r: f64, fill: &str, opacity: f64);

    /// Draw a filled ellipse.
    fn draw_ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, fill: &str, opacity: f64);

    /// Draw a line segment.
    fn draw_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: &str, width: f64);

    /// Draw an SVG-style path.
    /// `commands` is an SVG path data string (M, L, C, Z, etc.).
    fn draw_path(
        &mut self,
        commands: &str,
        fill: &str,
        stroke: &str,
        stroke_width: f64,
        opacity: f64,
    );

    /// Draw text and return its measured (width, height).
    fn draw_text(
        &mut self,
        text: &str,
        x: f64,
        y: f64,
        font_size: f64,
        color: &str,
        font_weight: u32, // 100-900
        font_family: &str,
        max_width: f64,
        letter_spacing: f64,
    ) -> (f64, f64);

    /// Draw an image from a source path/URL.
    fn draw_image(&mut self, src: &str, x: f64, y: f64, w: f64, h: f64, opacity: f64);

    // ── Gradients ──

    /// Draw a rectangle filled with a linear gradient.
    /// `stops` is a flat array: [offset0, r0, g0, b0, a0, offset1, r1, g1, b1, a1, ...]
    fn draw_linear_gradient(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        angle_deg: f64,
        stops: &[f64],
        border_radius: f64,
    );

    /// Draw a rectangle filled with a radial gradient.
    fn draw_radial_gradient(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        stops: &[f64],
        border_radius: f64,
    );

    // ── Shadows ──

    /// Draw a box shadow.
    fn draw_shadow(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        blur: f64,
        spread: f64,
        color: &str,
        offset_x: f64,
        offset_y: f64,
        border_radius: f64,
    );

    // ── Clipping ──

    /// Push a clip rectangle onto the clip stack.
    fn push_clip(&mut self, x: f64, y: f64, w: f64, h: f64, border_radius: f64);

    /// Pop the last clip rectangle from the clip stack.
    fn pop_clip(&mut self);

    // ── Transform ──

    /// Push a 2D transform onto the transform stack.
    fn push_transform(
        &mut self,
        translate_x: f64,
        translate_y: f64,
        rotate_deg: f64,
        scale_x: f64,
        scale_y: f64,
    );

    /// Pop the last transform from the transform stack.
    fn pop_transform(&mut self);

    // ── Opacity layering ──

    /// Push an opacity layer (0.0 = transparent, 1.0 = opaque).
    fn push_opacity(&mut self, opacity: f64);

    /// Pop the last opacity layer.
    fn pop_opacity(&mut self);

    // ── Arcs (for pie/donut charts) ──

    /// Draw an arc segment (donut slice).
    fn draw_arc(
        &mut self,
        cx: f64,
        cy: f64,
        radius: f64,
        thickness: f64,
        start_angle: f64,
        end_angle: f64,
        color: &str,
    );

    // ── Measurement ──

    /// Measure text without drawing it. Returns (width, height).
    fn measure_text(
        &self,
        text: &str,
        font_size: f64,
        font_weight: u32,
        font_family: &str,
        max_width: f64,
    ) -> (f64, f64);

    // ── Viewport ──

    /// Get the viewport width in logical pixels.
    fn get_viewport_width(&self) -> f64;

    /// Get the viewport height in logical pixels.
    fn get_viewport_height(&self) -> f64;
}
