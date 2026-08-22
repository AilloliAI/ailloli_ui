//! Visual and layout styling types (no paint implementation).
//!
//! Widgets combine layout/flex/box styles with [`Color`] and [`Theme`] tokens.
//! Layout resolution: `ResolvedLayout`, `resolve_widget_size` (see `layout_resolve` module).

/// Optional solid box backgrounds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::background::Background;
/// assert_eq!(Background::default(), Background::None);
/// ```
pub mod background;
/// Per-edge border appearance and inner geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::border::Border;
/// assert!(!Border::none().is_visible());
/// ```
pub mod border;
/// Composable margin and padding.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::box_model::BoxModel;
/// assert_eq!(BoxModel::new().padding.left, 0.0);
/// ```
pub mod box_model;
/// Background, border, radius, shadow, and opacity bundles.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::box_style::BoxStyle;
/// assert!(BoxStyle::new().shadows.is_empty());
/// ```
pub mod box_style;
/// Linear-RGBA colors and sRGB conversion.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::color::Color;
/// assert_eq!(Color::WHITE.as_rgba8(), (255, 255, 255, 255));
/// ```
pub mod color;
/// Platform cursor hints.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::cursor::CursorStyle;
/// assert_eq!(CursorStyle::default(), CursorStyle::Auto);
/// ```
pub mod cursor;
/// Per-child flex behavior.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::flex_item_style::FlexItemStyle;
/// assert_eq!(FlexItemStyle::new().flex_grow, 0.0);
/// ```
pub mod flex_item_style;
/// Flex-container direction, alignment, and distribution.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::flex_style::FlexStyle;
/// assert_eq!(FlexStyle::row().gap, 0.0);
/// ```
pub mod flex_style;
/// Interaction flags and state-style resolution.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::interaction::InteractionState;
/// assert!(!InteractionState::normal().disabled);
/// ```
pub mod interaction;
/// Declarative-length resolution against constraints.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Constraints;
/// use ailloli_ui_core::style::layout_resolve::resolve_widget_size;
/// use ailloli_ui_core::style::LayoutStyle;
/// assert_eq!(resolve_widget_size(ailloli_ui_core::Size::new(1.0, 2.0), LayoutStyle::new(), Constraints::loose(10.0, 10.0)).w, 1.0);
/// ```
pub mod layout_resolve;
/// Compact width/height hints for parent layouts.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::layout_size_hint::LayoutSizeHint;
/// assert_eq!(LayoutSizeHint::default().width, ailloli_ui_core::style::Length::Auto);
/// ```
pub mod layout_size_hint;
/// Per-widget dimensions, bounds, margin, and padding.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::layout_style::LayoutStyle;
/// assert_eq!(LayoutStyle::new().margin.left, 0.0);
/// ```
pub mod layout_style;
/// Intrinsic, fixed, fill, and percentage lengths.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::length::Length;
/// assert_eq!(Length::px(10.0).resolve(20.0), Some(10.0));
/// ```
pub mod length;
/// Clamped widget opacity multipliers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::opacity::Opacity;
/// assert_eq!(Opacity::new(2.0), Opacity(1.0));
/// ```
pub mod opacity;
/// Per-corner box radii.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::radius::Radius;
/// assert_eq!(Radius::uniform(3.0).tl, 3.0);
/// ```
pub mod radius;
/// Outer and inset box-shadow geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::shadow::BoxShadow;
/// assert_eq!(BoxShadow::sm().blur_radius, 2.0);
/// ```
pub mod shadow;
/// Line cap, join, and stroke parameters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::stroke::StrokeStyle;
/// assert_eq!(StrokeStyle::default().width, 1.0);
/// ```
pub mod stroke;
/// Text font, size, color, and decoration.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// use ailloli_ui_core::style::text_style::TextStyle;
/// assert_eq!(TextStyle::new(FontId::Ui, 14, ailloli_ui_core::Color::WHITE).px_size, 14);
/// ```
pub mod text_style;
/// Built-in semantic design tokens.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::theme::Theme;
/// assert_eq!(Theme::dark().spacing().md, 12.0);
/// ```
pub mod theme;

pub use background::Background;
pub use border::{Border, BorderStyle, EdgeColors};
pub use box_model::BoxModel;
pub use box_style::BoxStyle;
#[allow(deprecated)]
pub use color::Rgba;
pub use color::{Color, ColorParseError};
pub use cursor::CursorStyle;
pub use flex_item_style::FlexItemStyle;
pub use flex_style::{AlignItems, FlexDirection, FlexStyle, JustifyContent};
pub use interaction::{InteractionState, StateStyle};
pub use layout_resolve::{resolve_widget_size, ResolvedLayout};
pub use layout_size_hint::LayoutSizeHint;
pub use layout_style::LayoutStyle;
pub use length::Length;
pub use opacity::Opacity;
pub use radius::Radius;
pub use shadow::BoxShadow;
#[allow(deprecated)]
pub use shadow::Shadow;
pub use stroke::{LineCap, LineJoin, StrokeStyle};
pub use text_style::{TextDecoration, TextStyle};
pub use theme::{
    Theme, ThemePalette, ThemeRadius, ThemeShadows, ThemeSpacing, ThemeState, ThemeTypography,
};
