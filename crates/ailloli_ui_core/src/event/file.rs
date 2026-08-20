use crate::{Point, UploadFile};

/// Platform-neutral file hover/drop event.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum FileEvent {
    /// A batch of files entered the logical window.
    Entered {
        /// Logical pointer position when supplied by the provider.
        pos: Option<Point>,
        files: Vec<UploadFile>,
    },
    /// A hovering batch moved within the logical window.
    Moved {
        /// Logical pointer position when supplied by the provider.
        pos: Option<Point>,
        files: Vec<UploadFile>,
    },
    /// The current file batch left the logical window.
    Left,
    /// A batch of files was dropped in the logical window.
    Dropped {
        /// Logical pointer position when supplied by the provider.
        pos: Option<Point>,
        files: Vec<UploadFile>,
    },
    /// One or more files are hovering over the window at logical `pos`.
    ///
    /// New adapters should use [`FileEvent::Entered`] and [`FileEvent::Moved`].
    Hover { pos: Point, files: Vec<UploadFile> },
    /// File hover was cancelled by the platform.
    ///
    /// New adapters should use [`FileEvent::Left`].
    HoverCancelled,
    /// One or more files were dropped at logical `pos`.
    ///
    /// New adapters should use [`FileEvent::Dropped`].
    Drop { pos: Point, files: Vec<UploadFile> },
}

impl FileEvent {
    /// Creates an ordered batch that entered the logical window.
    pub fn entered(pos: Option<Point>, files: impl IntoIterator<Item = UploadFile>) -> Self {
        Self::Entered {
            pos,
            files: files.into_iter().collect(),
        }
    }

    /// Creates an ordered hovering batch movement.
    pub fn moved(pos: Option<Point>, files: impl IntoIterator<Item = UploadFile>) -> Self {
        Self::Moved {
            pos,
            files: files.into_iter().collect(),
        }
    }

    /// Creates a notification that the current batch left the window.
    pub const fn left() -> Self {
        Self::Left
    }

    /// Creates an ordered batch dropped in the logical window.
    pub fn dropped(pos: Option<Point>, files: impl IntoIterator<Item = UploadFile>) -> Self {
        Self::Dropped {
            pos,
            files: files.into_iter().collect(),
        }
    }

    /// Logical pointer position supplied by the provider, when known.
    pub fn pos(&self) -> Option<Point> {
        match self {
            Self::Entered { pos, .. } | Self::Moved { pos, .. } | Self::Dropped { pos, .. } => *pos,
            Self::Hover { pos, .. } | Self::Drop { pos, .. } => Some(*pos),
            Self::Left | Self::HoverCancelled => None,
        }
    }

    /// Ordered files carried by the event, or an empty slice for leave events.
    pub fn files(&self) -> &[UploadFile] {
        match self {
            Self::Entered { files, .. }
            | Self::Moved { files, .. }
            | Self::Dropped { files, .. }
            | Self::Hover { files, .. }
            | Self::Drop { files, .. } => files,
            Self::Left | Self::HoverCancelled => &[],
        }
    }

    /// Returns whether the current file batch left the logical window.
    pub const fn is_left(&self) -> bool {
        matches!(self, Self::Left | Self::HoverCancelled)
    }
}
