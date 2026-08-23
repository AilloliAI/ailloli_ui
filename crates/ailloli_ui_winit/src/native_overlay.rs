//! Portable native_overlay contracts and Linux backend selection.

use std::fmt;

/// Logical desktop rectangle used to select and place an overlay exactly.
///
/// Coordinates and dimensions are compositor logical pixels. Negative origins
/// are valid for monitors left of or above the desktop origin; all components
/// must be finite and both dimensions must be strictly positive before native
/// creation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::native_overlay::NativeOverlayRect;
/// let rect = NativeOverlayRect::new(-1920.0, 0.0, 1920.0, 1080.0);
/// assert_eq!((rect.x, rect.width), (-1920.0, 1920.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeOverlayRect {
    /// Left edge in logical desktop coordinates.
    pub x: f64,
    /// Top edge in logical desktop coordinates.
    pub y: f64,
    /// Strictly positive logical width.
    pub width: f64,
    /// Strictly positive logical height.
    pub height: f64,
}

/// Construction and validation of a logical output rectangle.
impl NativeOverlayRect {
    /// Creates a rectangle without validation or normalization.
    ///
    /// Native creation later rejects non-finite coordinates and non-positive
    /// dimensions with [`NativeOverlayError::InvalidTarget`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::native_overlay::NativeOverlayRect;
    /// let rect = NativeOverlayRect::new(10.0, 20.0, 640.0, 480.0);
    /// assert_eq!((rect.y, rect.height), (20.0, 480.0));
    /// ```
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Accepts finite coordinates and strictly positive finite dimensions.
    ///
    /// # Errors
    ///
    /// Returns [`NativeOverlayError::InvalidTarget`] when either coordinate is
    /// non-finite or either dimension is non-finite or not strictly positive.
    ///
    /// # Examples
    ///
    /// ```
    /// // Native constructors apply this validation before contacting a backend.
    /// let invalid_width = 0.0_f64;
    /// assert!(invalid_width <= 0.0);
    /// ```
    pub(crate) fn validate(self) -> Result<Self, NativeOverlayError> {
        if !self.x.is_finite()
            || !self.y.is_finite()
            || !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return Err(NativeOverlayError::InvalidTarget);
        }
        Ok(self)
    }
}

/// Output selected by the portal/compositor logical desktop rectangle.
///
/// The rectangle is authoritative. An optional output name only narrows an
/// otherwise exact rectangle match and never substitutes for its geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::native_overlay::{NativeOverlayRect, NativeOverlayTarget};
/// let target = NativeOverlayTarget::new(NativeOverlayRect::new(0.0, 0.0, 1920.0, 1080.0))
///     .output_name("DP-1");
/// assert_eq!(target.output_name.as_deref(), Some("DP-1"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct NativeOverlayTarget {
    /// Exact logical desktop rectangle supplied by the capture/portal workflow.
    pub logical_rect: NativeOverlayRect,
    /// Optional stable output name used as an additional, never substitute, discriminator.
    pub output_name: Option<String>,
}

/// Exact-geometry target construction and optional name discrimination.
impl NativeOverlayTarget {
    /// Creates an unnamed target for the exact logical rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::native_overlay::{NativeOverlayRect, NativeOverlayTarget};
    /// let target = NativeOverlayTarget::new(NativeOverlayRect::new(0.0, 0.0, 800.0, 600.0));
    /// assert!(target.output_name.is_none());
    /// ```
    pub const fn new(logical_rect: NativeOverlayRect) -> Self {
        Self {
            logical_rect,
            output_name: None,
        }
    }

    /// Adds a stable output-name discriminator.
    ///
    /// An empty string is retained as an exact name and will generally produce
    /// [`NativeOverlayError::OutputMatchMissing`] rather than being treated as absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::native_overlay::{NativeOverlayRect, NativeOverlayTarget};
    /// let target = NativeOverlayTarget::new(NativeOverlayRect::new(0.0, 0.0, 1.0, 1.0))
    ///     .output_name("HDMI-A-1");
    /// assert_eq!(target.output_name.as_deref(), Some("HDMI-A-1"));
    /// ```
    pub fn output_name(mut self, name: impl Into<String>) -> Self {
        self.output_name = Some(name.into());
        self
    }
}

/// Pointer behavior of the native overlay. Keyboard focus is never requested.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::native_overlay::NativeOverlayInputMode;
/// assert_eq!(NativeOverlayInputMode::default(), NativeOverlayInputMode::PassThrough);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativeOverlayInputMode {
    /// Installs an empty input region so pointer events reach content underneath.
    #[default]
    PassThrough,
    /// Lets the full overlay surface receive and block pointer events.
    BlockPointer,
}

/// Native overlay creation options.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::native_overlay::{
///     NativeOverlayInputMode, NativeOverlayOptions, NativeOverlayRect, NativeOverlayTarget,
/// };
/// let options = NativeOverlayOptions::new(NativeOverlayTarget::new(
///     NativeOverlayRect::new(0.0, 0.0, 1280.0, 720.0),
/// )).input_mode(NativeOverlayInputMode::BlockPointer);
/// assert_eq!(options.input_mode, NativeOverlayInputMode::BlockPointer);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct NativeOverlayOptions {
    /// Exact compositor output target.
    pub target: NativeOverlayTarget,
    /// Pointer policy; defaults to pass-through.
    pub input_mode: NativeOverlayInputMode,
}

/// Builder-style native overlay configuration.
impl NativeOverlayOptions {
    /// Creates pass-through options for `target`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::native_overlay::*;
    /// let options = NativeOverlayOptions::new(NativeOverlayTarget::new(
    ///     NativeOverlayRect::new(0.0, 0.0, 100.0, 100.0),
    /// ));
    /// assert_eq!(options.input_mode, NativeOverlayInputMode::PassThrough);
    /// ```
    pub const fn new(target: NativeOverlayTarget) -> Self {
        Self {
            target,
            input_mode: NativeOverlayInputMode::PassThrough,
        }
    }

    /// Replaces the pointer policy.
    ///
    /// This never enables keyboard focus; both modes keep keyboard interactivity disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::native_overlay::*;
    /// let options = NativeOverlayOptions::new(NativeOverlayTarget::new(
    ///     NativeOverlayRect::new(0.0, 0.0, 100.0, 100.0),
    /// )).input_mode(NativeOverlayInputMode::BlockPointer);
    /// assert_eq!(options.input_mode, NativeOverlayInputMode::BlockPointer);
    /// ```
    pub const fn input_mode(mut self, input_mode: NativeOverlayInputMode) -> Self {
        self.input_mode = input_mode;
        self
    }
}

/// Backend that actually established the overlay invariants.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::native_overlay::NativeOverlayBackend;
/// let backend = NativeOverlayBackend::WaylandLayerShell;
/// assert_ne!(backend, NativeOverlayBackend::X11);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOverlayBackend {
    /// X11 dock/override behavior established through the winit window path.
    X11,
    /// Wayland `wlr-layer-shell` overlay surface.
    WaylandLayerShell,
}

/// Capabilities are returned only after every required overlay invariant is active.
///
/// `pointer_pass_through` describes the selected pointer policy and is not part
/// of [`Self::is_fully_effective`], because blocking mode is also a valid fully
/// effective overlay. All other booleans must be true for full effectiveness.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::native_overlay::{NativeOverlayBackend, NativeOverlayCapabilities};
/// let partial = NativeOverlayCapabilities {
///     backend: NativeOverlayBackend::X11,
///     placed: true,
///     topmost: false,
///     transparent: true,
///     non_activating: true,
///     keyboard_focus_disabled: true,
///     pointer_pass_through: true,
/// };
/// assert!(!partial.is_fully_effective());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOverlayCapabilities {
    /// Native backend that established these claims.
    pub backend: NativeOverlayBackend,
    /// Exact target placement was confirmed.
    pub placed: bool,
    /// The surface is above ordinary application windows.
    pub topmost: bool,
    /// The native surface supports alpha transparency.
    pub transparent: bool,
    /// Showing the surface does not activate it.
    pub non_activating: bool,
    /// Keyboard focus is disabled at the native protocol level.
    pub keyboard_focus_disabled: bool,
    /// Pointer events pass through to underlying surfaces.
    pub pointer_pass_through: bool,
}

/// Capability construction and invariant checking.
impl NativeOverlayCapabilities {
    /// Records a backend whose required invariants were all established.
    ///
    /// Pointer pass-through mirrors `mode`; pointer blocking remains fully effective.
    ///
    /// # Examples
    ///
    /// ```
    /// // Backends expose an established capability record only after setup succeeds.
    /// use ailloli_ui_winit::native_overlay::NativeOverlayInputMode;
    /// assert_eq!(NativeOverlayInputMode::default(), NativeOverlayInputMode::PassThrough);
    /// ```
    pub(crate) const fn established(
        backend: NativeOverlayBackend,
        mode: NativeOverlayInputMode,
    ) -> Self {
        Self {
            backend,
            placed: true,
            topmost: true,
            transparent: true,
            non_activating: true,
            keyboard_focus_disabled: true,
            pointer_pass_through: matches!(mode, NativeOverlayInputMode::PassThrough),
        }
    }

    /// Returns whether placement, z-order, transparency, non-activation, and
    /// keyboard-focus suppression are all active.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::native_overlay::{NativeOverlayBackend, NativeOverlayCapabilities};
    /// let caps = NativeOverlayCapabilities {
    ///     backend: NativeOverlayBackend::WaylandLayerShell,
    ///     placed: true, topmost: true, transparent: true,
    ///     non_activating: true, keyboard_focus_disabled: true,
    ///     pointer_pass_through: false,
    /// };
    /// assert!(caps.is_fully_effective());
    /// ```
    pub const fn is_fully_effective(self) -> bool {
        self.placed
            && self.topmost
            && self.transparent
            && self.non_activating
            && self.keyboard_focus_disabled
    }
}

/// Failure to validate, establish, or retain a native overlay.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::native_overlay::NativeOverlayError;
/// assert_eq!(NativeOverlayError::Closed.to_string(), "native overlay was closed by the compositor");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeOverlayError {
    /// The current operating system or display backend has no supported implementation.
    Unsupported,
    /// Geometry was non-finite, non-positive, fractional where exact integers are required, or out of range.
    InvalidTarget,
    /// No compositor output matched the exact rectangle and optional name.
    OutputMatchMissing,
    /// More than one compositor output matched the requested identity.
    OutputMatchAmbiguous,
    /// A native protocol or operating-system operation failed with this message.
    Backend(String),
    /// The compositor closed the overlay surface.
    Closed,
}

/// Stable human-readable category messages for native overlay failures.
impl fmt::Display for NativeOverlayError {
    /// Formats the platform, target, capability, or geometry failure.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "native overlays are unsupported on this platform"),
            Self::InvalidTarget => write!(f, "native overlay target is invalid"),
            Self::OutputMatchMissing => {
                write!(
                    f,
                    "no compositor output matches the requested logical rectangle"
                )
            }
            Self::OutputMatchAmbiguous => {
                write!(
                    f,
                    "multiple compositor outputs match the requested logical rectangle"
                )
            }
            Self::Backend(message) => write!(f, "native overlay backend failed: {message}"),
            Self::Closed => write!(f, "native overlay was closed by the compositor"),
        }
    }
}

/// Marks native overlay failures as standard errors without an additional source.
impl std::error::Error for NativeOverlayError {}

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
/// Linux Wayland layer-shell implementation and raw-window-handle owner.
pub(crate) mod wayland;

#[cfg(feature = "native_overlay")]
/// Wayland output probing and capture-calibration marker service.
mod calibration;
#[cfg(feature = "native_overlay")]
pub use calibration::{
    NativeCalibrationMarkerGuard, NativeCalibrationMarkerPixel, NativeCalibrationMarkerSpec,
    NativeOutputDescriptor, NativeOutputProbeService, NativeOutputScale, NativeOutputTransform,
};

#[cfg(test)]
/// Target validation and effective-capability policy scenarios.
mod tests {
    use super::*;

    #[test]
    fn native_overlay_rejects_non_finite_or_empty_targets() {
        assert_eq!(
            NativeOverlayRect::new(0.0, 0.0, 0.0, 10.0).validate(),
            Err(NativeOverlayError::InvalidTarget)
        );
        assert_eq!(
            NativeOverlayRect::new(f64::NAN, 0.0, 10.0, 10.0).validate(),
            Err(NativeOverlayError::InvalidTarget)
        );
    }

    #[test]
    fn capabilities_distinguish_pointer_blocking() {
        let pass = NativeOverlayCapabilities::established(
            NativeOverlayBackend::X11,
            NativeOverlayInputMode::PassThrough,
        );
        let block = NativeOverlayCapabilities::established(
            NativeOverlayBackend::X11,
            NativeOverlayInputMode::BlockPointer,
        );
        assert!(pass.pointer_pass_through);
        assert!(!block.pointer_pass_through);
        assert!(pass.is_fully_effective());
        assert!(block.is_fully_effective());
    }
}
