/// Platform cursor hint for hover regions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorStyle {
    #[default]
    Auto,
    Default,
    Text,
    Pointer,
    Grab,
    Grabbing,
    ResizeX,
    ResizeY,
}
