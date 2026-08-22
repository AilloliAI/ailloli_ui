//! Theme-aware containers for grouping a single child as a card.

use ailloli_ui_core::style::{Border, BoxShadow, FlexItemStyle, LayoutStyle, Radius};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{IntoView, View};

use crate::layout::Container;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Built-in visual treatments for a [`Card`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::CardVariant;
/// assert_eq!(CardVariant::default(), CardVariant::Surface);
/// ```
pub enum CardVariant {
    /// Opaque panel surface with a border and no shadow.
    #[default]
    Surface,
    /// Elevated surface with the theme's medium shadow.
    Elevated,
    /// Transparent surface with a standard border.
    Outline,
    /// Accent-tinted surface, border, and glow.
    Accent,
}

#[derive(Clone, Debug, PartialEq)]
/// Fully resolved visual style for a [`Card`].
///
/// Dimensions are logical pixels. Only the top border width/color and top-left
/// radius are currently forwarded to the underlying uniform container; callers
/// should therefore use uniform borders and radii for predictable rendering.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{CardStyle, CardVariant};
/// let style = CardStyle::from_theme(Theme::dark(), CardVariant::Elevated);
/// assert_eq!(style.shadows.len(), 1);
/// assert!(style.padding >= 0.0);
/// ```
pub struct CardStyle {
    /// Card fill color.
    pub background: Color,
    /// Card border; use uniform widths and colors.
    pub border: Border,
    /// Card corner radii; use a uniform value for current rendering.
    pub radius: Radius,
    /// Shadows painted in vector order by the backing container.
    pub shadows: Vec<BoxShadow>,
    /// Inner padding on every edge, in logical pixels.
    pub padding: f32,
}

impl Default for CardStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), CardVariant::Surface)
    }
}

impl CardStyle {
    /// Resolves `variant` against `theme` into concrete colors and metrics.
    ///
    /// `Surface` and `Outline` have no shadows, `Elevated` has one medium
    /// shadow, and `Accent` has one accent-colored glow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, Theme};
    /// use ailloli_ui_widgets::controls::{CardStyle, CardVariant};
    /// let style = CardStyle::from_theme(Theme::dark(), CardVariant::Outline);
    /// assert_eq!(style.background, Color::TRANSPARENT);
    /// assert!(style.shadows.is_empty());
    /// ```
    pub fn from_theme(theme: Theme, variant: CardVariant) -> Self {
        let palette = theme.palette();
        let radius = theme.radius().panel();
        match variant {
            CardVariant::Surface => Self {
                background: palette.surface,
                border: Border::new(1.0, palette.border),
                radius,
                shadows: Vec::new(),
                padding: theme.spacing().md,
            },
            CardVariant::Elevated => Self {
                background: palette.surface_elevated,
                border: Border::new(1.0, palette.border),
                radius,
                shadows: vec![theme.shadows().md],
                padding: theme.spacing().md,
            },
            CardVariant::Outline => Self {
                background: Color::TRANSPARENT,
                border: Border::new(1.0, palette.border),
                radius,
                shadows: Vec::new(),
                padding: theme.spacing().md,
            },
            CardVariant::Accent => Self {
                background: palette.accent.with_alpha(0.12),
                border: Border::new(1.0, palette.accent.with_alpha(0.55)),
                radius,
                shadows: vec![BoxShadow::glow(palette.accent.with_alpha(0.22))],
                padding: theme.spacing().md,
            },
        }
    }
}

/// A themed, single-child panel with configurable layout.
///
/// Adding a second child replaces the first. The generic parameter is the
/// action type emitted by descendants.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Card;
/// let card: Card<()> = Card::new();
/// let _ = card;
/// ```
pub struct Card<A = ()> {
    /// Layout applied to the backing container.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior applied when this card is a child.
    pub(crate) flex_item: FlexItemStyle,
    /// Resolved paint and padding configuration.
    style: CardStyle,
    /// The optional sole child; a later [`Card::child`] call replaces it.
    child: Option<View<A>>,
}

crate::impl_layout_builders!(Card);

impl<A: 'static> Default for Card<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Card<A> {
    /// Creates an empty surface card using the default theme.
    ///
    /// Its layout padding is initialized from [`CardStyle::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Card;
    /// let card: Card<()> = Card::new();
    /// let _ = card;
    /// ```
    pub fn new() -> Self {
        let style = CardStyle::default();
        Self {
            layout: LayoutStyle::default().padding(style.padding),
            flex_item: FlexItemStyle::default(),
            style,
            child: None,
        }
    }

    /// Creates an empty [`CardVariant::Surface`] card.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Card;
    /// let card: Card<()> = Card::surface();
    /// let _ = card;
    /// ```
    pub fn surface() -> Self {
        Self::new().variant(CardVariant::Surface)
    }

    /// Creates an empty [`CardVariant::Elevated`] card.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Card;
    /// let card: Card<()> = Card::elevated();
    /// let _ = card;
    /// ```
    pub fn elevated() -> Self {
        Self::new().variant(CardVariant::Elevated)
    }

    /// Replaces the visual style with the default-theme form of `variant`.
    ///
    /// This also replaces all four layout padding edges with the variant's
    /// resolved padding. Call layout padding builders after this method when
    /// an override is required.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Card, CardVariant};
    /// let card: Card<()> = Card::new().variant(CardVariant::Accent);
    /// let _ = card;
    /// ```
    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.style = CardStyle::from_theme(Theme::default(), variant);
        self.layout = self.layout.padding(self.style.padding);
        self
    }

    /// Replaces the resolved card style and synchronizes layout padding.
    ///
    /// Style values, including negative padding, are accepted as-is.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{Card, CardStyle, CardVariant};
    /// let style = CardStyle::from_theme(Theme::dark(), CardVariant::Outline);
    /// let card: Card<()> = Card::new().card_style(style);
    /// let _ = card;
    /// ```
    pub fn card_style(mut self, style: CardStyle) -> Self {
        self.layout = self.layout.padding(style.padding);
        self.style = style;
        self
    }

    /// Sets the card's sole child, replacing any child set earlier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Card;
    /// use ailloli_ui_widgets::text::Text;
    /// let card = Card::<()>::new().child(Text::new("Details"));
    /// let _ = card;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

impl<A: 'static> IntoView<A> for Card<A> {
    fn into_view(self) -> View<A> {
        let mut container = Container::<A>::new()
            .background(self.style.background)
            .radius(self.style.radius.tl)
            .border(self.style.border.widths.top, self.style.border.colors.top);
        container.layout = self.layout;
        container.flex_item = self.flex_item;
        for shadow in self.style.shadows {
            container = container.shadow(shadow);
        }
        if let Some(child) = self.child {
            container = container.child(child);
        }
        container.into_view()
    }
}
