//! Visual and layout styling types (no paint implementation).
//!
//! Widgets combine layout/flex/box styles with [`Color`] and [`Theme`] tokens.
//! Layout resolution: `ResolvedLayout`, `resolve_widget_size` (see `layout_resolve` module).

pub mod background;
pub mod border;
pub mod box_model;
pub mod box_style;
pub mod color;
pub mod cursor;
pub mod flex_item_style;
pub mod flex_style;
pub mod interaction;
pub mod layout_resolve;
pub mod layout_size_hint;
pub mod layout_style;
pub mod length;
pub mod opacity;
pub mod radius;
pub mod shadow;
pub mod stroke;
pub mod text_style;
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
