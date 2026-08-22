//! Immediate-mode drawing for a compact, non-interactive row list.
//!
//! Rows are clipped by count at the bottom edge; this helper does not implement
//! scrolling, hit testing, or retained virtualization.

use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone)]
/// Owned text and optional trailing decoration for one list row.
///
/// Empty strings are valid and still produce text commands. `right_icon_bg`
/// is a color for a trailing rounded square, not an icon identifier; `None`
/// omits that command.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_widgets::controls::list::ListRow;
/// let row = ListRow {
///     title: "Project".into(),
///     subtitle: "Updated now".into(),
///     right_icon_bg: Some(Color::WHITE),
/// };
/// assert_eq!(row.title, "Project");
/// ```
pub struct ListRow {
    /// Primary line of text.
    pub title: String,
    /// Secondary line of text.
    pub subtitle: String,
    /// Optional fill color for the `28 × 28` trailing rounded square.
    pub right_icon_bg: Option<Color>,
}

#[derive(Debug, Clone, Copy)]
/// Logical-pixel geometry, typography, and colors for [`draw_list_rows`].
///
/// Values are used as-is. Callers should provide a positive `row_h`; a zero or
/// negative height cannot make vertical progress through a large slice.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId};
/// use ailloli_ui_widgets::controls::list::ListStyle;
/// let style = ListStyle {
///     row_h: 44.0,
///     radius: 8.0,
///     title_fg: Color::WHITE,
///     subtitle_fg: Color::WHITE.with_alpha(0.7),
///     font: FontId::Ui,
///     title_px: 14,
///     subtitle_px: 12,
/// };
/// assert_eq!(style.row_h, 44.0);
/// ```
pub struct ListStyle {
    /// Height of each row in logical pixels.
    pub row_h: f32,
    /// Background corner radius in logical pixels.
    pub radius: f32,
    /// Primary text color.
    pub title_fg: Color,
    /// Secondary text color.
    pub subtitle_fg: Color,
    /// Font used for both text lines.
    pub font: FontId,
    /// Primary font size in logical pixels.
    pub title_px: u16,
    /// Secondary font size in logical pixels.
    pub subtitle_px: u16,
}

/// Draws complete rows from the top of `rect` until the next row would overflow.
///
/// A row whose bottom is exactly `rect`'s bottom is included. Each included row
/// emits a transparent background plus two text commands, and optionally a
/// trailing rounded square. Horizontal content is not clipped by this helper.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, Rect};
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::controls::list::{draw_list_rows, ListRow, ListStyle};
/// let rows = vec![ListRow {
///     title: "One".into(),
///     subtitle: "First".into(),
///     right_icon_bg: None,
/// }];
/// let style = ListStyle {
///     row_h: 44.0,
///     radius: 8.0,
///     title_fg: Color::WHITE,
///     subtitle_fg: Color::WHITE,
///     font: FontId::Ui,
///     title_px: 14,
///     subtitle_px: 12,
/// };
/// let mut text = TextSystem::new();
/// let commands = draw_list_rows(Rect::new(0.0, 0.0, 200.0, 44.0), &rows, style, &mut text);
/// assert_eq!(commands.len(), 3);
/// ```
pub fn draw_list_rows(
    rect: Rect,
    rows: &[ListRow],
    style: ListStyle,
    text: &mut TextSystem,
) -> Vec<DrawCmd> {
    let mut out = Vec::new();
    let mut y = rect.y;
    for r in rows {
        if y + style.row_h > rect.y + rect.h {
            break;
        }
        let row_rect = Rect::new(rect.x, y, rect.w, style.row_h);
        out.push(DrawCmd::RRect(DrawRRect {
            rect: row_rect,
            radius: style.radius,
            color: Color::new(0.0, 0.0, 0.0, 0.0),
        }));
        out.push(DrawCmd::Text(DrawText {
            pos: [row_rect.x + 14.0, row_rect.y + 18.0],
            color: style.title_fg,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text.layout_cached(TextLayoutParams {
                text: &r.title,
                style: TextStyle::new(style.font, style.title_px, style.title_fg),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));
        out.push(DrawCmd::Text(DrawText {
            pos: [row_rect.x + 14.0, row_rect.y + 34.0],
            color: style.subtitle_fg,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text.layout_cached(TextLayoutParams {
                text: &r.subtitle,
                style: TextStyle::new(style.font, style.subtitle_px, style.subtitle_fg),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));

        if let Some(bg) = r.right_icon_bg {
            let icon_rect = Rect::new(row_rect.x + row_rect.w - 34.0, row_rect.y + 8.0, 28.0, 28.0);
            out.push(DrawCmd::RRect(DrawRRect {
                rect: icon_rect,
                radius: 8.0,
                color: bg,
            }));
        }

        y += style.row_h;
    }
    out
}
