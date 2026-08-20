//! Portable native_overlay contracts and Linux backend selection.

use std::fmt;

/// Logical desktop rectangle used to select and place an overlay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeOverlayRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl NativeOverlayRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

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
#[derive(Debug, Clone, PartialEq)]
pub struct NativeOverlayTarget {
    pub logical_rect: NativeOverlayRect,
    /// Optional stable output name used as an additional, never substitute, discriminator.
    pub output_name: Option<String>,
}

impl NativeOverlayTarget {
    pub const fn new(logical_rect: NativeOverlayRect) -> Self {
        Self {
            logical_rect,
            output_name: None,
        }
    }

    pub fn output_name(mut self, name: impl Into<String>) -> Self {
        self.output_name = Some(name.into());
        self
    }
}

/// Pointer behavior of the native overlay. Keyboard focus is never requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NativeOverlayInputMode {
    #[default]
    PassThrough,
    BlockPointer,
}

/// Native overlay creation options.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeOverlayOptions {
    pub target: NativeOverlayTarget,
    pub input_mode: NativeOverlayInputMode,
}

impl NativeOverlayOptions {
    pub const fn new(target: NativeOverlayTarget) -> Self {
        Self {
            target,
            input_mode: NativeOverlayInputMode::PassThrough,
        }
    }

    pub const fn input_mode(mut self, input_mode: NativeOverlayInputMode) -> Self {
        self.input_mode = input_mode;
        self
    }
}

/// Backend that actually established the overlay invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOverlayBackend {
    X11,
    WaylandLayerShell,
}

/// Capabilities are returned only after every required overlay invariant is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOverlayCapabilities {
    pub backend: NativeOverlayBackend,
    pub placed: bool,
    pub topmost: bool,
    pub transparent: bool,
    pub non_activating: bool,
    pub keyboard_focus_disabled: bool,
    pub pointer_pass_through: bool,
}

impl NativeOverlayCapabilities {
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

    pub const fn is_fully_effective(self) -> bool {
        self.placed
            && self.topmost
            && self.transparent
            && self.non_activating
            && self.keyboard_focus_disabled
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeOverlayError {
    Unsupported,
    InvalidTarget,
    OutputMatchMissing,
    OutputMatchAmbiguous,
    Backend(String),
    Closed,
}

impl fmt::Display for NativeOverlayError {
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

impl std::error::Error for NativeOverlayError {}

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
pub(crate) mod wayland;

#[cfg(feature = "native_overlay")]
mod calibration;
#[cfg(feature = "native_overlay")]
pub use calibration::{
    NativeCalibrationMarkerGuard, NativeCalibrationMarkerPixel, NativeCalibrationMarkerSpec,
    NativeOutputDescriptor, NativeOutputProbeService, NativeOutputScale, NativeOutputTransform,
};

#[cfg(test)]
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
