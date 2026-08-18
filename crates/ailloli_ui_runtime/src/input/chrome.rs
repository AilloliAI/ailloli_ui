#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Default,
    Resize(ResizeEdge),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChromeAction {
    StartWindowDrag,
    StartWindowResize { edge: ResizeEdge },
    SetCursor { cursor: CursorStyle },
}
