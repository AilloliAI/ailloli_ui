/// CSS-like length for layout axes (width, height, min/max).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length {
    /// Intrinsic size (children, text measure).
    #[default]
    Auto,
    /// Fixed logical pixels.
    Px(f32),
    /// Fill the parent on this axis when resolved via [`Length::resolve`].
    ///
    /// In a flex main axis, parent layout treats `Fill` as **remaining space**
    /// (grow participant) instead of calling `resolve` with the full parent max.
    Fill,
    /// Fraction of parent available space.
    Percent(f32),
}

impl Length {
    /// Fixed pixel length.
    pub fn px(value: f32) -> Self {
        Self::Px(value)
    }

    /// Fill parent on this axis (see [`Length::Fill`]).
    pub fn fill() -> Self {
        Self::Fill
    }

    /// Percentage of parent available space.
    pub fn percent(value: f32) -> Self {
        let fraction = if value.abs() <= 1.0 {
            value
        } else {
            value / 100.0
        };
        Self::Percent(fraction)
    }

    /// `true` when this axis uses intrinsic sizing.
    pub fn is_auto(self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Resolves to a concrete length given parent `available` space, or `None` for `Auto`.
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
