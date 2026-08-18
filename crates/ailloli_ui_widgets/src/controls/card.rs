use ailloli_ui_core::style::{Border, BoxShadow, FlexItemStyle, LayoutStyle, Radius};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{IntoView, View};

use crate::layout::Container;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CardVariant {
    #[default]
    Surface,
    Elevated,
    Outline,
    Accent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CardStyle {
    pub background: Color,
    pub border: Border,
    pub radius: Radius,
    pub shadows: Vec<BoxShadow>,
    pub padding: f32,
}

impl Default for CardStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), CardVariant::Surface)
    }
}

impl CardStyle {
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

pub struct Card<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    style: CardStyle,
    child: Option<View<A>>,
}

crate::impl_layout_builders!(Card);

impl<A: 'static> Default for Card<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Card<A> {
    pub fn new() -> Self {
        let style = CardStyle::default();
        Self {
            layout: LayoutStyle::default().padding(style.padding),
            flex_item: FlexItemStyle::default(),
            style,
            child: None,
        }
    }

    pub fn surface() -> Self {
        Self::new().variant(CardVariant::Surface)
    }

    pub fn elevated() -> Self {
        Self::new().variant(CardVariant::Elevated)
    }

    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.style = CardStyle::from_theme(Theme::default(), variant);
        self.layout = self.layout.padding(self.style.padding);
        self
    }

    pub fn card_style(mut self, style: CardStyle) -> Self {
        self.layout = self.layout.padding(style.padding);
        self.style = style;
        self
    }

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
