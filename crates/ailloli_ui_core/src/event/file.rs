//! Ordered, provider-neutral file hover and drop payloads.

use crate::{Point, UploadFile};

/// Platform-neutral file hover/drop event.
///
/// Current variants are `Entered`, `Moved`, `Left`, and `Dropped`; `Hover`,
/// `HoverCancelled`, and `Drop` remain compatibility variants.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{event::FileEvent, Point, UploadFile};
/// let event = FileEvent::dropped(Some(Point::new(2.0, 3.0)), [UploadFile::named("a.txt")]);
/// assert_eq!(event.files()[0].name, "a.txt");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum FileEvent {
    /// A batch of files entered the logical window.
    Entered {
        /// Logical pointer position when supplied by the provider.
        pos: Option<Point>,
        /// Files in provider order; the batch may be empty.
        files: Vec<UploadFile>,
    },
    /// A hovering batch moved within the logical window.
    Moved {
        /// Logical pointer position when supplied by the provider.
        pos: Option<Point>,
        /// Current files in provider order; the batch may be empty.
        files: Vec<UploadFile>,
    },
    /// The current file batch left the logical window.
    Left,
    /// A batch of files was dropped in the logical window.
    Dropped {
        /// Logical pointer position when supplied by the provider.
        pos: Option<Point>,
        /// Dropped files in provider order; the batch may be empty.
        files: Vec<UploadFile>,
    },
    /// One or more files are hovering over the window at logical `pos`.
    ///
    /// New adapters should use [`FileEvent::Entered`] and [`FileEvent::Moved`].
    Hover {
        /// Logical pointer position reported by the legacy adapter.
        pos: Point,
        /// Hovering files in provider order.
        files: Vec<UploadFile>,
    },
    /// File hover was cancelled by the platform.
    ///
    /// New adapters should use [`FileEvent::Left`].
    HoverCancelled,
    /// One or more files were dropped at logical `pos`.
    ///
    /// New adapters should use [`FileEvent::Dropped`].
    Drop {
        /// Logical pointer position reported by the legacy adapter.
        pos: Point,
        /// Dropped files in provider order.
        files: Vec<UploadFile>,
    },
}

impl FileEvent {
    /// Creates an ordered batch that entered the logical window.
    ///
    /// `None` preserves that the provider did not supply a pointer position;
    /// it is not replaced with an origin or stale coordinate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::FileEvent, UploadFile};
    /// let event = FileEvent::entered(None, [UploadFile::named("a.txt")]);
    /// assert_eq!(event.pos(), None);
    /// ```
    pub fn entered(pos: Option<Point>, files: impl IntoIterator<Item = UploadFile>) -> Self {
        Self::Entered {
            pos,
            files: files.into_iter().collect(),
        }
    }

    /// Creates an ordered hovering batch movement.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::FileEvent, Point, UploadFile};
    /// let event = FileEvent::moved(Some(Point::new(1.0, 2.0)), [UploadFile::named("a.txt")]);
    /// assert_eq!(event.pos(), Some(Point::new(1.0, 2.0)));
    /// ```
    pub fn moved(pos: Option<Point>, files: impl IntoIterator<Item = UploadFile>) -> Self {
        Self::Moved {
            pos,
            files: files.into_iter().collect(),
        }
    }

    /// Creates a notification that the current batch left the window.
    ///
    /// Leave events carry neither files nor a position.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::FileEvent;
    /// assert!(FileEvent::left().is_left());
    /// ```
    pub const fn left() -> Self {
        Self::Left
    }

    /// Creates an ordered batch dropped in the logical window.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::FileEvent, UploadFile};
    /// assert_eq!(FileEvent::dropped(None, [UploadFile::named("a.txt")]).files().len(), 1);
    /// ```
    pub fn dropped(pos: Option<Point>, files: impl IntoIterator<Item = UploadFile>) -> Self {
        Self::Dropped {
            pos,
            files: files.into_iter().collect(),
        }
    }

    /// Logical pointer position supplied by the provider, when known.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::FileEvent, Point, UploadFile};
    /// let event = FileEvent::entered(Some(Point::new(3.0, 4.0)), [UploadFile::named("a")]);
    /// assert_eq!(event.pos(), Some(Point::new(3.0, 4.0)));
    /// ```
    pub fn pos(&self) -> Option<Point> {
        match self {
            Self::Entered { pos, .. } | Self::Moved { pos, .. } | Self::Dropped { pos, .. } => *pos,
            Self::Hover { pos, .. } | Self::Drop { pos, .. } => Some(*pos),
            Self::Left | Self::HoverCancelled => None,
        }
    }

    /// Ordered files carried by the event, or an empty slice for leave events.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::FileEvent;
    /// assert!(FileEvent::left().files().is_empty());
    /// ```
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

    /// Returns `true` for both current and legacy leave variants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::FileEvent;
    /// assert!(FileEvent::left().is_left());
    /// ```
    pub const fn is_left(&self) -> bool {
        matches!(self, Self::Left | Self::HoverCancelled)
    }
}
