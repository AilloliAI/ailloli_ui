use crate::{Point, UploadFile};

/// Platform-neutral file hover/drop event.
#[derive(Debug, Clone, PartialEq)]
pub enum FileEvent {
    /// One or more files are hovering over the window at logical `pos`.
    Hover { pos: Point, files: Vec<UploadFile> },
    /// File hover was cancelled by the platform.
    HoverCancelled,
    /// One or more files were dropped at logical `pos`.
    Drop { pos: Point, files: Vec<UploadFile> },
}

impl FileEvent {
    pub fn pos(&self) -> Option<Point> {
        match self {
            Self::Hover { pos, .. } | Self::Drop { pos, .. } => Some(*pos),
            Self::HoverCancelled => None,
        }
    }
}
