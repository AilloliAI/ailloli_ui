//! Immediate-mode checkbox drawing primitives.

use ailloli_ui_core::style::{Border, Radius};
use ailloli_ui_core::{Color, FontId, IconId, Rect, TextStyle, Theme};
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::primitives::draw_icon;

#[derive(Debug, Clone, Copy)]
/// Colors, typography, and logical-pixel metrics used by [`draw_checkbox`].
///
/// The style is a value-level drawing configuration. It does not store checked,
/// disabled, hover, or focus state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::CheckboxStyle;
/// let style = CheckboxStyle::from_theme(Theme::dark());
/// assert_eq!(style.box_size, 18.0);
/// assert_eq!(style.label_px, 13);
/// ```
pub struct CheckboxStyle {
    /// Unchecked box fill color.
    pub box_bg: Color,
    /// Checked box fill color.
    pub checked_bg: Color,
    /// Unchecked box border color.
    pub box_border: Color,
    /// Checked box border color.
    pub checked_border: Color,
    /// Box corner radius in logical pixels.
    pub box_radius: f32,
    /// Width and height of the square box in logical pixels.
    pub box_size: f32,
    /// Horizontal gap between the box and label in logical pixels.
    pub gap: f32,
    /// Label text color.
    pub label_color: Color,
    /// Label font size in logical pixels.
    pub label_px: u16,
    /// Checkmark icon color.
    pub icon_tint: Color,
}

impl Default for CheckboxStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl CheckboxStyle {
    /// Resolves a checkbox style from `theme`'s current palette.
    ///
    /// The method uses fixed logical-pixel defaults for geometry and typography;
    /// only colors are derived from the theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::CheckboxStyle;
    /// let style = CheckboxStyle::from_theme(Theme::dark());
    /// assert_eq!(style.box_radius, 4.0);
    /// assert_eq!(style.gap, 8.0);
    /// ```
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            box_bg: palette.surface,
            checked_bg: palette.accent,
            box_border: palette.border,
            checked_border: palette.accent,
            box_radius: 4.0,
            box_size: 18.0,
            gap: 8.0,
            label_color: palette.text,
            label_px: 13,
            icon_tint: palette.text,
        }
    }
}

/// Produces draw commands for a checkbox row without retaining interaction state.
///
/// The square is vertically centered at `row_rect.x`. A checked and enabled row
/// includes a checkmark; a checked but disabled row deliberately omits it.
/// Disabled state multiplies the box fill and border alpha by `0.45`. `None`
/// draws no label, whereas `Some("")` still emits an empty text command.
/// Geometry values are not clamped, and content may extend outside `row_rect`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::controls::{draw_checkbox, CheckboxStyle};
/// let mut text = TextSystem::new();
/// let commands = draw_checkbox(
///     Rect::new(0.0, 0.0, 120.0, 24.0),
///     true,
///     Some("Ready"),
///     false,
///     CheckboxStyle::default(),
///     &mut text,
/// );
/// assert_eq!(commands.len(), 4); // fill, border, checkmark, and label
/// ```
pub fn draw_checkbox(
    row_rect: Rect,
    checked: bool,
    label: Option<&str>,
    disabled: bool,
    style: CheckboxStyle,
    text_system: &mut TextSystem,
) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    let alpha = if disabled { 0.45 } else { 1.0 };
    let bg_src = if checked {
        style.checked_bg
    } else {
        style.box_bg
    };
    let border_src = if checked {
        style.checked_border
    } else {
        style.box_border
    };
    let box_bg = Color::new(bg_src.r, bg_src.g, bg_src.b, bg_src.a * alpha);
    let border = Color::new(
        border_src.r,
        border_src.g,
        border_src.b,
        border_src.a * alpha,
    );
    let box_r = Rect::new(
        row_rect.x,
        row_rect.y + (row_rect.h - style.box_size) / 2.0,
        style.box_size,
        style.box_size,
    );
    out.push(DrawCmd::RRect(DrawRRect {
        rect: box_r,
        radius: style.box_radius,
        color: box_bg,
    }));
    out.push(DrawCmd::Border(DrawBorder {
        rect: box_r,
        radius: Radius::uniform(style.box_radius),
        border: Border::new(1.0, border),
    }));

    if checked && !disabled {
        let inset = 3.0;
        out.push(draw_icon(
            Rect::new(
                box_r.x + inset,
                box_r.y + inset,
                box_r.w - inset * 2.0,
                box_r.h - inset * 2.0,
            ),
            IconId::Check,
            style.icon_tint,
        ));
    }

    if let Some(lbl) = label {
        let lx = box_r.x + box_r.w + style.gap;
        let baseline_y = row_rect.y + row_rect.h / 2.0 + 4.0;
        out.push(DrawCmd::Text(DrawText {
            pos: [lx, baseline_y],
            color: style.label_color,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text_system.layout_cached(TextLayoutParams {
                text: lbl,
                style: TextStyle::new(FontId::Ui, style.label_px, style.label_color),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));
    }

    out
}
