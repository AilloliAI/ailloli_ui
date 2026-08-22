//! CSS-like intrinsic, fixed, fill, and percentage axis lengths.

/// CSS-like length for layout axes (width, height, min/max).
///
/// Possible values are intrinsic [`Length::Auto`], fixed [`Length::Px`], parent
/// fill, and [`Length::Percent`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Length;
/// assert_eq!(Length::percent(50.0).resolve(200.0), Some(100.0));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length {
    /// Intrinsic size (children, text measure).
    #[default]
    Auto,
    /// Fixed logical pixels; resolution floors negative values and NaN at zero.
    /// Positive infinity remains infinite.
    Px(f32),
    /// Fill the parent on this axis when resolved via [`Length::resolve`].
    ///
    /// In a flex main axis, parent layout treats `Fill` as **remaining space**
    /// (grow participant) instead of calling `resolve` with the full parent max.
    Fill,
    /// Fraction of parent available space.
    ///
    /// Direct construction stores the fraction verbatim; [`Length::percent`]
    /// accepts either fractional or percentage-style input.
    Percent(f32),
}

impl Length {
    /// Creates a fixed logical-pixel length without immediate normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// assert_eq!(Length::px(24.0), Length::Px(24.0));
    /// ```
    pub fn px(value: f32) -> Self {
        Self::Px(value)
    }

    /// Fill parent on this axis (see [`Length::Fill`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// assert_eq!(Length::fill(), Length::Fill);
    /// ```
    pub fn fill() -> Self {
        Self::Fill
    }

    /// Creates a percentage/fraction of parent available space.
    ///
    /// Values with absolute magnitude at most `1.0` are treated as fractions,
    /// so `0.5` and `1.0` mean 50% and 100%. Larger magnitudes are divided by
    /// 100, so `50.0` also means 50%. Negative fractions resolve to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// assert_eq!(Length::percent(50.0), Length::Percent(0.5));
    /// assert_eq!(Length::percent(0.5), Length::Percent(0.5));
    /// ```
    pub fn percent(value: f32) -> Self {
        let fraction = if value.abs() <= 1.0 {
            value
        } else {
            value / 100.0
        };
        Self::Percent(fraction)
    }

    /// `true` when this axis uses intrinsic sizing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// assert!(Length::Auto.is_auto());
    /// assert!(!Length::Fill.is_auto());
    /// ```
    pub fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Resolves against parent `available` logical pixels.
    ///
    /// [`Self::Auto`] returns `None`. Fixed, fill, and percentage results are
    /// floored at zero; they are not capped to `available`. Non-finite results
    /// follow floating-point `max` semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    ///
    /// assert_eq!(Length::Auto.resolve(200.0), None);
    /// assert_eq!(Length::px(24.0).resolve(200.0), Some(24.0));
    /// assert_eq!(Length::percent(50.0).resolve(200.0), Some(100.0));
    /// assert_eq!(Length::fill().resolve(200.0), Some(200.0));
    /// assert_eq!(Length::px(-1.0).resolve(200.0), Some(0.0));
    /// ```
    pub fn resolve(self, available: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(value) => Some(value.max(0.0)),
            Self::Fill => Some(available.max(0.0)),
            Self::Percent(value) => Some((available * value).max(0.0)),
        }
    }
}

impl From<f32> for Length {
    fn from(value: f32) -> Self {
        Self::Px(value)
    }
}
