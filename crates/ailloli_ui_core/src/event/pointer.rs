//! Provider-neutral pointer identity, high-fidelity samples, and routed events.

use crate::{Point, Size};

use super::keyboard::Modifiers;

/// Provider-neutral pointer button identifier.
///
/// Possible values are left, middle, right, and an uninterpreted provider
/// number through [`PointerButton::Other`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::PointerButton;
/// assert_ne!(PointerButton::Left, PointerButton::Other(4));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    /// Primary/left button.
    Left,
    /// Middle/auxiliary button.
    Middle,
    /// Secondary/right button.
    Right,
    /// Additional platform button number with no framework interpretation.
    Other(u16),
}

/// Compatibility name for [`PointerButton`].
///
/// New APIs and consumer code should prefer [`PointerButton`]. The alias is
/// intentionally not deprecated yet so existing applications can migrate
/// without turning warning-deny builds red.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{MouseButton, PointerButton};
/// let button: MouseButton = PointerButton::Left;
/// assert_eq!(button, PointerButton::Left);
/// ```
pub type MouseButton = PointerButton;

/// Provider-neutral pointer identifier.
///
/// Identifiers are stable within one logical window presentation generation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::PointerId;
/// assert_eq!(PointerId::new(7).get(), 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerId(u64);

impl PointerId {
    /// Conventional identifier used by the compatibility mouse event path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::PointerId;
    /// assert_eq!(PointerId::MOUSE.get(), 0);
    /// ```
    pub const MOUSE: Self = Self(0);

    /// Wraps a provider-assigned numeric pointer identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::PointerId;
    /// assert_eq!(PointerId::new(9).get(), 9);
    /// ```
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the wrapped provider-assigned number.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::PointerId;
    /// assert_eq!(PointerId::new(9).get(), 9);
    /// ```
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
///
/// Possible values distinguish mouse, touch, pen, eraser, and other providers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::PointerSource;
/// assert_eq!(PointerSource::default(), PointerSource::Mouse);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerSource {
    /// Conventional mouse or trackpad pointer; this is the default.
    #[default]
    Mouse,
    /// Direct or indirect touch contact.
    Touch,
    /// Pen or stylus tip.
    Pen,
    /// Pen eraser end.
    Eraser,
    /// Provider source not represented by a dedicated variant.
    Other,
}

/// Whether a pointer gesture is allowed to activate action controls.
///
/// Possible values are normal activation and focus-only activation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::ActivationKind;
/// assert_eq!(ActivationKind::default(), ActivationKind::Normal);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivationKind {
    /// Gesture may focus and activate controls; this is the default.
    #[default]
    Normal,
    /// The platform used this gesture only to activate/focus the application.
    FocusOnly,
}

/// Invalid provider-neutral pointer data.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSampleError, PointerSource}, Point};
/// assert_eq!(PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::new(f32::NAN, 0.0)).unwrap_err(), PointerSampleError::NonFinitePosition);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PointerSampleError {
    /// At least one logical position coordinate is NaN or infinite.
    #[error("pointer position must be finite")]
    NonFinitePosition,
    /// Pressure is non-finite or outside the normalized `0.0..=1.0` range.
    #[error("pointer pressure must be finite and within 0..=1")]
    InvalidPressure,
    /// Either tilt component is non-finite or outside `-90.0..=90.0` degrees.
    #[error("pointer tilt must be finite and within -90..=90 degrees")]
    InvalidTilt,
    /// Twist is non-finite or outside the half-open `0.0..360.0` degree range.
    #[error("pointer twist must be finite and within 0..360 degrees")]
    InvalidTwist,
    /// Contact width or height is non-finite or negative.
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
/// let sample = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::new(1.0, 2.0))?;
/// assert!(sample.is_primary());
/// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerSample {
    /// Stable pointer identity within the provider session.
    id: PointerId,
    /// Device family that produced the sample.
    source: PointerSource,
    /// Pointer location in logical window coordinates.
    position: Point,
    /// Whether this is the primary pointer for its source.
    is_primary: bool,
    /// Optional normalized pressure in the inclusive range `0.0..=1.0`.
    pressure: Option<f32>,
    /// Optional horizontal and vertical tilt angles in degrees.
    tilt: Option<(f32, f32)>,
    /// Optional clockwise barrel rotation in degrees.
    twist: Option<f32>,
    /// Optional contact footprint in logical pixels.
    contact_size: Option<Size>,
    /// Activation semantics inferred from the provider event.
    activation: ActivationKind,
}

impl PointerSample {
    /// Creates a sample for the compatibility single-pointer path.
    ///
    /// This constructor keeps the original three-argument API and treats the
    /// sample as primary. Multi-pointer providers should use
    /// [`Self::new_with_primary`] so secondary pointers are represented
    /// explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`PointerSampleError::NonFinitePosition`] unless both logical
    /// position coordinates are finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::new(1.0, 2.0))?;
    /// assert_eq!(sample.position(), Point::new(1.0, 2.0));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub fn new(
        id: PointerId,
        source: PointerSource,
        position: Point,
    ) -> Result<Self, PointerSampleError> {
        Self::new_with_primary(id, source, position, true)
    }

    /// Creates a sample with an explicit primary-pointer classification.
    ///
    /// Optional pressure, tilt, twist, and contact size start as `None`, and
    /// activation starts as [`ActivationKind::Normal`].
    ///
    /// # Errors
    ///
    /// Returns [`PointerSampleError::NonFinitePosition`] unless both logical
    /// position coordinates are finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new_with_primary(PointerId::new(2), PointerSource::Touch, Point::new(1.0, 2.0), false)?;
    /// assert!(!sample.is_primary());
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
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

    /// Returns the provider-neutral pointer identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(2), PointerSource::Touch, Point::default())?;
    /// assert_eq!(sample.id(), PointerId::new(2));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn id(&self) -> PointerId {
        self.id
    }

    /// Returns the physical source classification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(2), PointerSource::Pen, Point::default())?;
    /// assert_eq!(sample.source(), PointerSource::Pen);
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn source(&self) -> PointerSource {
        self.source
    }

    /// Returns the logical window position.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::new(3.0, 4.0))?;
    /// assert_eq!(sample.position(), Point::new(3.0, 4.0));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn position(&self) -> Point {
        self.position
    }

    /// Returns whether this is the primary pointer for its source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// assert!(PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::default())?.is_primary());
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn is_primary(&self) -> bool {
        self.is_primary
    }

    /// Returns normalized pressure, or `None` when the provider omitted it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Pen, Point::default())?.with_pressure(0.5)?;
    /// assert_eq!(sample.pressure(), Some(0.5));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn pressure(&self) -> Option<f32> {
        self.pressure
    }

    /// Returns `(x, y)` pen tilt in degrees, or `None` when unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Pen, Point::default())?.with_tilt(10.0, -5.0)?;
    /// assert_eq!(sample.tilt(), Some((10.0, -5.0)));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn tilt(&self) -> Option<(f32, f32)> {
        self.tilt
    }

    /// Returns clockwise pen twist in `0.0..360.0` degrees, when available.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Pen, Point::default())?.with_twist(45.0)?;
    /// assert_eq!(sample.twist(), Some(45.0));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn twist(&self) -> Option<f32> {
        self.twist
    }

    /// Returns logical contact width/height, or `None` when unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point, Size};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Touch, Point::default())?.with_contact_size(Size::new(4.0, 5.0))?;
    /// assert_eq!(sample.contact_size(), Some(Size::new(4.0, 5.0)));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn contact_size(&self) -> Option<Size> {
        self.contact_size
    }

    /// Returns whether this gesture may activate controls.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{ActivationKind, PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::default())?.with_activation(ActivationKind::FocusOnly);
    /// assert_eq!(sample.activation(), ActivationKind::FocusOnly);
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn activation(&self) -> ActivationKind {
        self.activation
    }

    /// Sets normalized pressure in the inclusive range `0.0..=1.0`.
    ///
    /// # Errors
    ///
    /// Returns [`PointerSampleError::InvalidPressure`] for non-finite or
    /// out-of-range input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Pen, Point::default())?.with_pressure(0.75)?;
    /// assert_eq!(sample.pressure(), Some(0.75));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub fn with_pressure(mut self, pressure: f32) -> Result<Self, PointerSampleError> {
        if !pressure.is_finite() || !(0.0..=1.0).contains(&pressure) {
            return Err(PointerSampleError::InvalidPressure);
        }
        self.pressure = Some(pressure);
        Ok(self)
    }

    /// Sets pen tilt components in inclusive degrees `-90.0..=90.0`.
    ///
    /// # Errors
    ///
    /// Returns [`PointerSampleError::InvalidTilt`] if either component is
    /// non-finite or outside the supported range.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Pen, Point::default())?.with_tilt(20.0, -20.0)?;
    /// assert_eq!(sample.tilt(), Some((20.0, -20.0)));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
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

    /// Sets clockwise pen twist in the half-open range `0.0..360.0` degrees.
    ///
    /// # Errors
    ///
    /// Returns [`PointerSampleError::InvalidTwist`] for non-finite input or for
    /// a value below zero or at least 360 degrees.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Pen, Point::default())?.with_twist(180.0)?;
    /// assert_eq!(sample.twist(), Some(180.0));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub fn with_twist(mut self, twist: f32) -> Result<Self, PointerSampleError> {
        if !twist.is_finite() || !(0.0..360.0).contains(&twist) {
            return Err(PointerSampleError::InvalidTwist);
        }
        self.twist = Some(twist);
        Ok(self)
    }

    /// Sets the non-negative contact size in logical pixels.
    ///
    /// Zero width or height is valid and distinct from `None`.
    ///
    /// # Errors
    ///
    /// Returns [`PointerSampleError::InvalidContactSize`] for non-finite or
    /// negative dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point, Size};
    /// let sample = PointerSample::new(PointerId::new(1), PointerSource::Touch, Point::default())?.with_contact_size(Size::new(4.0, 5.0))?;
    /// assert_eq!(sample.contact_size(), Some(Size::new(4.0, 5.0)));
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub fn with_contact_size(mut self, size: Size) -> Result<Self, PointerSampleError> {
        if !size.w.is_finite() || !size.h.is_finite() || size.w < 0.0 || size.h < 0.0 {
            return Err(PointerSampleError::InvalidContactSize);
        }
        self.contact_size = Some(size);
        Ok(self)
    }

    /// Replaces the control-activation classification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{ActivationKind, PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, Point::default())?.with_activation(ActivationKind::FocusOnly);
    /// assert_eq!(sample.activation(), ActivationKind::FocusOnly);
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn with_activation(mut self, activation: ActivationKind) -> Self {
        self.activation = activation;
        self
    }

    /// Overrides the primary-pointer classification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{PointerId, PointerSample, PointerSource}, Point};
    /// let sample = PointerSample::new(PointerId::new(2), PointerSource::Touch, Point::default())?.with_primary(false);
    /// assert!(!sample.is_primary());
    /// # Ok::<(), ailloli_ui_core::event::PointerSampleError>(())
    /// ```
    pub const fn with_primary(mut self, is_primary: bool) -> Self {
        self.is_primary = is_primary;
        self
    }
}

/// Scroll wheel delta in lines or pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::WheelDelta;
/// let delta = WheelDelta::PixelDelta { x: 1.0, y: -2.0 };
/// assert!(matches!(delta, WheelDelta::PixelDelta { .. }));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelDelta {
    /// Abstract line units (platform-defined).
    LineDelta {
        /// Horizontal platform line units.
        x: f32,
        /// Vertical platform line units.
        y: f32,
    },
    /// Logical pixel delta.
    PixelDelta {
        /// Horizontal logical pixels.
        x: f32,
        /// Vertical logical pixels.
        y: f32,
    },
}

/// Pointer move, button, cancellation, or wheel event.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{event::{Modifiers, PointerEvent}, Point};
/// let event = PointerEvent::moved(Point::new(1.0, 2.0), Modifiers::default());
/// assert_eq!(event.position(), Point::new(1.0, 2.0));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PointerEvent {
    /// Cursor moved to `pos`.
    Moved {
        /// Current logical window position.
        pos: Point,
        /// Modifier snapshot at the time of motion.
        modifiers: Modifiers,
    },
    /// Button pressed or released at `pos`.
    Button {
        /// Logical window position of the transition.
        pos: Point,
        /// Button whose state changed.
        button: PointerButton,
        /// `true` for press and `false` for release.
        pressed: bool,
        /// Modifier snapshot at the time of transition.
        modifiers: Modifiers,
    },
    /// The active gesture was cancelled by the host or input provider.
    ///
    /// Cancellation never represents a click release. Runtimes must clear
    /// press/capture state for the corresponding [`PointerId`].
    Cancelled {
        /// Last known logical window position.
        pos: Point,
        /// Modifier snapshot at cancellation time.
        modifiers: Modifiers,
    },
    /// Scroll wheel at `pos`.
    Wheel {
        /// Logical window position associated with the wheel input.
        pos: Point,
        /// Provider wheel displacement before widget-specific normalization.
        delta: WheelDelta,
        /// Modifier snapshot at wheel time.
        modifiers: Modifiers,
        /// High-resolution trackpad scroll when supported.
        precise: bool,
    },
}

impl PointerEvent {
    /// Creates a pointer movement event in logical coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerEvent}, Point};
    /// assert_eq!(PointerEvent::moved(Point::new(1.0, 2.0), Modifiers::default()).position(), Point::new(1.0, 2.0));
    /// ```
    pub const fn moved(pos: Point, modifiers: Modifiers) -> Self {
        Self::Moved { pos, modifiers }
    }

    /// Creates a pointer-button transition in logical coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerButton, PointerEvent}, Point};
    /// let event = PointerEvent::button(Point::default(), PointerButton::Left, true, Modifiers::default());
    /// assert_eq!(event.button_transition(), Some((PointerButton::Left, true)));
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerEvent}, Point};
    /// assert!(PointerEvent::cancelled(Point::default(), Modifiers::default()).is_cancelled());
    /// ```
    pub const fn cancelled(pos: Point, modifiers: Modifiers) -> Self {
        Self::Cancelled { pos, modifiers }
    }

    /// Creates a pointer-wheel event in logical coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerEvent, WheelDelta}, Point};
    /// let event = PointerEvent::wheel(Point::default(), WheelDelta::LineDelta { x: 0.0, y: 1.0 }, Modifiers::default(), false);
    /// assert!(event.wheel_delta().is_some());
    /// ```
    pub const fn wheel(pos: Point, delta: WheelDelta, modifiers: Modifiers, precise: bool) -> Self {
        Self::Wheel {
            pos,
            delta,
            modifiers,
            precise,
        }
    }

    /// Logical position associated with this event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerEvent}, Point};
    /// let event = PointerEvent::moved(Point::new(2.0, 3.0), Modifiers::default());
    /// assert_eq!(event.position(), Point::new(2.0, 3.0));
    /// ```
    pub const fn position(&self) -> Point {
        match self {
            Self::Moved { pos, .. }
            | Self::Button { pos, .. }
            | Self::Cancelled { pos, .. }
            | Self::Wheel { pos, .. } => *pos,
        }
    }

    /// Keyboard modifiers observed with this event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerEvent}, Point};
    /// let modifiers = Modifiers { shift: true, ..Modifiers::default() };
    /// assert!(PointerEvent::moved(Point::default(), modifiers).modifiers().shift);
    /// ```
    pub const fn modifiers(&self) -> Modifiers {
        match self {
            Self::Moved { modifiers, .. }
            | Self::Button { modifiers, .. }
            | Self::Cancelled { modifiers, .. }
            | Self::Wheel { modifiers, .. } => *modifiers,
        }
    }

    /// Returns the button and transition state for a button event.
    ///
    /// The boolean is `true` for press and `false` for release; all other event
    /// variants return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerButton, PointerEvent}, Point};
    /// let event = PointerEvent::button(Point::default(), PointerButton::Right, false, Modifiers::default());
    /// assert_eq!(event.button_transition(), Some((PointerButton::Right, false)));
    /// ```
    pub const fn button_transition(&self) -> Option<(PointerButton, bool)> {
        match self {
            Self::Button {
                button, pressed, ..
            } => Some((*button, *pressed)),
            Self::Moved { .. } | Self::Cancelled { .. } | Self::Wheel { .. } => None,
        }
    }

    /// Returns wheel displacement and the high-resolution flag for a wheel event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerEvent, WheelDelta}, Point};
    /// let event = PointerEvent::wheel(Point::default(), WheelDelta::PixelDelta { x: 0.0, y: 2.0 }, Modifiers::default(), true);
    /// assert_eq!(event.wheel_delta(), Some((WheelDelta::PixelDelta { x: 0.0, y: 2.0 }, true)));
    /// ```
    pub const fn wheel_delta(&self) -> Option<(WheelDelta, bool)> {
        match self {
            Self::Wheel { delta, precise, .. } => Some((*delta, *precise)),
            Self::Moved { .. } | Self::Button { .. } | Self::Cancelled { .. } => None,
        }
    }

    /// Returns whether the provider cancelled the active gesture.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, PointerEvent}, Point};
    /// assert!(!PointerEvent::moved(Point::default(), Modifiers::default()).is_cancelled());
    /// ```
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}
