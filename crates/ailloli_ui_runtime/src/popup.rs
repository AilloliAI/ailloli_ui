//! Provider-neutral popup ownership and lifecycle.
//!
//! [`PopupPortal`](crate::popup::PopupPortal) is the runtime authority for
//! popup identity, ordering, and dismissal. It intentionally does not choose
//! an overlay or native-window backend: hosts consume
//! [`PopupIntent`](crate::popup::PopupIntent) values and obtain popup content
//! from the portal. This keeps widget APIs independent from a particular
//! windowing provider.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use ailloli_ui_core::{ElementId, LogicalWindowId, Point, Rect, Size};

use crate::app::PresentationGeneration;
use crate::component::View;

/// Logical presentation used by direct/headless event routing.
///
/// Native adapters replace this owner metadata as soon as they dispatch an
/// [`crate::input::EventEnvelope`]. Keeping the fallback in runtime (rather
/// than in a widget crate) lets the input router enforce the same portal
/// semantics in deterministic tests.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::HEADLESS_POPUP_WINDOW_ID;
/// assert_eq!(HEADLESS_POPUP_WINDOW_ID, "__ailloli_headless__");
/// ```
pub const HEADLESS_POPUP_WINDOW_ID: &str = "__ailloli_headless__";

/// Preferred vertical side of an anchored popup.
///
/// The selected side can differ from this preference when placement
/// resolution is allowed to flip the popup to keep more of it in the
/// viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupPlacement;
/// assert_eq!(PopupPlacement::default(), PopupPlacement::Bottom);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupPlacement {
    /// Place the popup above its anchor before viewport correction.
    Top,
    /// Place the popup below its anchor before viewport correction.
    #[default]
    Bottom,
}

/// Private side-flipping helper used by placement resolution.
impl PopupPlacement {
    /// Returns the other vertical side.
    const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }
}

/// Cross-axis alignment of a popup relative to its anchor.
///
/// `Start` and `End` are logical leading and trailing edges. The current
/// left-to-right geometry resolver maps them to left and right; a future
/// direction-aware host can preserve this public contract while changing that
/// mapping at the provider boundary.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupAlignment;
/// assert_eq!(PopupAlignment::default(), PopupAlignment::Center);
/// assert_ne!(PopupAlignment::Start, PopupAlignment::End);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupAlignment {
    /// Align logical leading edges (currently left edges).
    Start,
    /// Center the popup on the anchor's cross axis.
    #[default]
    Center,
    /// Align logical trailing edges (currently right edges).
    End,
}

/// Provider-neutral placement requested before a host viewport is known.
///
/// The active host combines this semantic geometry with its viewport and
/// backend capabilities through [`resolve_popup_placement`]. Keeping viewport
/// data out of this value prevents widgets from substituting their own bounds
/// for the complete presentation area.
/// All geometry is in logical pixels and is stored without validation; portal
/// publication or placement resolution performs validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size};
/// use ailloli_ui_runtime::popup::PopupPlacementSpec;
/// let spec = PopupPlacementSpec::new(Rect::new(1.0, 2.0, 3.0, 4.0), Size::new(20.0, 10.0));
/// assert_eq!(spec.gap(), 0.0);
/// assert!(spec.allows_flip());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopupPlacementSpec {
    /// Anchor rectangle in global logical pixels.
    anchor: Rect,
    /// Requested popup size in logical pixels.
    desired_size: Size,
    /// Preferred vertical side.
    placement: PopupPlacement,
    /// Cross-axis alignment against the anchor.
    alignment: PopupAlignment,
    /// Non-negative logical-pixel separation from the anchor.
    gap: f32,
    /// Whether viewport resolution may choose the opposite vertical side.
    allow_flip: bool,
}

/// Builder and accessor operations for semantic placement input.
impl PopupPlacementSpec {
    /// Creates bottom-centered placement with zero gap and flipping enabled.
    ///
    /// Values are stored verbatim and validated only when published/resolved.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::{PopupAlignment, PopupPlacement, PopupPlacementSpec};
    /// let spec = PopupPlacementSpec::new(Rect::new(0.0, 0.0, 10.0, 5.0), Size::new(30.0, 20.0));
    /// assert_eq!((spec.placement(), spec.alignment(), spec.gap(), spec.allows_flip()), (PopupPlacement::Bottom, PopupAlignment::Center, 0.0, true));
    /// ```
    pub const fn new(anchor: Rect, desired_size: Size) -> Self {
        Self {
            anchor,
            desired_size,
            placement: PopupPlacement::Bottom,
            alignment: PopupAlignment::Center,
            gap: 0.0,
            allow_flip: true,
        }
    }

    /// Returns the stored global logical-pixel anchor rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::PopupPlacementSpec;
    /// let anchor = Rect::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(PopupPlacementSpec::new(anchor, Size::new(5.0, 6.0)).anchor(), anchor);
    /// ```
    pub const fn anchor(self) -> Rect {
        self.anchor
    }

    /// Returns the requested logical-pixel popup size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::PopupPlacementSpec;
    /// let size = Size::new(20.0, 10.0);
    /// assert_eq!(PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), size).desired_size(), size);
    /// ```
    pub const fn desired_size(self) -> Size {
        self.desired_size
    }

    /// Returns the preferred vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::{PopupPlacement, PopupPlacementSpec};
    /// assert_eq!(PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).placement(), PopupPlacement::Bottom);
    /// ```
    pub const fn placement(self) -> PopupPlacement {
        self.placement
    }

    /// Returns the requested cross-axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::{PopupAlignment, PopupPlacementSpec};
    /// assert_eq!(PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).alignment(), PopupAlignment::Center);
    /// ```
    pub const fn alignment(self) -> PopupAlignment {
        self.alignment
    }

    /// Returns the requested anchor gap in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::PopupPlacementSpec;
    /// assert_eq!(PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).with_gap(4.0).gap(), 4.0);
    /// ```
    pub const fn gap(self) -> f32 {
        self.gap
    }

    /// Returns whether the host may select the opposite vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::PopupPlacementSpec;
    /// assert!(!PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).with_flip(false).allows_flip());
    /// ```
    pub const fn allows_flip(self) -> bool {
        self.allow_flip
    }

    /// Replaces the preferred vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::{PopupPlacement, PopupPlacementSpec};
    /// let spec = PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).with_placement(PopupPlacement::Top);
    /// assert_eq!(spec.placement(), PopupPlacement::Top);
    /// ```
    pub const fn with_placement(mut self, placement: PopupPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Replaces cross-axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::{PopupAlignment, PopupPlacementSpec};
    /// let spec = PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).with_alignment(PopupAlignment::End);
    /// assert_eq!(spec.alignment(), PopupAlignment::End);
    /// ```
    pub const fn with_alignment(mut self, alignment: PopupAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Replaces the logical-pixel gap without validating it.
    ///
    /// Negative and non-finite values are rejected when the spec is published.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::PopupPlacementSpec;
    /// assert_eq!(PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).with_gap(2.5).gap(), 2.5);
    /// ```
    pub const fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Enables or disables automatic vertical-side flipping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::PopupPlacementSpec;
    /// assert!(!PopupPlacementSpec::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default()).with_flip(false).allows_flip());
    /// ```
    pub const fn with_flip(mut self, allow_flip: bool) -> Self {
        self.allow_flip = allow_flip;
        self
    }
}

/// Presentation backend requested or selected for a popup.
///
/// Overlay support is universal. `Native` is only selected when the active
/// host explicitly reports that capability; requesting it never disables the
/// deterministic overlay fallback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupBackend;
/// assert_eq!(PopupBackend::default(), PopupBackend::Overlay);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupBackend {
    /// Mount content in the retained in-window overlay.
    #[default]
    Overlay,
    /// Ask a capable host for a separate native popup presentation.
    Native,
}

/// Popup presentation capabilities reported by a host adapter.
///
/// The safe default is overlay-only. In particular, the winit 0.30 adapter
/// does not advertise native popup support.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
/// assert!(!PopupBackendCapabilities::default().supports(PopupBackend::Native));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PopupBackendCapabilities {
    /// Whether native popup presentation has been validated by this host.
    native: bool,
}

/// Backend support tests and deterministic fallback resolution.
impl PopupBackendCapabilities {
    /// Capabilities for headless hosts and the universal fallback path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
    /// let capabilities = PopupBackendCapabilities::overlay_only();
    /// assert!(capabilities.supports(PopupBackend::Overlay));
    /// assert!(!capabilities.supports(PopupBackend::Native));
    /// ```
    pub const fn overlay_only() -> Self {
        Self { native: false }
    }

    /// Capabilities for a host that has independently validated native popup
    /// presentation while retaining overlay fallback support.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
    /// let capabilities = PopupBackendCapabilities::native_and_overlay();
    /// assert!(capabilities.supports(PopupBackend::Native));
    /// ```
    pub const fn native_and_overlay() -> Self {
        Self { native: true }
    }

    /// Reports direct support for a backend before fallback.
    ///
    /// Overlay is always supported; native support equals the advertised flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
    /// assert!(PopupBackendCapabilities::overlay_only().supports(PopupBackend::Overlay));
    /// ```
    pub const fn supports(self, backend: PopupBackend) -> bool {
        match backend {
            PopupBackend::Overlay => true,
            PopupBackend::Native => self.native,
        }
    }

    /// Selects the request when supported, otherwise the overlay fallback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
    /// let resolution = PopupBackendCapabilities::overlay_only().resolve(PopupBackend::Native);
    /// assert_eq!(resolution.selected(), PopupBackend::Overlay);
    /// assert!(resolution.fell_back());
    /// ```
    pub const fn resolve(self, requested: PopupBackend) -> PopupBackendResolution {
        let selected = if self.supports(requested) {
            requested
        } else {
            PopupBackend::Overlay
        };
        PopupBackendResolution {
            requested,
            selected,
        }
    }
}

/// Observable backend selection, including a native-to-overlay fallback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
/// let value = PopupBackendCapabilities::overlay_only().resolve(PopupBackend::Native);
/// assert_eq!((value.requested(), value.selected()), (PopupBackend::Native, PopupBackend::Overlay));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupBackendResolution {
    /// Original semantic request.
    requested: PopupBackend,
    /// Actually selected supported backend.
    selected: PopupBackend,
}

/// Accessors for requested and selected backend identity.
impl PopupBackendResolution {
    /// Returns the original requested backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
    /// assert_eq!(PopupBackendCapabilities::overlay_only().resolve(PopupBackend::Native).requested(), PopupBackend::Native);
    /// ```
    pub const fn requested(self) -> PopupBackend {
        self.requested
    }

    /// Returns the supported backend chosen for presentation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
    /// assert_eq!(PopupBackendCapabilities::overlay_only().resolve(PopupBackend::Native).selected(), PopupBackend::Overlay);
    /// ```
    pub const fn selected(self) -> PopupBackend {
        self.selected
    }

    /// Reports whether selection differs from the request.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupBackendCapabilities};
    /// assert!(!PopupBackendCapabilities::native_and_overlay().resolve(PopupBackend::Native).fell_back());
    /// ```
    pub fn fell_back(self) -> bool {
        self.requested != self.selected
    }
}

/// Complete provider-neutral input to popup placement resolution.
///
/// All rectangles/sizes/gaps are logical pixels. Construction and builder
/// calls store values verbatim; [`resolve_popup_placement`] validates finite,
/// non-negative geometry and a non-empty viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size};
/// use ailloli_ui_runtime::popup::PopupPlacementInput;
/// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 10.0, 5.0), Size::new(20.0, 10.0), Rect::new(0.0, 0.0, 100.0, 80.0));
/// assert!(input.allows_flip());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopupPlacementInput {
    /// Global anchor rectangle.
    anchor: Rect,
    /// Requested popup size.
    desired_size: Size,
    /// Complete global host viewport.
    viewport: Rect,
    /// Preferred vertical side.
    placement: PopupPlacement,
    /// Cross-axis anchor alignment.
    alignment: PopupAlignment,
    /// Requested anchor separation.
    gap: f32,
    /// Whether vertical flipping is permitted.
    allow_flip: bool,
    /// Preferred presentation backend.
    backend: PopupBackend,
}

/// Builder and accessor operations for complete placement resolution input.
impl PopupPlacementInput {
    /// Creates bottom-centered overlay input with zero gap and flipping enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size};
    /// use ailloli_ui_runtime::popup::{PopupBackend, PopupPlacementInput};
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::new(10.0, 5.0), Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(input.backend(), PopupBackend::Overlay);
    /// assert_eq!(input.gap(), 0.0);
    /// ```
    pub fn new(anchor: Rect, desired_size: Size, viewport: Rect) -> Self {
        Self {
            anchor,
            desired_size,
            viewport,
            placement: PopupPlacement::Bottom,
            alignment: PopupAlignment::Center,
            gap: 0.0,
            allow_flip: true,
            backend: PopupBackend::Overlay,
        }
    }

    /// Returns the stored global logical-pixel anchor.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::PopupPlacementInput;
    /// let anchor = Rect::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(PopupPlacementInput::new(anchor, Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).anchor(), anchor);
    /// ```
    pub const fn anchor(self) -> Rect {
        self.anchor
    }

    /// Returns the requested logical-pixel popup size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::PopupPlacementInput;
    /// let size = Size::new(3.0, 4.0);
    /// assert_eq!(PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), size, Rect::new(0.0, 0.0, 1.0, 1.0)).desired_size(), size);
    /// ```
    pub const fn desired_size(self) -> Size {
        self.desired_size
    }

    /// Returns the complete global logical-pixel viewport.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::PopupPlacementInput;
    /// let viewport = Rect::new(5.0, 6.0, 100.0, 80.0);
    /// assert_eq!(PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), viewport).viewport(), viewport);
    /// ```
    pub const fn viewport(self) -> Rect {
        self.viewport
    }

    /// Returns the preferred vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::{PopupPlacement, PopupPlacementInput};
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0));
    /// assert_eq!(input.placement(), PopupPlacement::Bottom);
    /// ```
    pub const fn placement(self) -> PopupPlacement {
        self.placement
    }

    /// Returns the cross-axis anchor alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::{PopupAlignment, PopupPlacementInput};
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0));
    /// assert_eq!(input.alignment(), PopupAlignment::Center);
    /// ```
    pub const fn alignment(self) -> PopupAlignment {
        self.alignment
    }

    /// Returns the requested logical-pixel gap.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::PopupPlacementInput;
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_gap(3.0);
    /// assert_eq!(input.gap(), 3.0);
    /// ```
    pub const fn gap(self) -> f32 {
        self.gap
    }

    /// Returns whether vertical side flipping is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::PopupPlacementInput;
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_flip(false);
    /// assert!(!input.allows_flip());
    /// ```
    pub const fn allows_flip(self) -> bool {
        self.allow_flip
    }

    /// Returns the requested presentation backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::{PopupBackend, PopupPlacementInput};
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_backend(PopupBackend::Native);
    /// assert_eq!(input.backend(), PopupBackend::Native);
    /// ```
    pub const fn backend(self) -> PopupBackend {
        self.backend
    }

    /// Replaces the preferred vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::{PopupPlacement, PopupPlacementInput};
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_placement(PopupPlacement::Top);
    /// assert_eq!(input.placement(), PopupPlacement::Top);
    /// ```
    pub const fn with_placement(mut self, placement: PopupPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Replaces the cross-axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::{PopupAlignment, PopupPlacementInput};
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_alignment(PopupAlignment::Start);
    /// assert_eq!(input.alignment(), PopupAlignment::Start);
    /// ```
    pub const fn with_alignment(mut self, alignment: PopupAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Replaces the logical-pixel gap without validating it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::PopupPlacementInput;
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_gap(2.0);
    /// assert_eq!(input.gap(), 2.0);
    /// ```
    pub const fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Enables or disables automatic vertical flipping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::PopupPlacementInput;
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_flip(false);
    /// assert!(!input.allows_flip());
    /// ```
    pub const fn with_flip(mut self, allow_flip: bool) -> Self {
        self.allow_flip = allow_flip;
        self
    }

    /// Replaces the preferred backend; capability fallback occurs at resolve.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::{PopupBackend, PopupPlacementInput};
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::default(), Rect::new(0.0, 0.0, 1.0, 1.0)).with_backend(PopupBackend::Native);
    /// assert_eq!(input.backend(), PopupBackend::Native);
    /// ```
    pub const fn with_backend(mut self, backend: PopupBackend) -> Self {
        self.backend = backend;
        self
    }
}

/// Deterministic result of backend and popup geometry resolution.
///
/// `bounds` is a finite rectangle clamped inside the input viewport in global
/// logical pixels. `flipped` compares selected/requested side; `clamped` covers
/// both size reduction and position correction.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size};
/// use ailloli_ui_runtime::popup::{resolve_popup_placement, PopupBackendCapabilities, PopupPlacementInput};
/// let result = resolve_popup_placement(PopupPlacementInput::new(Rect::new(10.0, 10.0, 10.0, 5.0), Size::new(20.0, 10.0), Rect::new(0.0, 0.0, 100.0, 100.0)), PopupBackendCapabilities::overlay_only())?;
/// assert_eq!(result.bounds().w, 20.0);
/// # Ok::<(), ailloli_ui_runtime::popup::PopupPlacementError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPopupPlacement {
    /// Requested and selected backend.
    backend: PopupBackendResolution,
    /// Final viewport-contained global bounds.
    bounds: Rect,
    /// Selected vertical side.
    placement: PopupPlacement,
    /// Whether the selected side differs from the request.
    flipped: bool,
    /// Whether size or position required viewport correction.
    clamped: bool,
}

/// Accessors for one immutable placement result.
impl ResolvedPopupPlacement {
    /// Returns requested/selected backend resolution.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::*;
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 1.0, 1.0), Size::new(1.0, 1.0), Rect::new(0.0, 0.0, 10.0, 10.0)).with_backend(PopupBackend::Native);
    /// assert!(resolve_popup_placement(input, PopupBackendCapabilities::overlay_only())?.backend().fell_back());
    /// # Ok::<(), PopupPlacementError>(())
    /// ```
    pub const fn backend(self) -> PopupBackendResolution {
        self.backend
    }

    /// Returns final global logical-pixel bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::*;
    /// let result = resolve_popup_placement(PopupPlacementInput::new(Rect::new(0.0, 0.0, 2.0, 2.0), Size::new(4.0, 3.0), Rect::new(0.0, 0.0, 10.0, 10.0)), PopupBackendCapabilities::overlay_only())?;
    /// assert_eq!(result.bounds().w, 4.0);
    /// # Ok::<(), PopupPlacementError>(())
    /// ```
    pub const fn bounds(self) -> Rect {
        self.bounds
    }

    /// Returns the selected side after optional flipping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::*;
    /// let input = PopupPlacementInput::new(Rect::new(10.0, 90.0, 5.0, 5.0), Size::new(10.0, 20.0), Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert_eq!(resolve_popup_placement(input, PopupBackendCapabilities::overlay_only())?.placement(), PopupPlacement::Top);
    /// # Ok::<(), PopupPlacementError>(())
    /// ```
    pub const fn placement(self) -> PopupPlacement {
        self.placement
    }

    /// Reports whether resolution selected the opposite vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::*;
    /// let input = PopupPlacementInput::new(Rect::new(10.0, 90.0, 5.0, 5.0), Size::new(10.0, 20.0), Rect::new(0.0, 0.0, 100.0, 100.0));
    /// assert!(resolve_popup_placement(input, PopupBackendCapabilities::overlay_only())?.flipped());
    /// # Ok::<(), PopupPlacementError>(())
    /// ```
    pub const fn flipped(self) -> bool {
        self.flipped
    }

    /// Reports whether desired size or positioned bounds were viewport-corrected.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::*;
    /// let input = PopupPlacementInput::new(Rect::new(0.0, 0.0, 0.0, 0.0), Size::new(50.0, 50.0), Rect::new(0.0, 0.0, 10.0, 10.0));
    /// assert!(resolve_popup_placement(input, PopupBackendCapabilities::overlay_only())?.clamped());
    /// # Ok::<(), PopupPlacementError>(())
    /// ```
    pub const fn clamped(self) -> bool {
        self.clamped
    }
}

/// Invalid geometry supplied to popup placement resolution.
///
/// Validation is deterministic and performs no platform calls. The enum is
/// non-exhaustive so downstream matches require a wildcard arm.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size};
/// use ailloli_ui_runtime::popup::{position_popup, PopupAlignment, PopupPlacement, PopupPlacementError};
/// assert_eq!(position_popup(Rect::new(0.0, 0.0, 0.0, 0.0), Size::new(-1.0, 2.0), PopupPlacement::Bottom, PopupAlignment::Center, 0.0), Err(PopupPlacementError::InvalidDesiredSize));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PopupPlacementError {
    /// Anchor coordinate/size is non-finite or a dimension is negative.
    #[error("popup anchor must be finite and non-negative in size")]
    InvalidAnchor,
    /// Desired width/height is non-finite or negative.
    #[error("popup desired size must be finite and non-negative")]
    InvalidDesiredSize,
    /// Viewport coordinate/size is non-finite or a dimension is negative.
    #[error("popup viewport must be finite and non-negative in size")]
    InvalidViewport,
    /// Viewport width or height is exactly zero after basic validation.
    #[error("popup viewport must have a positive width and height")]
    EmptyViewport,
    /// Gap is negative or non-finite.
    #[error("popup gap must be finite and non-negative")]
    InvalidGap,
    /// A retained request was resolved before receiving an anchor.
    #[error("popup request has no anchor")]
    MissingAnchor,
    /// A retained request was resolved before receiving desired size.
    #[error("popup request has no desired size")]
    MissingDesiredSize,
    /// Finite inputs overflowed to non-finite positioned/clamped edges.
    #[error("popup geometry cannot be represented with finite coordinates")]
    UnrepresentableGeometry,
}

/// Resolves popup side, alignment, flip, viewport clamp, and backend fallback.
///
/// Flipping occurs only when the preferred side cannot contain the resolved
/// height and the opposite side either can contain it or offers strictly more
/// space. The final rectangle is always clamped to the viewport, including a
/// deterministic size reduction when the desired popup is larger than it.
/// All input/output geometry is in global logical pixels. Validation order is
/// anchor, desired size, viewport, then gap; backend fallback is not an error.
///
/// # Errors
///
/// Returns [`PopupPlacementError`] for invalid/missing representable geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::*;
/// let input = PopupPlacementInput::new(Rect::new(40.0, 90.0, 20.0, 5.0), Size::new(30.0, 20.0), Rect::new(0.0, 0.0, 100.0, 100.0));
/// let result = resolve_popup_placement(input, PopupBackendCapabilities::overlay_only())?;
/// assert_eq!(result.placement(), PopupPlacement::Top);
/// assert_eq!(result.bounds(), Rect::new(35.0, 70.0, 30.0, 20.0));
/// # Ok::<(), PopupPlacementError>(())
/// ```
pub fn resolve_popup_placement(
    input: PopupPlacementInput,
    capabilities: PopupBackendCapabilities,
) -> Result<ResolvedPopupPlacement, PopupPlacementError> {
    validate_anchor(input.anchor)?;
    validate_desired_size(input.desired_size)?;
    validate_viewport(input.viewport)?;
    validate_gap(input.gap)?;

    let resolved_size = Size::new(
        input.desired_size.w.min(input.viewport.w),
        input.desired_size.h.min(input.viewport.h),
    );
    let preferred_space =
        available_vertical_space(input.anchor, input.viewport, input.placement, input.gap);
    let opposite = input.placement.opposite();
    let opposite_space =
        available_vertical_space(input.anchor, input.viewport, opposite, input.gap);
    let preferred_fits = resolved_size.h <= preferred_space;
    let opposite_fits = resolved_size.h <= opposite_space;
    let placement = if input.allow_flip
        && !preferred_fits
        && (opposite_fits || opposite_space > preferred_space)
    {
        opposite
    } else {
        input.placement
    };

    let positioned = position_popup_unchecked(
        input.anchor,
        resolved_size,
        placement,
        input.alignment,
        input.gap,
    )?;
    let bounds = clamp_popup_to_viewport(positioned, input.viewport)?;
    let clamped = resolved_size != input.desired_size || bounds != positioned;

    Ok(ResolvedPopupPlacement {
        backend: capabilities.resolve(input.backend),
        bounds,
        placement,
        flipped: placement != input.placement,
        clamped,
    })
}

/// Positions a popup relative to an anchor without viewport flip or clamp.
///
/// Procedural overlays use this primitive before their window viewport is
/// available. Once a viewport is known, prefer [`resolve_popup_placement`].
/// All geometry is global logical pixels. Center alignment may produce negative
/// coordinates; no clipping occurs here.
///
/// # Errors
///
/// Returns typed validation errors or `UnrepresentableGeometry` on arithmetic
/// overflow to non-finite edges.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size}; use ailloli_ui_runtime::popup::*;
/// let bounds = position_popup(Rect::new(10.0, 20.0, 20.0, 5.0), Size::new(10.0, 4.0), PopupPlacement::Bottom, PopupAlignment::Center, 2.0)?;
/// assert_eq!(bounds, Rect::new(15.0, 27.0, 10.0, 4.0));
/// # Ok::<(), PopupPlacementError>(())
/// ```
pub fn position_popup(
    anchor: Rect,
    desired_size: Size,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    gap: f32,
) -> Result<Rect, PopupPlacementError> {
    validate_anchor(anchor)?;
    validate_desired_size(desired_size)?;
    validate_gap(gap)?;
    position_popup_unchecked(anchor, desired_size, placement, alignment, gap)
}

/// Clamps popup bounds to a viewport without tracking a requested side.
///
/// Both size and origin are corrected: oversize dimensions shrink to viewport
/// dimensions, then origin clamps so all edges remain inside. Inputs/output are
/// global logical pixels.
///
/// # Errors
///
/// Invalid bounds map to `UnrepresentableGeometry`; invalid/empty viewport uses
/// its specific validation error.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect; use ailloli_ui_runtime::popup::{clamp_popup_to_viewport, PopupPlacementError};
/// assert_eq!(clamp_popup_to_viewport(Rect::new(-5.0, 8.0, 30.0, 20.0), Rect::new(0.0, 0.0, 10.0, 10.0))?, Rect::new(0.0, 0.0, 10.0, 10.0));
/// # Ok::<(), PopupPlacementError>(())
/// ```
pub fn clamp_popup_to_viewport(bounds: Rect, viewport: Rect) -> Result<Rect, PopupPlacementError> {
    validate_anchor(bounds).map_err(|_| PopupPlacementError::UnrepresentableGeometry)?;
    validate_viewport(viewport)?;

    let width = bounds.w.min(viewport.w);
    let height = bounds.h.min(viewport.h);
    let max_x = viewport.right() - width;
    let max_y = viewport.bottom() - height;
    let x = bounds.x.clamp(viewport.x, max_x);
    let y = bounds.y.clamp(viewport.y, max_y);
    let clamped = Rect::new(x, y, width, height);
    if rect_has_finite_edges(clamped) {
        Ok(clamped)
    } else {
        Err(PopupPlacementError::UnrepresentableGeometry)
    }
}

/// Stable identity of one retained element tree.
///
/// `ElementId` values are only unique within a tree, so popup ownership always
/// carries both identifiers.
/// Zero is a valid compatibility namespace and no value is reserved by the
/// type itself.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::ElementTreeId;
/// assert_eq!(ElementTreeId::new(7).get(), 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementTreeId(u64);

/// Construction and extraction for a retained-tree namespace.
impl ElementTreeId {
    /// Wraps an arbitrary `u64` without validation or allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::ElementTreeId;
    /// assert_eq!(ElementTreeId::new(0).get(), 0);
    /// ```
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the exact stored integer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::ElementTreeId;
    /// assert_eq!(ElementTreeId::new(u64::MAX).get(), u64::MAX);
    /// ```
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identity of one popup registration.
///
/// The value type does not allocate or reserve identifiers. [`PopupPortal`]
/// allocates one-based checked IDs, while callers may explicitly construct zero.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupId;
/// assert_eq!(PopupId::new(1).get(), 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PopupId(u64);

/// Construction and extraction for popup identity.
impl PopupId {
    /// Wraps an arbitrary `u64` without registration or validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupId;
    /// assert_eq!(PopupId::new(0).get(), 0);
    /// ```
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the exact stored integer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupId;
    /// assert_eq!(PopupId::new(42).get(), 42);
    /// ```
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Complete identity of the element that owns a popup.
///
/// Logical window, native presentation generation, retained-tree namespace,
/// and tree-local element ID are all required to avoid stale/cross-window
/// aliasing. Values are stored without liveness validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, popup::{ElementTreeId, PopupOwner}};
/// let owner = PopupOwner::new("main", PresentationGeneration::new(2), ElementTreeId::new(3), ElementId(4));
/// assert_eq!(owner.logical_window_id().as_str(), "main");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PopupOwner {
    /// Stable application-level logical window.
    logical_window_id: LogicalWindowId,
    /// Native presentation generation preventing stale surface reuse.
    presentation_generation: PresentationGeneration,
    /// Retained-tree namespace.
    element_tree_id: ElementTreeId,
    /// Tree-local owner element.
    element_id: ElementId,
}

/// Construction, field access, and exact presentation matching for owners.
impl PopupOwner {
    /// Creates a complete owner identity without checking current liveness.
    ///
    /// Empty logical-window IDs and zero numeric IDs are stored verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, popup::{ElementTreeId, PopupOwner}};
    /// let owner = PopupOwner::new("editor", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1));
    /// assert_eq!(owner.element_id(), ElementId(1));
    /// ```
    pub fn new(
        logical_window_id: impl Into<LogicalWindowId>,
        presentation_generation: PresentationGeneration,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
    ) -> Self {
        Self {
            logical_window_id: logical_window_id.into(),
            presentation_generation,
            element_tree_id,
            element_id,
        }
    }

    /// Borrows the stable logical-window ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, popup::{ElementTreeId, PopupOwner}};
    /// let owner = PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1));
    /// assert_eq!(owner.logical_window_id().as_str(), "main");
    /// ```
    pub fn logical_window_id(&self) -> &LogicalWindowId {
        &self.logical_window_id
    }

    /// Returns the exact native presentation generation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, popup::{ElementTreeId, PopupOwner}};
    /// let owner = PopupOwner::new("main", PresentationGeneration::new(9), ElementTreeId::new(0), ElementId(1));
    /// assert_eq!(owner.presentation_generation(), PresentationGeneration::new(9));
    /// ```
    pub const fn presentation_generation(&self) -> PresentationGeneration {
        self.presentation_generation
    }

    /// Returns the retained-tree namespace.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, popup::{ElementTreeId, PopupOwner}};
    /// let owner = PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(4), ElementId(1));
    /// assert_eq!(owner.element_tree_id(), ElementTreeId::new(4));
    /// ```
    pub const fn element_tree_id(&self) -> ElementTreeId {
        self.element_tree_id
    }

    /// Returns the tree-local owner element ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, popup::{ElementTreeId, PopupOwner}};
    /// let owner = PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(6));
    /// assert_eq!(owner.element_id(), ElementId(6));
    /// ```
    pub const fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns whether this owner belongs to the active presentation of a
    /// logical window.
    /// Tree and element IDs are deliberately irrelevant to this comparison.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId}; use ailloli_ui_runtime::{app::PresentationGeneration, popup::{ElementTreeId, PopupOwner}};
    /// let owner = PopupOwner::new("main", PresentationGeneration::new(2), ElementTreeId::new(3), ElementId(4));
    /// assert!(owner.belongs_to(&LogicalWindowId::new("main"), PresentationGeneration::new(2)));
    /// assert!(!owner.belongs_to(&LogicalWindowId::new("main"), PresentationGeneration::new(3)));
    /// ```
    pub fn belongs_to(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> bool {
        self.logical_window_id == *logical_window_id
            && self.presentation_generation == presentation_generation
    }
}

/// Retained factory used to remount popup content in either an overlay tree or
/// a native presentation.
///
/// The UI-thread-local `Rc` makes clones share one factory without requiring
/// `Send` or `Sync`. Each [`Self::build`] invocation calls the factory again and
/// returns a fresh declarative view.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::{component::View, popup::PopupContent};
/// let content = PopupContent::<()>::new(|| View::empty().key("menu"));
/// assert_eq!(content.build().key_ref(), Some("menu"));
/// ```
pub struct PopupContent<A> {
    /// Shared remount factory.
    factory: Rc<dyn Fn() -> View<A>>,
}

/// Factory construction and invocation.
impl<A> PopupContent<A> {
    /// Stores a UI-thread-local `'static` view factory without invoking it.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::Cell, rc::Rc}; use ailloli_ui_runtime::{component::View, popup::PopupContent};
    /// let calls = Rc::new(Cell::new(0)); let seen = calls.clone();
    /// let content = PopupContent::<()>::new(move || { seen.set(seen.get() + 1); View::empty() });
    /// assert_eq!(calls.get(), 0); content.build(); assert_eq!(calls.get(), 1);
    /// ```
    pub fn new(factory: impl Fn() -> View<A> + 'static) -> Self {
        Self {
            factory: Rc::new(factory),
        }
    }

    /// Invokes the retained factory once and returns its new view.
    ///
    /// # Panics
    ///
    /// Propagates a factory panic.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::{component::View, popup::PopupContent};
    /// let content = PopupContent::<()>::new(|| View::empty().key("popup"));
    /// assert_eq!(content.build().key_ref(), Some("popup"));
    /// ```
    pub fn build(&self) -> View<A> {
        (self.factory)()
    }
}

/// Clones the shared factory rather than invoking or duplicating it.
impl<A> Clone for PopupContent<A> {
    /// Increments the factory's strong count.
    fn clone(&self) -> Self {
        Self {
            factory: Rc::clone(&self.factory),
        }
    }
}

/// Redacts the closure internals from debug output.
impl<A> fmt::Debug for PopupContent<A> {
    /// Writes the stable placeholder `PopupContent(<factory>)`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PopupContent(<factory>)")
    }
}

/// Rendering ownership selected for one popup registration.
///
/// New popup content is mounted into the provider-neutral retained overlay by
/// default. Widgets that still draw their own overlay must opt into
/// [`Self::ProceduralFallback`] until their shell, placement, and interaction
/// have migrated to the retained subtree.
///
/// The portal keeps two fixed z-order strata: procedural fallbacks are always
/// below retained overlays. Opening a popup raises it only within its own
/// stratum, so paint, hit-testing, outside dismissal, and Escape all agree on
/// the same topmost popup during the migration.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupMountPolicy;
/// assert_eq!(PopupMountPolicy::default(), PopupMountPolicy::RetainedOverlay);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupMountPolicy {
    /// Mount the popup factory into the runtime-owned retained overlay tree.
    #[default]
    RetainedOverlay,
    /// Keep widget-owned procedural drawing in the lower migration stratum.
    ProceduralFallback,
}

/// Semantic role exposed by a popup independently from its presentation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupRole;
/// assert_eq!(PopupRole::default(), PopupRole::Generic);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupRole {
    /// Popup with no more specific semantic category.
    #[default]
    Generic,
    /// Selectable list of options.
    Listbox,
    /// Command/action menu.
    Menu,
    /// Non-interactive explanatory tooltip.
    Tooltip,
    /// Modal dialog whose retained descendants own focus until dismissal.
    Dialog,
}

/// Focus behavior requested when the popup becomes visible.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupFocusPolicy;
/// assert_eq!(PopupFocusPolicy::default(), PopupFocusPolicy::None);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupFocusPolicy {
    /// Preserve current focus.
    #[default]
    None,
    /// Move focus to the first focusable popup descendant.
    MoveIntoPopup,
    /// Move focus into the popup and request focus trapping within it.
    TrapWithinPopup,
}

/// Provider-neutral interaction and accessibility contract for a popup.
///
/// Defaults describe an interactive generic popup: outside press/Escape close
/// it, pointer input is consumed, and focus is restored on close, but opening
/// does not move focus. Builder booleans mean exactly whether each behavior is
/// requested; the portal does not infer a role-specific override.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupSemantics;
/// let semantics = PopupSemantics::new();
/// assert!(semantics.dismisses_on_escape() && semantics.consumes_pointer_input());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupSemantics {
    /// Accessibility/interaction category.
    role: PopupRole,
    /// Focus action emitted on first open.
    focus_policy: PopupFocusPolicy,
    /// Whether a press outside this popup subtree dismisses it.
    dismiss_on_outside_press: bool,
    /// Whether Escape dismisses this popup.
    dismiss_on_escape: bool,
    /// Whether pointer input inside/opening dismissal is considered handled.
    consume_pointer_input: bool,
    /// Whether closing emits restoration to the complete owner identity.
    restore_focus_on_close: bool,
}

/// Uses the interactive defaults from [`PopupSemantics::new`].
impl Default for PopupSemantics {
    /// Returns [`PopupSemantics::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Constructors, accessors, and immutable-style semantic builders.
impl PopupSemantics {
    /// Creates the interactive generic defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupFocusPolicy, PopupRole, PopupSemantics};
    /// let value = PopupSemantics::new();
    /// assert_eq!((value.role(), value.focus_policy()), (PopupRole::Generic, PopupFocusPolicy::None));
    /// assert!(value.dismisses_on_outside_press() && value.dismisses_on_escape() && value.consumes_pointer_input() && value.restores_focus_on_close());
    /// ```
    pub const fn new() -> Self {
        Self {
            role: PopupRole::Generic,
            focus_policy: PopupFocusPolicy::None,
            dismiss_on_outside_press: true,
            dismiss_on_escape: true,
            consume_pointer_input: true,
            restore_focus_on_close: true,
        }
    }

    /// Returns the semantic category.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupRole, PopupSemantics};
    /// assert_eq!(PopupSemantics::new().with_role(PopupRole::Menu).role(), PopupRole::Menu);
    /// ```
    pub const fn role(&self) -> PopupRole {
        self.role
    }

    /// Returns requested focus behavior on open.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupFocusPolicy, PopupSemantics};
    /// assert_eq!(PopupSemantics::new().with_focus_policy(PopupFocusPolicy::MoveIntoPopup).focus_policy(), PopupFocusPolicy::MoveIntoPopup);
    /// ```
    pub const fn focus_policy(&self) -> PopupFocusPolicy {
        self.focus_policy
    }

    /// Reports whether an outside press requests dismissal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().dismiss_on_outside_press(false).dismisses_on_outside_press());
    /// ```
    pub const fn dismisses_on_outside_press(&self) -> bool {
        self.dismiss_on_outside_press
    }

    /// Reports whether Escape requests dismissal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().dismiss_on_escape(false).dismisses_on_escape());
    /// ```
    pub const fn dismisses_on_escape(&self) -> bool {
        self.dismiss_on_escape
    }

    /// Reports whether routed pointer input is considered handled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().consume_pointer_input(false).consumes_pointer_input());
    /// ```
    pub const fn consumes_pointer_input(&self) -> bool {
        self.consume_pointer_input
    }

    /// Reports whether close emits owner focus restoration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().restore_focus_on_close(false).restores_focus_on_close());
    /// ```
    pub const fn restores_focus_on_close(&self) -> bool {
        self.restore_focus_on_close
    }

    /// Replaces the semantic category without changing other behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupRole, PopupSemantics};
    /// assert_eq!(PopupSemantics::new().with_role(PopupRole::Listbox).role(), PopupRole::Listbox);
    /// ```
    pub const fn with_role(mut self, role: PopupRole) -> Self {
        self.role = role;
        self
    }

    /// Replaces focus-on-open behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupFocusPolicy, PopupSemantics};
    /// assert_eq!(PopupSemantics::new().with_focus_policy(PopupFocusPolicy::TrapWithinPopup).focus_policy(), PopupFocusPolicy::TrapWithinPopup);
    /// ```
    pub const fn with_focus_policy(mut self, focus_policy: PopupFocusPolicy) -> Self {
        self.focus_policy = focus_policy;
        self
    }

    /// Sets outside-press dismissal exactly to `dismiss`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().dismiss_on_outside_press(false).dismisses_on_outside_press());
    /// ```
    pub const fn dismiss_on_outside_press(mut self, dismiss: bool) -> Self {
        self.dismiss_on_outside_press = dismiss;
        self
    }

    /// Sets Escape dismissal exactly to `dismiss`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().dismiss_on_escape(false).dismisses_on_escape());
    /// ```
    pub const fn dismiss_on_escape(mut self, dismiss: bool) -> Self {
        self.dismiss_on_escape = dismiss;
        self
    }

    /// Sets pointer-input consumption exactly to `consume`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().consume_pointer_input(false).consumes_pointer_input());
    /// ```
    pub const fn consume_pointer_input(mut self, consume: bool) -> Self {
        self.consume_pointer_input = consume;
        self
    }

    /// Sets owner focus restoration exactly to `restore`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupSemantics;
    /// assert!(!PopupSemantics::new().restore_focus_on_close(false).restores_focus_on_close());
    /// ```
    pub const fn restore_focus_on_close(mut self, restore: bool) -> Self {
        self.restore_focus_on_close = restore;
        self
    }

    /// Non-interactive semantics suitable for a tooltip.
    ///
    /// Role is tooltip; Escape dismissal stays enabled, while outside press,
    /// pointer consumption, and focus restoration are disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupRole, PopupSemantics};
    /// let value = PopupSemantics::tooltip();
    /// assert_eq!(value.role(), PopupRole::Tooltip);
    /// assert!(value.dismisses_on_escape());
    /// assert!(!value.dismisses_on_outside_press() && !value.consumes_pointer_input());
    /// assert!(!value.restores_focus_on_close());
    /// ```
    pub const fn tooltip() -> Self {
        Self::new()
            .with_role(PopupRole::Tooltip)
            .dismiss_on_escape(true)
            .dismiss_on_outside_press(false)
            .consume_pointer_input(false)
            .restore_focus_on_close(false)
    }
}

/// Registration contract for one popup.
///
/// A request combines immutable identity/ownership, optional parent and
/// geometry, backend/placement preferences, a remountable content factory, and
/// interaction semantics. Defaults are closed-registration friendly: no parent,
/// anchor, or desired size; bottom-center, zero gap, flip enabled; overlay
/// backend and retained-overlay mounting. Builders store geometry verbatim;
/// portal publication/resolution validates it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
/// let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
/// assert_eq!(request.parent(), None);
/// assert_eq!(request.mount_policy(), PopupMountPolicy::RetainedOverlay);
/// ```
pub struct PopupRequest<A> {
    /// Stable registry identity.
    id: PopupId,
    /// Complete retained owner identity.
    owner: PopupOwner,
    /// Optional parent popup in the same presentation.
    parent: Option<PopupId>,
    /// Optional global logical-pixel anchor.
    anchor: Option<Rect>,
    /// Optional requested logical-pixel size.
    desired_size: Option<Size>,
    /// Preferred vertical side.
    placement: PopupPlacement,
    /// Cross-axis alignment.
    alignment: PopupAlignment,
    /// Logical-pixel anchor separation.
    gap: f32,
    /// Whether viewport resolution may flip sides.
    allow_flip: bool,
    /// Preferred presentation backend.
    backend: PopupBackend,
    /// Remountable declarative content.
    content: PopupContent<A>,
    /// Interaction/accessibility behavior.
    semantics: PopupSemantics,
    /// Retained-versus-procedural migration stratum.
    mount_policy: PopupMountPolicy,
}

/// Construction, immutable access, builders, and placement resolution.
impl<A> PopupRequest<A> {
    /// Creates a closed-registration request with semantic defaults.
    ///
    /// IDs/owner liveness are not checked and content is not built.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let request = PopupRequest::<()>::new(PopupId::new(2), PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(3)), PopupContent::new(View::empty));
    /// assert_eq!(request.id(), PopupId::new(2));
    /// assert_eq!((request.anchor(), request.desired_size()), (None, None));
    /// ```
    pub fn new(id: PopupId, owner: PopupOwner, content: PopupContent<A>) -> Self {
        Self {
            id,
            owner,
            parent: None,
            anchor: None,
            desired_size: None,
            placement: PopupPlacement::Bottom,
            alignment: PopupAlignment::Center,
            gap: 0.0,
            allow_flip: true,
            backend: PopupBackend::Overlay,
            content,
            semantics: PopupSemantics::default(),
            mount_policy: PopupMountPolicy::default(),
        }
    }

    /// Returns the stable popup registration ID.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(7), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.id(), PopupId::new(7));
    /// ```
    pub const fn id(&self) -> PopupId {
        self.id
    }

    /// Borrows the complete popup owner identity.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(9)), PopupContent::new(View::empty));
    /// assert_eq!(request.owner().element_id(), ElementId(9));
    /// ```
    pub const fn owner(&self) -> &PopupOwner {
        &self.owner
    }

    /// Returns the optional parent popup ID.
    ///
    /// `None` denotes a root popup, not an unknown parent.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.parent(), None);
    /// ```
    pub const fn parent(&self) -> Option<PopupId> {
        self.parent
    }

    /// Returns optional global logical-pixel anchor geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{ElementId, Rect};
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_anchor(Rect::new(1.0, 2.0, 3.0, 4.0)).anchor(), Some(Rect::new(1.0, 2.0, 3.0, 4.0)));
    /// ```
    pub const fn anchor(&self) -> Option<Rect> {
        self.anchor
    }

    /// Returns optional requested logical-pixel popup size.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{ElementId, Size};
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_desired_size(Size::new(20.0, 10.0)).desired_size(), Some(Size::new(20.0, 10.0)));
    /// ```
    pub const fn desired_size(&self) -> Option<Size> {
        self.desired_size
    }

    /// Returns the preferred vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.placement(), PopupPlacement::Bottom);
    /// ```
    pub const fn placement(&self) -> PopupPlacement {
        self.placement
    }

    /// Returns cross-axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.alignment(), PopupAlignment::Center);
    /// ```
    pub const fn alignment(&self) -> PopupAlignment {
        self.alignment
    }

    /// Returns requested anchor gap in logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.gap(), 0.0);
    /// ```
    pub const fn gap(&self) -> f32 {
        self.gap
    }

    /// Reports whether viewport resolution may flip vertical sides.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert!(request.allows_flip());
    /// ```
    pub const fn allows_flip(&self) -> bool {
        self.allow_flip
    }

    /// Returns the preferred presentation backend.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.backend(), PopupBackend::Overlay);
    /// ```
    pub const fn backend(&self) -> PopupBackend {
        self.backend
    }

    /// Borrows the remountable content factory without invoking it.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(|| View::empty().key("content")));
    /// assert_eq!(request.content().build().key_ref(), Some("content"));
    /// ```
    pub const fn content(&self) -> &PopupContent<A> {
        &self.content
    }

    /// Borrows interaction/accessibility semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.semantics().role(), PopupRole::Generic);
    /// ```
    pub const fn semantics(&self) -> &PopupSemantics {
        &self.semantics
    }

    /// Returns retained/procedural mounting policy.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.mount_policy(), PopupMountPolicy::RetainedOverlay);
    /// ```
    pub const fn mount_policy(&self) -> PopupMountPolicy {
        self.mount_policy
    }

    /// Assigns a parent popup ID.
    ///
    /// Registration later requires that parent to exist in the same presentation;
    /// opening later requires it to be open.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(2), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_parent(PopupId::new(1)).parent(), Some(PopupId::new(1)));
    /// ```
    pub const fn with_parent(mut self, parent: PopupId) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Sets global logical-pixel anchor geometry without validation.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{ElementId, Rect};
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// let anchor = Rect::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(request.with_anchor(anchor).anchor(), Some(anchor));
    /// ```
    pub const fn with_anchor(mut self, anchor: Rect) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// Sets requested logical-pixel size without validation.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{ElementId, Size};
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// let size = Size::new(20.0, 10.0);
    /// assert_eq!(request.with_desired_size(size).desired_size(), Some(size));
    /// ```
    pub const fn with_desired_size(mut self, desired_size: Size) -> Self {
        self.desired_size = Some(desired_size);
        self
    }

    /// Replaces preferred vertical side.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_placement(PopupPlacement::Top).placement(), PopupPlacement::Top);
    /// ```
    pub const fn with_placement(mut self, placement: PopupPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Replaces cross-axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_alignment(PopupAlignment::End).alignment(), PopupAlignment::End);
    /// ```
    pub const fn with_alignment(mut self, alignment: PopupAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Replaces logical-pixel gap without validation.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_gap(4.0).gap(), 4.0);
    /// ```
    pub const fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Enables or disables vertical-side flipping.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert!(!request.with_flip(false).allows_flip());
    /// ```
    pub const fn with_flip(mut self, allow_flip: bool) -> Self {
        self.allow_flip = allow_flip;
        self
    }

    /// Replaces preferred backend; capability fallback occurs during resolution.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_backend(PopupBackend::Native).backend(), PopupBackend::Native);
    /// ```
    pub const fn with_backend(mut self, backend: PopupBackend) -> Self {
        self.backend = backend;
        self
    }

    /// Replaces the complete semantic contract.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_semantics(PopupSemantics::tooltip()).semantics().role(), PopupRole::Tooltip);
    /// ```
    pub fn with_semantics(mut self, semantics: PopupSemantics) -> Self {
        self.semantics = semantics;
        self
    }

    /// Replaces retained/procedural mounting policy.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::ElementId;
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// assert_eq!(request.with_mount_policy(PopupMountPolicy::ProceduralFallback).mount_policy(), PopupMountPolicy::ProceduralFallback);
    /// ```
    pub const fn with_mount_policy(mut self, mount_policy: PopupMountPolicy) -> Self {
        self.mount_policy = mount_policy;
        self
    }

    /// Resolves this request against a host viewport and its advertised popup
    /// capabilities.
    ///
    /// Registration remains valid without geometry because declaratively open
    /// popups can be mounted before their first layout. This method reports a
    /// typed missing-field error until both anchor and desired size are known.
    /// All geometry is logical pixels. Backend selection uses `capabilities`;
    /// the request itself is not mutated.
    ///
    /// # Errors
    ///
    /// Returns `MissingAnchor`, then `MissingDesiredSize`, or a validation/
    /// representation error from [`resolve_popup_placement`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{ElementId, Rect, Size};
    /// # use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// # let request = PopupRequest::<()>::new(PopupId::new(1), PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// let request = request.with_anchor(Rect::new(10.0, 10.0, 5.0, 5.0)).with_desired_size(Size::new(20.0, 10.0));
    /// let resolved = request.resolve_placement(Rect::new(0.0, 0.0, 100.0, 100.0), PopupBackendCapabilities::overlay_only())?;
    /// assert_eq!(resolved.bounds().w, 20.0);
    /// # Ok::<(), PopupPlacementError>(())
    /// ```
    pub fn resolve_placement(
        &self,
        viewport: Rect,
        capabilities: PopupBackendCapabilities,
    ) -> Result<ResolvedPopupPlacement, PopupPlacementError> {
        let anchor = self.anchor.ok_or(PopupPlacementError::MissingAnchor)?;
        let desired_size = self
            .desired_size
            .ok_or(PopupPlacementError::MissingDesiredSize)?;
        resolve_popup_placement(
            PopupPlacementInput::new(anchor, desired_size, viewport)
                .with_placement(self.placement)
                .with_alignment(self.alignment)
                .with_gap(self.gap)
                .with_flip(self.allow_flip)
                .with_backend(self.backend),
            capabilities,
        )
    }
}

/// Clones every value and shares the content factory's `Rc`.
impl<A> Clone for PopupRequest<A> {
    /// Produces an equivalent independent request value.
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            owner: self.owner.clone(),
            parent: self.parent,
            anchor: self.anchor,
            desired_size: self.desired_size,
            placement: self.placement,
            alignment: self.alignment,
            gap: self.gap,
            allow_flip: self.allow_flip,
            backend: self.backend,
            content: self.content.clone(),
            semantics: self.semantics.clone(),
            mount_policy: self.mount_policy,
        }
    }
}

/// Formats registration metadata while redacting the content closure.
impl<A> fmt::Debug for PopupRequest<A> {
    /// Writes all request fields through their debug representations.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PopupRequest")
            .field("id", &self.id)
            .field("owner", &self.owner)
            .field("parent", &self.parent)
            .field("anchor", &self.anchor)
            .field("desired_size", &self.desired_size)
            .field("placement", &self.placement)
            .field("alignment", &self.alignment)
            .field("gap", &self.gap)
            .field("allow_flip", &self.allow_flip)
            .field("backend", &self.backend)
            .field("content", &self.content)
            .field("semantics", &self.semantics)
            .field("mount_policy", &self.mount_policy)
            .finish()
    }
}

/// Reason recorded when the portal asks a backend to hide a popup.
///
/// This value is diagnostic/behavioral metadata carried by a dismissal intent;
/// it does not itself close a popup. The enum is non-exhaustive.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupDismissReason;
/// assert_ne!(PopupDismissReason::Escape, PopupDismissReason::OutsidePress);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupDismissReason {
    /// Explicit application/widget request.
    Programmatic,
    /// Pointer press occurred outside the active popup subtree.
    OutsidePress,
    /// Escape dismissed the topmost eligible popup.
    Escape,
    /// Retained owner element no longer exists.
    OwnerRemoved,
    /// Native presentation generation is obsolete.
    PresentationStale,
    /// An ancestor popup closed.
    ParentClosed,
    /// Registration was explicitly removed.
    Unregistered,
}

/// Host-independent side effect emitted by [`PopupPortal`].
///
/// Intents are ordered: presentation precedes focus movement on open; subtree
/// dismissals precede optional owner focus restoration on close. The enum is
/// non-exhaustive.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::{PopupId, PopupIntent};
/// assert!(matches!(PopupIntent::Present { popup_id: PopupId::new(1) }, PopupIntent::Present { .. }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupIntent {
    /// Ask the selected backend to present or raise a popup.
    Present {
        /// Stable registration ID.
        popup_id: PopupId,
    },
    /// Ask input routing to focus the first retained popup descendant.
    MoveFocusInto {
        /// Popup whose mounted subtree should receive focus.
        popup_id: PopupId,
        /// Whether subsequent focus cycling should remain inside the subtree.
        trap: bool,
    },
    /// Ask a backend to hide a popup for the recorded reason.
    Dismiss {
        /// Stable registration ID.
        popup_id: PopupId,
        /// Lifecycle/input reason for dismissal.
        reason: PopupDismissReason,
    },
    /// Restore focus only if the host can still resolve this complete owner.
    RestoreFocus {
        /// Exact owner that should regain focus if it is still live/current.
        owner: PopupOwner,
    },
}

/// Result of routing one popup lifecycle or input operation.
///
/// `handled` answers whether the popup authority consumed/recognized the
/// operation; it is independent from whether any intents were emitted. An
/// already-open raise, for example, may be handled with no new intent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupPortalOutcome;
/// let outcome = PopupPortalOutcome::default();
/// assert!(!outcome.handled() && outcome.intents().is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PopupPortalOutcome {
    /// Whether the authority recognized/consumed the operation.
    handled: bool,
    /// Ordered provider/input side effects.
    intents: Vec<PopupIntent>,
}

/// Observation and ownership conversion for popup outcomes.
impl PopupPortalOutcome {
    /// Reports whether the popup authority handled the operation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupId, PopupPortal};
    /// assert!(!PopupPortal::<()>::new().close(PopupId::new(1)).handled());
    /// ```
    pub const fn handled(&self) -> bool {
        self.handled
    }

    /// Borrows ordered intents without consuming the outcome.
    ///
    /// An empty slice does not imply `handled == false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupPortalOutcome;
    /// assert_eq!(PopupPortalOutcome::default().intents(), &[]);
    /// ```
    pub fn intents(&self) -> &[PopupIntent] {
        &self.intents
    }

    /// Consumes the outcome and returns its ordered intent vector.
    ///
    /// The `handled` flag is discarded.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupPortalOutcome;
    /// assert!(PopupPortalOutcome::default().into_intents().is_empty());
    /// ```
    pub fn into_intents(self) -> Vec<PopupIntent> {
        self.intents
    }

    /// Creates a handled outcome with no side effects.
    fn handled_empty() -> Self {
        Self {
            handled: true,
            intents: Vec::new(),
        }
    }

    /// ORs `handled` and appends another outcome's intents in order.
    fn append(&mut self, mut other: Self) {
        self.handled |= other.handled;
        self.intents.append(&mut other.intents);
    }
}

/// Invalid popup registry operation.
///
/// Runtime-handle helpers return these errors and also queue a cloned copy for
/// non-fatal host diagnostics. The enum is non-exhaustive.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupPortalError;
/// assert_eq!(PopupPortalError::UnknownPopup.to_string(), "popup identifier is not registered");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PopupPortalError {
    /// Requested registration ID already exists.
    #[error("popup identifier is already registered")]
    DuplicateId,
    /// Operation addressed an unregistered popup ID.
    #[error("popup identifier is not registered")]
    UnknownPopup,
    /// A requested parent ID is not registered.
    #[error("popup parent is not registered")]
    UnknownParent,
    /// Parent/child owners use different logical window or generation.
    #[error("popup parent belongs to another presentation")]
    ParentPresentationMismatch,
    /// A child was opened while its registered parent was closed.
    #[error("popup parent must be open before its child")]
    ParentNotOpen,
    /// Checked one-based `u64` ID allocation cannot advance.
    #[error("popup identifier space is exhausted")]
    IdExhausted,
    /// Bounds/anchor contain non-finite edges or negative dimensions.
    #[error("popup geometry must be finite and non-negative")]
    InvalidBounds,
}

/// Internal mutable record for one registered popup.
struct PopupEntry<A> {
    /// Semantic registration contract.
    request: PopupRequest<A>,
    /// Backend-resolved global bounds, or `None` before positioning.
    bounds: Option<Rect>,
    /// Viewport paired with host-resolved bounds, or `None` for explicit bounds.
    resolved_viewport: Option<Rect>,
    /// Current semantic visibility.
    open: bool,
}

/// UI-local popup registry and z-order authority.
///
/// The portal is deliberately `Rc`-backed through [`PopupContent`], matching
/// the runtime's retained UI ownership. It must be driven on the UI thread.
/// Registration storage is unbounded. Open z-order is bottom-to-top with a
/// fixed procedural-fallback stratum below retained overlays; opening raises
/// only within the popup's stratum.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::popup::PopupPortal;
/// let portal = PopupPortal::<()>::new();
/// assert!(portal.topmost().is_none());
/// ```
pub struct PopupPortal<A> {
    /// Next checked one-based automatically allocated ID.
    next_id: u64,
    /// All closed/open registrations by stable ID.
    entries: HashMap<PopupId, PopupEntry<A>>,
    /// Open IDs in effective bottom-to-top mount-stratum order.
    z_order: Vec<PopupId>,
}

/// Creates an empty portal whose first allocated ID is one.
impl<A> Default for PopupPortal<A> {
    /// Returns empty registration and z-order storage.
    fn default() -> Self {
        Self {
            next_id: 1,
            entries: HashMap::new(),
            z_order: Vec::new(),
        }
    }
}

/// Registration, geometry, lifecycle, z-order, and input authority operations.
impl<A> PopupPortal<A> {
    /// Creates an empty portal equivalent to [`Default::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupPortal;
    /// assert_eq!(PopupPortal::<()>::new().open_ids().count(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a portal-local popup ID suitable for registration.
    ///
    /// IDs start at one, skip currently registered collisions, and are not
    /// reused merely because an entry is unregistered. Allocation reserves no
    /// entry, so callers must still register the returned ID.
    ///
    /// # Errors
    ///
    /// Returns `IdExhausted` when checked increment cannot advance beyond
    /// `u64::MAX`; that terminal candidate is not returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupId, PopupPortal};
    /// let mut portal = PopupPortal::<()>::new();
    /// assert_eq!(portal.allocate_id()?, PopupId::new(1));
    /// assert_eq!(portal.allocate_id()?, PopupId::new(2));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
    pub fn allocate_id(&mut self) -> Result<PopupId, PopupPortalError> {
        while self.entries.contains_key(&PopupId::new(self.next_id)) {
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or(PopupPortalError::IdExhausted)?;
        }
        let id = PopupId::new(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(PopupPortalError::IdExhausted)?;
        Ok(id)
    }

    /// Registers a closed popup without presenting or building it.
    ///
    /// Explicit IDs advance subsequent automatic allocation. A parent must
    /// already be registered and share its logical window/presentation
    /// generation; tree namespace may differ. Geometry is not validated here.
    ///
    /// # Errors
    ///
    /// Returns `DuplicateId`, `UnknownParent`, `ParentPresentationMismatch`, or
    /// `IdExhausted`. Errors leave registration storage unchanged, though a
    /// terminal explicit `u64::MAX` cannot advance the allocator.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(4);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// assert!(portal.contains(id) && !portal.is_open(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn register(&mut self, request: PopupRequest<A>) -> Result<(), PopupPortalError> {
        let id = request.id();
        if self.entries.contains_key(&id) {
            return Err(PopupPortalError::DuplicateId);
        }

        if let Some(parent_id) = request.parent() {
            let parent = self
                .entries
                .get(&parent_id)
                .ok_or(PopupPortalError::UnknownParent)?;
            if !same_presentation(parent.request.owner(), request.owner()) {
                return Err(PopupPortalError::ParentPresentationMismatch);
            }
        }

        if id.get() >= self.next_id {
            self.next_id = id
                .get()
                .checked_add(1)
                .ok_or(PopupPortalError::IdExhausted)?;
        }
        self.entries.insert(
            id,
            PopupEntry {
                request,
                bounds: None,
                resolved_viewport: None,
                open: false,
            },
        );
        Ok(())
    }

    /// Reports whether an ID is registered, regardless of visibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupId, PopupPortal};
    /// assert!(!PopupPortal::<()>::new().contains(PopupId::new(1)));
    /// ```
    pub fn contains(&self, popup_id: PopupId) -> bool {
        self.entries.contains_key(&popup_id)
    }

    /// Reports whether a registered popup is semantically open.
    ///
    /// Unknown and closed IDs both return `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupId, PopupPortal};
    /// assert!(!PopupPortal::<()>::new().is_open(PopupId::new(1)));
    /// ```
    pub fn is_open(&self, popup_id: PopupId) -> bool {
        self.entries.get(&popup_id).is_some_and(|entry| entry.open)
    }

    /// Borrows a registered semantic request.
    ///
    /// `None` means unknown ID. The returned request may describe either a
    /// closed or open popup; resolved bounds live separately in the portal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(2)), PopupContent::new(View::empty)))?;
    /// assert_eq!(portal.request(id).unwrap().owner().element_id(), ElementId(2));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn request(&self, popup_id: PopupId) -> Option<&PopupRequest<A>> {
        self.entries.get(&popup_id).map(|entry| &entry.request)
    }

    /// Builds a fresh view from a registered popup's retained factory.
    ///
    /// Closed registrations can be built. `None` means unknown ID; a factory
    /// panic propagates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(|| View::empty().key("built"))))?;
    /// assert_eq!(portal.build_content(id).unwrap().key_ref(), Some("built"));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn build_content(&self, popup_id: PopupId) -> Option<View<A>> {
        self.request(popup_id)
            .map(|request| request.content.build())
    }

    /// Replaces the declarative content factory without changing visibility,
    /// z-order, ownership, or backend-resolved geometry.
    ///
    /// Component reconciliation uses this when a stable popup owner rebuilds
    /// with new options, bindings, callbacks, or disabled state.
    /// The factory is moved but not invoked.
    ///
    /// # Errors
    ///
    /// Returns `UnknownPopup` without retaining `content` when the ID is absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// portal.set_content(id, PopupContent::new(|| View::empty().key("new")))?;
    /// assert_eq!(portal.build_content(id).unwrap().key_ref(), Some("new"));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn set_content(
        &mut self,
        popup_id: PopupId,
        content: PopupContent<A>,
    ) -> Result<(), PopupPortalError> {
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.request.content = content;
        Ok(())
    }

    /// Returns backend-resolved global logical-pixel bounds.
    ///
    /// `None` covers unknown IDs and registered popups with no current bounds;
    /// closing or changed semantic placement clears bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupId, PopupPortal};
    /// assert_eq!(PopupPortal::<()>::new().bounds(PopupId::new(1)), None);
    /// ```
    pub fn bounds(&self, popup_id: PopupId) -> Option<Rect> {
        self.entries.get(&popup_id).and_then(|entry| entry.bounds)
    }

    /// Returns the host viewport that produced the current resolved bounds.
    ///
    /// Explicit backend bounds installed through [`Self::set_bounds`] do not
    /// imply a viewport and therefore return `None` here.
    /// Unknown IDs and cleared/closed geometry also return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::{PopupId, PopupPortal};
    /// assert_eq!(PopupPortal::<()>::new().resolved_viewport(PopupId::new(1)), None);
    /// ```
    pub fn resolved_viewport(&self, popup_id: PopupId) -> Option<Rect> {
        self.entries
            .get(&popup_id)
            .and_then(|entry| entry.resolved_viewport)
    }

    /// Updates the retained anchor used by the selected presentation backend.
    ///
    /// This does not change popup bounds: the anchor belongs to the semantic
    /// request while [`Self::set_bounds`] records the rectangle produced by the
    /// active backend.
    /// `None` explicitly clears the semantic anchor. Validation occurs before
    /// ID lookup, so an invalid anchor returns `InvalidBounds` even for an
    /// unknown ID.
    ///
    /// # Errors
    ///
    /// Returns `InvalidBounds` for non-finite edges/negative dimensions, or
    /// `UnknownPopup` after successful validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Rect}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// let anchor = Rect::new(1.0, 2.0, 3.0, 4.0); portal.set_anchor(id, Some(anchor))?;
    /// assert_eq!(portal.request(id).unwrap().anchor(), Some(anchor));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn set_anchor(
        &mut self,
        popup_id: PopupId,
        anchor: Option<Rect>,
    ) -> Result<(), PopupPortalError> {
        if anchor.is_some_and(|anchor| !rect_is_valid(anchor)) {
            return Err(PopupPortalError::InvalidBounds);
        }
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.request.anchor = anchor;
        Ok(())
    }

    /// Publishes provider-neutral placement inputs for a registered popup.
    ///
    /// The update is atomic: every geometry value is validated before the
    /// retained request is changed. Previously resolved backend bounds are
    /// cleared only when a placement field changes, so an idempotent repaint
    /// cannot discard geometry that the host already resolved.
    /// Backend and mount policy are not part of [`PopupPlacementSpec`] and stay
    /// unchanged.
    ///
    /// # Errors
    ///
    /// All geometry validation failures map to `InvalidBounds`; a valid update
    /// for an absent ID returns `UnknownPopup`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Rect, Size}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// let spec = PopupPlacementSpec::new(Rect::new(1.0, 2.0, 3.0, 4.0), Size::new(20.0, 10.0)).with_gap(2.0);
    /// portal.set_placement_request(id, spec)?; assert_eq!(portal.request(id).unwrap().gap(), 2.0);
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn set_placement_request(
        &mut self,
        popup_id: PopupId,
        placement: PopupPlacementSpec,
    ) -> Result<(), PopupPortalError> {
        validate_anchor(placement.anchor).map_err(|_| PopupPortalError::InvalidBounds)?;
        validate_desired_size(placement.desired_size)
            .map_err(|_| PopupPortalError::InvalidBounds)?;
        validate_gap(placement.gap).map_err(|_| PopupPortalError::InvalidBounds)?;

        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        let geometry_changed = entry.request.anchor != Some(placement.anchor)
            || entry.request.desired_size != Some(placement.desired_size)
            || entry.request.placement != placement.placement
            || entry.request.alignment != placement.alignment
            || entry.request.gap != placement.gap
            || entry.request.allow_flip != placement.allow_flip;
        entry.request.anchor = Some(placement.anchor);
        entry.request.desired_size = Some(placement.desired_size);
        entry.request.placement = placement.placement;
        entry.request.alignment = placement.alignment;
        entry.request.gap = placement.gap;
        entry.request.allow_flip = placement.allow_flip;
        if geometry_changed {
            entry.bounds = None;
            entry.resolved_viewport = None;
        }
        Ok(())
    }

    /// Records bounds produced by the chosen popup backend for hit-testing.
    ///
    /// Bounds are global logical pixels and must have finite edges/non-negative
    /// dimensions. Any previously associated resolved viewport is cleared.
    /// Validation precedes ID lookup.
    ///
    /// # Errors
    ///
    /// Returns `InvalidBounds` or `UnknownPopup`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Rect}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// let bounds = Rect::new(5.0, 6.0, 20.0, 10.0); portal.set_bounds(id, bounds)?;
    /// assert_eq!(portal.bounds(id), Some(bounds));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn set_bounds(&mut self, popup_id: PopupId, bounds: Rect) -> Result<(), PopupPortalError> {
        if !rect_is_valid(bounds) {
            return Err(PopupPortalError::InvalidBounds);
        }
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.bounds = Some(bounds);
        entry.resolved_viewport = None;
        Ok(())
    }

    /// Records bounds resolved by a host together with its complete viewport.
    ///
    /// Both rectangles are validated before either value is committed, so a
    /// rejected update cannot separate bounds from the viewport that produced
    /// them.
    /// Geometry is global logical pixels. Viewport must have strictly positive
    /// dimensions, but bounds may be zero-sized and need not lie inside it.
    ///
    /// # Errors
    ///
    /// Any invalid/empty viewport or invalid bounds maps to `InvalidBounds`;
    /// valid geometry for an absent ID returns `UnknownPopup`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Rect}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// let viewport = Rect::new(0.0, 0.0, 100.0, 80.0); let bounds = Rect::new(5.0, 6.0, 20.0, 10.0);
    /// portal.set_resolved_bounds(id, viewport, bounds)?; assert_eq!(portal.resolved_viewport(id), Some(viewport));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn set_resolved_bounds(
        &mut self,
        popup_id: PopupId,
        viewport: Rect,
        bounds: Rect,
    ) -> Result<(), PopupPortalError> {
        validate_viewport(viewport).map_err(|_| PopupPortalError::InvalidBounds)?;
        if !rect_is_valid(bounds) {
            return Err(PopupPortalError::InvalidBounds);
        }
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.bounds = Some(bounds);
        entry.resolved_viewport = Some(viewport);
        Ok(())
    }

    /// Clears both backend bounds and their optional resolved viewport.
    ///
    /// Visibility, semantic anchor/request, and z-order remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns `UnknownPopup` for an absent ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Rect}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// portal.set_bounds(id, Rect::new(0.0, 0.0, 1.0, 1.0))?; portal.clear_bounds(id)?;
    /// assert_eq!(portal.bounds(id), None);
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn clear_bounds(&mut self, popup_id: PopupId) -> Result<(), PopupPortalError> {
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.bounds = None;
        entry.resolved_viewport = None;
        Ok(())
    }

    /// Opens or raises a popup and emits presentation/focus intents.
    ///
    /// A closed popup emits `Present`, followed by `MoveFocusInto` when
    /// requested. Reopening an open popup raises it within its mount stratum but
    /// emits no duplicate intents. Geometry is optional. A child requires its
    /// parent to be open.
    ///
    /// # Errors
    ///
    /// Returns `UnknownPopup` or `ParentNotOpen` without changing visibility.
    ///
    /// # Panics
    ///
    /// Panics only if the internal popup map loses the entry between the initial
    /// existence check and its mutation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// let outcome = portal.open(id)?; assert!(outcome.handled());
    /// assert!(matches!(outcome.intents(), [PopupIntent::Present { popup_id }] if *popup_id == id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn open(&mut self, popup_id: PopupId) -> Result<PopupPortalOutcome, PopupPortalError> {
        let parent = self
            .entries
            .get(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?
            .request
            .parent();
        if parent.is_some_and(|id| !self.is_open(id)) {
            return Err(PopupPortalError::ParentNotOpen);
        }

        let entry = self
            .entries
            .get_mut(&popup_id)
            .expect("popup existence checked above");
        let was_open = entry.open;
        entry.open = true;
        let focus_policy = entry.request.semantics.focus_policy();
        let mount_policy = entry.request.mount_policy();

        self.raise_in_effective_z_order(popup_id, mount_policy);

        let mut outcome = PopupPortalOutcome::handled_empty();
        if !was_open {
            outcome.intents.push(PopupIntent::Present { popup_id });
            match focus_policy {
                PopupFocusPolicy::None => {}
                PopupFocusPolicy::MoveIntoPopup => {
                    outcome.intents.push(PopupIntent::MoveFocusInto {
                        popup_id,
                        trap: false,
                    });
                }
                PopupFocusPolicy::TrapWithinPopup => {
                    outcome.intents.push(PopupIntent::MoveFocusInto {
                        popup_id,
                        trap: true,
                    });
                }
            }
        }
        Ok(outcome)
    }

    /// Programmatically closes an open popup subtree.
    ///
    /// This is shorthand for `close_with_reason(id, Programmatic)`. Unknown IDs
    /// return an unhandled empty outcome; a registered but already-closed ID is
    /// handled with no intents.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?; portal.open(id)?;
    /// assert!(portal.close(id).handled()); assert!(!portal.is_open(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn close(&mut self, popup_id: PopupId) -> PopupPortalOutcome {
        self.close_with_reason(popup_id, PopupDismissReason::Programmatic)
    }

    /// Closes a popup and every registered descendant with explicit root reason.
    ///
    /// Open descendants dismiss topmost-first with `ParentClosed`; the root uses
    /// `reason`. Bounds are cleared, registrations remain, and optional owner
    /// focus restoration is emitted last. Unknown IDs are unhandled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?; portal.open(id)?;
    /// let outcome = portal.close_with_reason(id, PopupDismissReason::Escape);
    /// assert!(matches!(outcome.intents()[0], PopupIntent::Dismiss { reason: PopupDismissReason::Escape, .. }));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn close_with_reason(
        &mut self,
        popup_id: PopupId,
        reason: PopupDismissReason,
    ) -> PopupPortalOutcome {
        self.close_tree(popup_id, reason, true)
    }

    /// Removes a registration and all registered descendants.
    ///
    /// Open entries first emit `Unregistered`/`ParentClosed` dismissals without
    /// focus restoration, then all subtree entries/z-order records are removed.
    /// A registered closed entry still yields `handled == true`; unknown IDs do
    /// not.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// assert!(portal.unregister(id).handled()); assert!(!portal.contains(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn unregister(&mut self, popup_id: PopupId) -> PopupPortalOutcome {
        let existed = self.entries.contains_key(&popup_id);
        let mut outcome = self.close_tree(popup_id, PopupDismissReason::Unregistered, false);
        let descendants = self.descendants_including(popup_id);
        for id in descendants {
            self.entries.remove(&id);
            self.z_order.retain(|candidate| *candidate != id);
        }
        outcome.handled |= existed;
        outcome
    }

    /// Iterates open popups from bottom to top across the fixed mount strata.
    ///
    /// The iterator borrows the portal, is double-ended, and yields IDs by
    /// value. Procedural fallback entries always precede retained overlays.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupPortal;
    /// let portal = PopupPortal::<()> ::new();
    /// assert_eq!(portal.open_ids().next(), None);
    /// ```
    pub fn open_ids(&self) -> impl DoubleEndedIterator<Item = PopupId> + '_ {
        self.z_order.iter().copied()
    }

    /// Returns the effective topmost open popup across mount strata.
    ///
    /// `None` means no popup is open.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupPortal;
    /// assert_eq!(PopupPortal::<()>::new().topmost(), None);
    /// ```
    pub fn topmost(&self) -> Option<PopupId> {
        self.z_order.last().copied()
    }

    /// Returns the topmost open popup owned by an exact retained element in
    /// one presentation.
    ///
    /// Procedural overlay backends use this to translate their authoritative
    /// tree hit-test into a portal popup id before first paint has committed
    /// global popup bounds.
    /// Exact matching includes logical window, presentation generation,
    /// retained-tree namespace, and element ID. Geometry is irrelevant.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1); let window = LogicalWindowId::new("w");
    /// portal.register(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(3), ElementId(4)), PopupContent::new(View::empty)))?; portal.open(id)?;
    /// assert_eq!(portal.topmost_for_owner(&window, PresentationGeneration::INITIAL, ElementTreeId::new(3), ElementId(4)), Some(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn topmost_for_owner(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
    ) -> Option<PopupId> {
        self.z_order.iter().rev().copied().find(|popup_id| {
            self.entries.get(popup_id).is_some_and(|entry| {
                let owner = entry.request.owner();
                entry.open
                    && owner.belongs_to(logical_window_id, presentation_generation)
                    && owner.element_tree_id() == element_tree_id
                    && owner.element_id() == element_id
            })
        })
    }

    /// Returns the topmost bounded popup containing `point` for one presentation.
    ///
    /// Point/bounds use global logical pixels and [`Rect::contains`] edge
    /// semantics. Open unpositioned popups cannot hit. Exact logical-window and
    /// generation matching prevents stale surfaces from participating.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId, Point, Rect}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1); let window = LogicalWindowId::new("w");
    /// portal.register(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?; portal.set_bounds(id, Rect::new(0.0, 0.0, 10.0, 10.0))?; portal.open(id)?;
    /// assert_eq!(portal.hit_test(&window, PresentationGeneration::INITIAL, Point::new(5.0, 5.0)), Some(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn hit_test(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
    ) -> Option<PopupId> {
        self.z_order.iter().rev().copied().find(|popup_id| {
            let Some(entry) = self.entries.get(popup_id) else {
                return false;
            };
            entry.open
                && entry
                    .request
                    .owner()
                    .belongs_to(logical_window_id, presentation_generation)
                && entry
                    .bounds
                    .is_some_and(|bounds| bounds.contains(point.x, point.y))
        })
    }

    /// Resolves a backend-confirmed candidate and a committed-bounds hit using
    /// the same effective z-order as every other portal operation.
    /// Invalid/closed/stale backend candidates are ignored; when both evidence
    /// sources hit different entries, effective topmost order wins.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::popup::PopupPortal;
    /// // Public pointer routing invokes this arbitration before dismissal.
    /// assert!(PopupPortal::<()>::new().topmost().is_none());
    /// ```
    pub(crate) fn resolve_pointer_hit(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
        backend_hit: Option<PopupId>,
    ) -> Option<PopupId> {
        let backend_hit = backend_hit.filter(|popup_id| {
            self.entries.get(popup_id).is_some_and(|entry| {
                entry.open
                    && entry
                        .request
                        .owner()
                        .belongs_to(logical_window_id, presentation_generation)
            })
        });
        let bounds_hit = self.hit_test(logical_window_id, presentation_generation, point);
        self.z_order
            .iter()
            .rev()
            .copied()
            .find(|candidate| Some(*candidate) == backend_hit || Some(*candidate) == bounds_hit)
    }

    /// Routes a pointer press through popup z-order.
    ///
    /// Popups above the hit popup see an outside press and may close. A hit on
    /// an interactive popup is consumed so underlying content is not activated.
    /// Routing is restricted to the exact logical window/generation and uses
    /// global logical-pixel geometry. `handled` can be true with no dismissal
    /// when an inside popup consumes pointer input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId, Point, Rect}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1); let window = LogicalWindowId::new("w");
    /// portal.register(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?; portal.set_bounds(id, Rect::new(0.0, 0.0, 10.0, 10.0))?; portal.open(id)?;
    /// assert!(portal.handle_pointer_press(&window, PresentationGeneration::INITIAL, Point::new(20.0, 20.0)).handled());
    /// assert!(!portal.is_open(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn handle_pointer_press(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
    ) -> PopupPortalOutcome {
        self.handle_pointer_press_with_backend_hit(
            logical_window_id,
            presentation_generation,
            point,
            None,
        )
    }

    /// Routes a pointer press using an optional hit confirmed by the selected
    /// presentation backend.
    ///
    /// The explicit hit is useful before a procedural overlay's first paint,
    /// when retained overlay hit regions exist but global portal bounds have
    /// not yet been committed. It is accepted only when the popup is open and
    /// belongs to the routed presentation, then arbitrated with any bounds hit
    /// according to the portal's effective z-order.
    /// `point` remains relevant for competing committed-bounds hits. A backend
    /// hit does not require portal bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId, Point}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1); let window = LogicalWindowId::new("w");
    /// portal.register(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?; portal.open(id)?;
    /// let outcome = portal.handle_pointer_press_with_backend_hit(&window, PresentationGeneration::INITIAL, Point::new(500.0, 500.0), Some(id));
    /// assert!(outcome.handled() && portal.is_open(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn handle_pointer_press_with_backend_hit(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
        backend_hit: Option<PopupId>,
    ) -> PopupPortalOutcome {
        let hit = self.resolve_pointer_hit(
            logical_window_id,
            presentation_generation,
            point,
            backend_hit,
        );
        let snapshot: Vec<PopupId> = self.z_order.iter().rev().copied().collect();
        let mut outcome = PopupPortalOutcome::default();

        for popup_id in snapshot {
            if Some(popup_id) == hit {
                if self
                    .entries
                    .get(&popup_id)
                    .is_some_and(|entry| entry.request.semantics.consumes_pointer_input())
                {
                    outcome.handled = true;
                }
                break;
            }

            let Some(entry) = self.entries.get(&popup_id) else {
                continue;
            };
            if !entry.open {
                continue;
            }
            if !entry
                .request
                .owner()
                .belongs_to(logical_window_id, presentation_generation)
            {
                continue;
            }
            if !entry.request.semantics.dismisses_on_outside_press() {
                if entry.request.semantics.consumes_pointer_input() {
                    outcome.handled = true;
                }
                break;
            }

            let consumes = entry.request.semantics.consumes_pointer_input();
            let dismissed = self.close_tree(popup_id, PopupDismissReason::OutsidePress, true);
            outcome.append(dismissed);
            outcome.handled |= consumes;
        }
        outcome
    }

    /// Dismisses the topmost popup in a presentation when it allows Escape.
    ///
    /// A topmost popup that disables Escape blocks lower popups and returns an
    /// unhandled outcome. Successful close uses reason `Escape` and may restore
    /// owner focus.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1); let window = LogicalWindowId::new("w");
    /// portal.register(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?; portal.open(id)?;
    /// assert!(portal.handle_escape(&window, PresentationGeneration::INITIAL).handled());
    /// assert!(!portal.is_open(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn handle_escape(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> PopupPortalOutcome {
        let candidate = self.z_order.iter().rev().copied().find(|popup_id| {
            self.entries.get(popup_id).is_some_and(|entry| {
                entry
                    .request
                    .owner()
                    .belongs_to(logical_window_id, presentation_generation)
            })
        });
        let Some(popup_id) = candidate else {
            return PopupPortalOutcome::default();
        };
        let dismisses = self
            .entries
            .get(&popup_id)
            .is_some_and(|entry| entry.request.semantics.dismisses_on_escape());
        if !dismisses {
            return PopupPortalOutcome::default();
        }
        self.close_tree(popup_id, PopupDismissReason::Escape, true)
    }

    /// Removes every registration whose owner callback reports missing.
    ///
    /// The callback sees all registrations in unspecified hash-map order. Stale
    /// roots are closed/removed with descendants; duplicate descendant matches
    /// are collapsed. Open roots can emit dismissal intents; focus is not
    /// restored during stale-owner pruning.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(9)), PopupContent::new(View::empty)))?;
    /// assert!(portal.prune_stale_owners(|owner| owner.element_id() != ElementId(9)).handled());
    /// assert!(!portal.contains(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn prune_stale_owners(
        &mut self,
        mut owner_is_alive: impl FnMut(&PopupOwner) -> bool,
    ) -> PopupPortalOutcome {
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| (!owner_is_alive(entry.request.owner())).then_some(*id))
            .collect();
        self.remove_stale_roots(stale, PopupDismissReason::OwnerRemoved)
    }

    /// Removes registrations owned by one retained tree whose element no
    /// longer exists, without inspecting registrations from sibling trees.
    ///
    /// A [`crate::app::RuntimeHandle`] can be shared by multiple windows. A
    /// tree-local reconcile must therefore never decide that owners belonging
    /// to another tree are stale.
    /// Callback order is unspecified. Removal semantics match
    /// [`Self::prune_stale_owners`] and do not restore focus.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1);
    /// portal.register(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, ElementTreeId::new(3), ElementId(9)), PopupContent::new(View::empty)))?;
    /// portal.prune_stale_owners_in_tree(ElementTreeId::new(3), |_| false);
    /// assert!(!portal.contains(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn prune_stale_owners_in_tree(
        &mut self,
        element_tree_id: ElementTreeId,
        mut element_is_alive: impl FnMut(ElementId) -> bool,
    ) -> PopupPortalOutcome {
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                let owner = entry.request.owner();
                (owner.element_tree_id() == element_tree_id
                    && !element_is_alive(owner.element_id()))
                .then_some(*id)
            })
            .collect();
        self.remove_stale_roots(stale, PopupDismissReason::OwnerRemoved)
    }

    /// Removes every registration owned by a released retained-tree
    /// namespace, including registered descendants.
    ///
    /// No backend intent is returned because the presentation tree itself is
    /// being destroyed. The caller uses the returned identities to discard
    /// queued effects that can no longer be applied.
    /// IDs are returned in ascending numeric order. Registrations belonging to
    /// other tree namespaces remain intact.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId; use ailloli_ui_runtime::{app::{PresentationGeneration, Runtime, RuntimeHandle}, component::View, popup::*};
    /// let shared = RuntimeHandle::<()>::new(); let runtime = Runtime::new(shared.clone()); let id = PopupId::new(1);
    /// runtime.runtime.register_popup(PopupRequest::new(id, PopupOwner::new("w", PresentationGeneration::INITIAL, runtime.runtime.element_tree_id(), ElementId(1)), PopupContent::new(View::empty)))?;
    /// drop(runtime); assert!(!shared.popup_portal().borrow().contains(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub(crate) fn release_element_tree(&mut self, element_tree_id: ElementTreeId) -> Vec<PopupId> {
        let before: HashSet<PopupId> = self.entries.keys().copied().collect();
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                (entry.request.owner().element_tree_id() == element_tree_id).then_some(*id)
            })
            .collect();
        let _ = self.remove_stale_roots(stale, PopupDismissReason::OwnerRemoved);

        let mut removed: Vec<PopupId> = before
            .into_iter()
            .filter(|popup_id| !self.entries.contains_key(popup_id))
            .collect();
        removed.sort_by_key(|popup_id| popup_id.get());
        removed
    }

    /// Removes popups attached to obsolete generations of a logical window.
    ///
    /// Registrations for other logical windows and for `current_generation`
    /// remain. Stale roots and descendants are closed/removed with
    /// `PresentationStale`/`ParentClosed`; focus restoration is suppressed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId}; use ailloli_ui_runtime::{app::PresentationGeneration, component::View, popup::*};
    /// let mut portal = PopupPortal::<()>::new(); let id = PopupId::new(1); let window = LogicalWindowId::new("w");
    /// portal.register(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::new(1), ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?; portal.open(id)?;
    /// assert!(portal.close_stale_presentations(&window, PresentationGeneration::new(2)).handled());
    /// assert!(!portal.contains(id));
    /// # Ok::<(), PopupPortalError>(())
    /// ```
    pub fn close_stale_presentations(
        &mut self,
        logical_window_id: &LogicalWindowId,
        current_generation: PresentationGeneration,
    ) -> PopupPortalOutcome {
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                let owner = entry.request.owner();
                (owner.logical_window_id() == logical_window_id
                    && owner.presentation_generation() != current_generation)
                    .then_some(*id)
            })
            .collect();
        self.remove_stale_roots(stale, PopupDismissReason::PresentationStale)
    }

    /// Collapses stale descendants to roots, closes/removes them deterministically.
    fn remove_stale_roots(
        &mut self,
        stale: Vec<PopupId>,
        reason: PopupDismissReason,
    ) -> PopupPortalOutcome {
        let stale_set: HashSet<PopupId> = stale.iter().copied().collect();
        let mut roots: Vec<PopupId> = stale
            .into_iter()
            .filter(|id| {
                self.entries
                    .get(id)
                    .and_then(|entry| entry.request.parent())
                    .is_none_or(|parent| !stale_set.contains(&parent))
            })
            .collect();
        roots.sort_by_key(|id| {
            self.z_order
                .iter()
                .position(|candidate| candidate == id)
                .map(|position| (0_u8, position as u64))
                .unwrap_or((1_u8, id.get()))
        });
        let mut outcome = PopupPortalOutcome::default();
        for root in roots {
            outcome.append(self.close_tree(root, reason, false));
            for id in self.descendants_including(root) {
                self.entries.remove(&id);
                self.z_order.retain(|candidate| *candidate != id);
            }
        }
        outcome
    }

    /// Raises a popup within its fixed procedural/retained mount stratum.
    fn raise_in_effective_z_order(&mut self, popup_id: PopupId, mount_policy: PopupMountPolicy) {
        self.z_order.retain(|candidate| *candidate != popup_id);
        match mount_policy {
            PopupMountPolicy::ProceduralFallback => {
                let retained_start = self
                    .z_order
                    .iter()
                    .position(|candidate| {
                        self.entries.get(candidate).is_some_and(|entry| {
                            entry.request.mount_policy() == PopupMountPolicy::RetainedOverlay
                        })
                    })
                    .unwrap_or(self.z_order.len());
                self.z_order.insert(retained_start, popup_id);
            }
            PopupMountPolicy::RetainedOverlay => self.z_order.push(popup_id),
        }
    }

    /// Closes an open subtree topmost-first and optionally restores root focus.
    fn close_tree(
        &mut self,
        popup_id: PopupId,
        reason: PopupDismissReason,
        restore_focus: bool,
    ) -> PopupPortalOutcome {
        if !self.entries.contains_key(&popup_id) {
            return PopupPortalOutcome::default();
        }

        let mut ids = self.descendants_including(popup_id);
        ids.sort_by_key(|id| {
            self.z_order
                .iter()
                .position(|candidate| candidate == id)
                .unwrap_or(0)
        });
        ids.reverse();

        let mut outcome = PopupPortalOutcome::handled_empty();
        let root_owner = self
            .entries
            .get(&popup_id)
            .map(|entry| entry.request.owner().clone());
        let root_restores_focus = self
            .entries
            .get(&popup_id)
            .is_some_and(|entry| entry.request.semantics.restores_focus_on_close());

        for id in ids {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            if !entry.open {
                continue;
            }
            entry.open = false;
            entry.bounds = None;
            self.z_order.retain(|candidate| *candidate != id);
            outcome.intents.push(PopupIntent::Dismiss {
                popup_id: id,
                reason: if id == popup_id {
                    reason
                } else {
                    PopupDismissReason::ParentClosed
                },
            });
        }

        if restore_focus && root_restores_focus && !outcome.intents.is_empty() {
            if let Some(owner) = root_owner {
                outcome.intents.push(PopupIntent::RestoreFocus { owner });
            }
        }
        outcome
    }

    /// Collects a registration and all parent-linked descendants without cycles.
    fn descendants_including(&self, popup_id: PopupId) -> Vec<PopupId> {
        let mut result = Vec::new();
        let mut pending = vec![popup_id];
        while let Some(id) = pending.pop() {
            if result.contains(&id) {
                continue;
            }
            result.push(id);
            pending.extend(self.entries.iter().filter_map(|(candidate, entry)| {
                (entry.request.parent() == Some(id)).then_some(*candidate)
            }));
        }
        result
    }
}

/// Compares only logical-window identity and presentation generation.
fn same_presentation(left: &PopupOwner, right: &PopupOwner) -> bool {
    left.logical_window_id() == right.logical_window_id()
        && left.presentation_generation() == right.presentation_generation()
}

/// Accepts finite rectangles with finite edges and non-negative dimensions.
fn rect_is_valid(rect: Rect) -> bool {
    rect_has_finite_edges(rect) && rect.w >= 0.0 && rect.h >= 0.0
}

/// Checks stored components plus computed right/bottom edges for finiteness.
fn rect_has_finite_edges(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.w.is_finite()
        && rect.h.is_finite()
        && rect.right().is_finite()
        && rect.bottom().is_finite()
}

/// Maps invalid anchor geometry to [`PopupPlacementError::InvalidAnchor`].
///
/// # Errors
///
/// Returns [`PopupPlacementError::InvalidAnchor`] for a negative dimension or
/// any non-finite component or computed edge.
fn validate_anchor(anchor: Rect) -> Result<(), PopupPlacementError> {
    if rect_is_valid(anchor) {
        Ok(())
    } else {
        Err(PopupPlacementError::InvalidAnchor)
    }
}

/// Accepts finite non-negative desired width and height, including zero.
///
/// # Errors
///
/// Returns [`PopupPlacementError::InvalidDesiredSize`] for a negative or
/// non-finite width or height.
fn validate_desired_size(desired_size: Size) -> Result<(), PopupPlacementError> {
    if desired_size.w.is_finite()
        && desired_size.h.is_finite()
        && desired_size.w >= 0.0
        && desired_size.h >= 0.0
    {
        Ok(())
    } else {
        Err(PopupPlacementError::InvalidDesiredSize)
    }
}

/// Requires valid finite edges and strictly positive viewport dimensions.
///
/// # Errors
///
/// Returns [`PopupPlacementError::InvalidViewport`] for invalid geometry, or
/// [`PopupPlacementError::EmptyViewport`] when either dimension is zero.
fn validate_viewport(viewport: Rect) -> Result<(), PopupPlacementError> {
    if !rect_is_valid(viewport) {
        return Err(PopupPlacementError::InvalidViewport);
    }
    if viewport.w == 0.0 || viewport.h == 0.0 {
        return Err(PopupPlacementError::EmptyViewport);
    }
    Ok(())
}

/// Accepts a finite non-negative logical-pixel anchor gap.
///
/// # Errors
///
/// Returns [`PopupPlacementError::InvalidGap`] for a negative or non-finite gap.
fn validate_gap(gap: f32) -> Result<(), PopupPlacementError> {
    if gap.is_finite() && gap >= 0.0 {
        Ok(())
    } else {
        Err(PopupPlacementError::InvalidGap)
    }
}

/// Computes non-negative viewport space on one anchor side after the gap.
fn available_vertical_space(
    anchor: Rect,
    viewport: Rect,
    placement: PopupPlacement,
    gap: f32,
) -> f32 {
    match placement {
        PopupPlacement::Top => (anchor.y - gap - viewport.y).max(0.0),
        PopupPlacement::Bottom => (viewport.bottom() - anchor.bottom() - gap).max(0.0),
    }
}

/// Computes aligned bounds after callers validate finite input geometry.
///
/// Arithmetic can still overflow, which maps to `UnrepresentableGeometry`.
///
/// # Errors
///
/// Returns [`PopupPlacementError::UnrepresentableGeometry`] when aligned-edge
/// arithmetic produces a non-finite coordinate.
fn position_popup_unchecked(
    anchor: Rect,
    desired_size: Size,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    gap: f32,
) -> Result<Rect, PopupPlacementError> {
    let x = match alignment {
        PopupAlignment::Start => anchor.x,
        PopupAlignment::Center => anchor.x + (anchor.w - desired_size.w) * 0.5,
        PopupAlignment::End => anchor.right() - desired_size.w,
    };
    let y = match placement {
        PopupPlacement::Top => anchor.y - gap - desired_size.h,
        PopupPlacement::Bottom => anchor.bottom() + gap,
    };
    let bounds = Rect::new(x, y, desired_size.w, desired_size.h);
    if rect_has_finite_edges(bounds) {
        Ok(bounds)
    } else {
        Err(PopupPlacementError::UnrepresentableGeometry)
    }
}
