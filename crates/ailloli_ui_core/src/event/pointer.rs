use crate::{Point, Size};

use super::keyboard::Modifiers;

/// Provider-neutral pointer button identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
    /// Additional button index from the platform.
    Other(u16),
}

/// Compatibility name for [`PointerButton`].
///
/// New APIs and consumer code should prefer [`PointerButton`]. The alias is
/// intentionally not deprecated yet so existing applications can migrate
/// without turning warning-deny builds red.
pub type MouseButton = PointerButton;

/// Provider-neutral pointer identifier.
///
/// Identifiers are stable within one logical window presentation generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerId(u64);

impl PointerId {
    /// Conventional identifier used by the compatibility mouse event path.
    pub const MOUSE: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Default for PointerId {
    fn default() -> Self {
        Self::MOUSE
    }
}

/// Physical source that produced a pointer event.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerSource {
    #[default]
    Mouse,
    Touch,
    Pen,
    Eraser,
    Other,
}

/// Whether a pointer gesture is allowed to activate action controls.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivationKind {
    #[default]
    Normal,
    /// The platform used this gesture only to activate/focus the application.
    FocusOnly,
}

/// Invalid provider-neutral pointer data.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PointerSampleError {
    #[error("pointer position must be finite")]
    NonFinitePosition,
    #[error("pointer pressure must be finite and within 0..=1")]
    InvalidPressure,
    #[error("pointer tilt must be finite and within -90..=90 degrees")]
    InvalidTilt,
    #[error("pointer twist must be finite and within 0..360 degrees")]
    InvalidTwist,
    #[error("pointer contact size must be finite and non-negative")]
    InvalidContactSize,
}

/// Optional high-fidelity data associated with a pointer event.
///
/// Mouse events normally provide only `id`, `source`, and `position`. Touch and
/// pen adapters can additionally supply normalized pressure, tilt, twist, and
/// contact size without leaking provider-specific event types into the runtime.
/// `is_primary` identifies the primary pointer for its source. Providers that
/// can expose multiple simultaneous pointers should use
/// [`PointerSample::new_with_primary`] or [`PointerSample::with_primary`]
/// instead of relying on the compatibility default of [`PointerSample::new`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerSample {
    id: PointerId,
    source: PointerSource,
    position: Point,
    is_primary: bool,
    pressure: Option<f32>,
    tilt: Option<(f32, f32)>,
    twist: Option<f32>,
    contact_size: Option<Size>,
    activation: ActivationKind,
}

impl PointerSample {
    /// Creates a sample for the compatibility single-pointer path.
    ///
    /// This constructor keeps the original three-argument API and treats the
    /// sample as primary. Multi-pointer providers should use
    /// [`Self::new_with_primary`] so secondary pointers are represented
    /// explicitly.
    pub fn new(
        id: PointerId,
        source: PointerSource,
        position: Point,
    ) -> Result<Self, PointerSampleError> {
        Self::new_with_primary(id, source, position, true)
    }

    /// Creates a sample with an explicit primary-pointer classification.
    pub fn new_with_primary(
        id: PointerId,
        source: PointerSource,
        position: Point,
        is_primary: bool,
    ) -> Result<Self, PointerSampleError> {
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(PointerSampleError::NonFinitePosition);
        }
        Ok(Self {
            id,
            source,
            position,
            is_primary,
            pressure: None,
            tilt: None,
            twist: None,
            contact_size: None,
            activation: ActivationKind::Normal,
        })
    }

    pub const fn id(&self) -> PointerId {
        self.id
    }

    pub const fn source(&self) -> PointerSource {
        self.source
    }

    pub const fn position(&self) -> Point {
        self.position
    }

    /// Returns whether this is the primary pointer for its source.
    pub const fn is_primary(&self) -> bool {
        self.is_primary
    }

    pub const fn pressure(&self) -> Option<f32> {
        self.pressure
    }

    pub const fn tilt(&self) -> Option<(f32, f32)> {
        self.tilt
    }

    pub const fn twist(&self) -> Option<f32> {
        self.twist
    }

    pub const fn contact_size(&self) -> Option<Size> {
        self.contact_size
    }

    pub const fn activation(&self) -> ActivationKind {
        self.activation
    }

    pub fn with_pressure(mut self, pressure: f32) -> Result<Self, PointerSampleError> {
        if !pressure.is_finite() || !(0.0..=1.0).contains(&pressure) {
            return Err(PointerSampleError::InvalidPressure);
        }
        self.pressure = Some(pressure);
        Ok(self)
    }

    pub fn with_tilt(mut self, x: f32, y: f32) -> Result<Self, PointerSampleError> {
        if !x.is_finite()
            || !y.is_finite()
            || !(-90.0..=90.0).contains(&x)
            || !(-90.0..=90.0).contains(&y)
        {
            return Err(PointerSampleError::InvalidTilt);
        }
        self.tilt = Some((x, y));
        Ok(self)
    }

    pub fn with_twist(mut self, twist: f32) -> Result<Self, PointerSampleError> {
        if !twist.is_finite() || !(0.0..360.0).contains(&twist) {
            return Err(PointerSampleError::InvalidTwist);
        }
        self.twist = Some(twist);
        Ok(self)
    }

    pub fn with_contact_size(mut self, size: Size) -> Result<Self, PointerSampleError> {
        if !size.w.is_finite() || !size.h.is_finite() || size.w < 0.0 || size.h < 0.0 {
            return Err(PointerSampleError::InvalidContactSize);
        }
        self.contact_size = Some(size);
        Ok(self)
    }

    pub const fn with_activation(mut self, activation: ActivationKind) -> Self {
        self.activation = activation;
        self
    }

    /// Overrides the primary-pointer classification.
    pub const fn with_primary(mut self, is_primary: bool) -> Self {
        self.is_primary = is_primary;
        self
    }
}

/// Scroll wheel delta in lines or pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelDelta {
    /// Abstract line units (platform-defined).
    LineDelta { x: f32, y: f32 },
    /// Logical pixel delta.
    PixelDelta { x: f32, y: f32 },
}

/// Pointer move, button, cancellation, or wheel event.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PointerEvent {
    /// Cursor moved to `pos`.
    Moved { pos: Point, modifiers: Modifiers },
    /// Button pressed or released at `pos`.
    Button {
        pos: Point,
        button: PointerButton,
        pressed: bool,
        modifiers: Modifiers,
    },
    /// The active gesture was cancelled by the host or input provider.
    ///
    /// Cancellation never represents a click release. Runtimes must clear
    /// press/capture state for the corresponding [`PointerId`].
    Cancelled { pos: Point, modifiers: Modifiers },
    /// Scroll wheel at `pos`.
    Wheel {
        pos: Point,
        delta: WheelDelta,
        modifiers: Modifiers,
        /// High-resolution trackpad scroll when supported.
        precise: bool,
    },
}

impl PointerEvent {
    /// Creates a pointer movement event in logical coordinates.
    pub const fn moved(pos: Point, modifiers: Modifiers) -> Self {
        Self::Moved { pos, modifiers }
    }

    /// Creates a pointer-button transition in logical coordinates.
    pub const fn button(
        pos: Point,
        button: PointerButton,
        pressed: bool,
        modifiers: Modifiers,
    ) -> Self {
        Self::Button {
            pos,
            button,
            pressed,
            modifiers,
        }
    }

    /// Creates a pointer cancellation event.
    pub const fn cancelled(pos: Point, modifiers: Modifiers) -> Self {
        Self::Cancelled { pos, modifiers }
    }

    /// Creates a pointer-wheel event in logical coordinates.
    pub const fn wheel(pos: Point, delta: WheelDelta, modifiers: Modifiers, precise: bool) -> Self {
        Self::Wheel {
            pos,
            delta,
            modifiers,
            precise,
        }
    }

    /// Logical position associated with this event.
    pub const fn position(&self) -> Point {
        match self {
            Self::Moved { pos, .. }
            | Self::Button { pos, .. }
            | Self::Cancelled { pos, .. }
            | Self::Wheel { pos, .. } => *pos,
        }
    }

    /// Keyboard modifiers observed with this event.
    pub const fn modifiers(&self) -> Modifiers {
        match self {
            Self::Moved { modifiers, .. }
            | Self::Button { modifiers, .. }
            | Self::Cancelled { modifiers, .. }
            | Self::Wheel { modifiers, .. } => *modifiers,
        }
    }

    /// Button and transition state when this is a button event.
    pub const fn button_transition(&self) -> Option<(PointerButton, bool)> {
        match self {
            Self::Button {
                button, pressed, ..
            } => Some((*button, *pressed)),
            Self::Moved { .. } | Self::Cancelled { .. } | Self::Wheel { .. } => None,
        }
    }

    /// Wheel delta and precision when this is a wheel event.
    pub const fn wheel_delta(&self) -> Option<(WheelDelta, bool)> {
        match self {
            Self::Wheel { delta, precise, .. } => Some((*delta, *precise)),
            Self::Moved { .. } | Self::Button { .. } | Self::Cancelled { .. } => None,
        }
    }

    /// Returns whether the provider cancelled the active gesture.
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}
