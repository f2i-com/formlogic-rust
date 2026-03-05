/// Trait for pluggable CSS layout backends (e.g. taffy).
/// Node IDs are opaque u64 handles managed by the implementation.
pub trait LayoutBridge {
    /// Create a layout node with the given style. Returns a node handle.
    fn create_node(&mut self, style: LayoutStyle) -> u64;

    /// Update the style of an existing layout node.
    fn update_style(&mut self, node: u64, style: LayoutStyle);

    /// Set the children of a layout node (ordered).
    fn set_children(&mut self, parent: u64, children: &[u64]);

    /// Compute layout for the tree rooted at `root` with the given available space.
    fn compute_layout(&mut self, root: u64, available_width: f64, available_height: f64);

    /// Get the computed layout for a node: (x, y, width, height).
    fn get_layout(&self, node: u64) -> (f64, f64, f64, f64);

    /// Remove a node and free its resources.
    fn remove_node(&mut self, node: u64);

    /// Remove all nodes (reset for next frame).
    fn clear(&mut self);
}

/// Layout style properties passed from formlogic to the layout engine.
#[derive(Clone, Debug)]
pub struct LayoutStyle {
    // Display
    pub display: LayoutDisplay,
    pub position: LayoutPosition,
    pub overflow: LayoutOverflow,

    // Flex container
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub align_content: AlignContent,
    pub gap_row: f64,
    pub gap_column: f64,

    // Flex item
    pub flex_grow: f64,
    pub flex_shrink: f64,
    pub flex_basis: Dimension,
    pub align_self: AlignSelf,

    // Size
    pub width: Dimension,
    pub height: Dimension,
    pub min_width: Dimension,
    pub min_height: Dimension,
    pub max_width: Dimension,
    pub max_height: Dimension,

    // Spacing: [top, right, bottom, left]
    pub padding: [f64; 4],
    pub margin: [Dimension; 4],
    pub border: [f64; 4],

    // Absolute positioning
    pub inset: [Dimension; 4], // [top, right, bottom, left]

    // Grid (basic)
    pub grid_template_columns: Vec<GridTrack>,
    pub grid_template_rows: Vec<GridTrack>,
    pub grid_column_start: GridPlacement,
    pub grid_column_end: GridPlacement,
    pub grid_row_start: GridPlacement,
    pub grid_row_end: GridPlacement,

    // Aspect ratio
    pub aspect_ratio: Option<f64>,

    // Ordering / stacking
    pub z_index: i32,
    pub order: i32,
}

#[derive(Clone, Debug, Default)]
pub enum LayoutDisplay {
    #[default]
    Flex,
    Grid,
    None,
}

#[derive(Clone, Debug, Default)]
pub enum LayoutPosition {
    #[default]
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Clone, Debug, Default)]
pub enum LayoutOverflow {
    #[default]
    Visible,
    Hidden,
    Scroll,
}

#[derive(Clone, Debug, Default)]
pub enum FlexDirection {
    Row,
    #[default]
    Column,
    RowReverse,
    ColumnReverse,
}

#[derive(Clone, Debug, Default)]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Clone, Debug, Default)]
pub enum JustifyContent {
    #[default]
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Debug, Default)]
pub enum AlignItems {
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    #[default]
    Stretch,
}

#[derive(Clone, Debug, Default)]
pub enum AlignContent {
    FlexStart,
    FlexEnd,
    Center,
    #[default]
    Stretch,
    SpaceBetween,
    SpaceAround,
}

#[derive(Clone, Debug, Default)]
pub enum AlignSelf {
    #[default]
    Auto,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    Stretch,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Dimension {
    #[default]
    Auto,
    Points(f64),
    Percent(f64),
}

#[derive(Clone, Debug)]
pub enum GridTrack {
    Points(f64),
    Percent(f64),
    Fr(f64),
    Auto,
    MinContent,
    MaxContent,
}

#[derive(Clone, Debug, Default)]
pub enum GridPlacement {
    #[default]
    Auto,
    Line(i32),
    Span(u32),
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            display: LayoutDisplay::Flex,
            position: LayoutPosition::Relative,
            overflow: LayoutOverflow::Visible,
            flex_direction: FlexDirection::Column,
            flex_wrap: FlexWrap::NoWrap,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            align_content: AlignContent::Stretch,
            gap_row: 0.0,
            gap_column: 0.0,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Dimension::Auto,
            align_self: AlignSelf::Auto,
            width: Dimension::Auto,
            height: Dimension::Auto,
            min_width: Dimension::Auto,
            min_height: Dimension::Auto,
            max_width: Dimension::Auto,
            max_height: Dimension::Auto,
            padding: [0.0; 4],
            margin: [
                Dimension::Auto,
                Dimension::Auto,
                Dimension::Auto,
                Dimension::Auto,
            ],
            border: [0.0; 4],
            inset: [Dimension::Auto; 4],
            grid_template_columns: Vec::new(),
            grid_template_rows: Vec::new(),
            grid_column_start: GridPlacement::Auto,
            grid_column_end: GridPlacement::Auto,
            grid_row_start: GridPlacement::Auto,
            grid_row_end: GridPlacement::Auto,
            aspect_ratio: None,
            z_index: 0,
            order: 0,
        }
    }
}
