//! Immediate-mode panel surface and horizontal border rules.

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawRect};

#[derive(Debug, Clone, Copy)]
/// Fill, optional border, and corner radius for [`draw_panel`].
///
/// Dimensions are logical pixels. A radius greater than zero selects a rounded
/// command; zero, negative, or `NaN` selects an axis-aligned rectangle.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_widgets::layout::panel::PanelStyle;
/// let style = PanelStyle::simple(Color::BLACK);
/// assert!(style.border.is_none());
/// assert_eq!(style.radius, 0.0);
/// ```
pub struct PanelStyle {
    /// Panel fill.
    pub bg: Color,
    /// Optional color for one-pixel top and bottom rules only.
    pub border: Option<Color>,
    /// Corner radius in logical pixels.
    pub radius: f32,
}

impl PanelStyle {
    /// Creates a square, borderless panel style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::layout::panel::PanelStyle;
    /// let style = PanelStyle::simple(Color::WHITE);
    /// assert_eq!(style.bg, Color::WHITE);
    /// ```
    pub fn simple(bg: Color) -> Self {
        Self {
            bg,
            border: None,
            radius: 0.0,
        }
    }
}

/// Draws one panel fill and optional one-pixel top/bottom border rules.
///
/// Returns one command without a border and three with a border. Geometry is
/// passed through without normalization; vertical side borders are absent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_widgets::layout::panel::{draw_panel, PanelStyle};
/// let style = PanelStyle { bg: Color::BLACK, border: Some(Color::WHITE), radius: 4.0 };
/// assert_eq!(draw_panel(Rect::new(0.0, 0.0, 80.0, 40.0), style).len(), 3);
/// ```
pub fn draw_panel(rect: Rect, style: PanelStyle) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    if style.radius > 0.0 {
        out.push(DrawCmd::RRect(DrawRRect {
            rect,
            radius: style.radius,
            color: style.bg,
        }));
    } else {
        out.push(DrawCmd::Rect(DrawRect {
            rect,
            color: style.bg,
        }));
    }

    if let Some(border) = style.border {
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, rect.w, 1.0),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y + rect.h - 1.0, rect.w, 1.0),
            color: border,
        }));
    }

    out
}
