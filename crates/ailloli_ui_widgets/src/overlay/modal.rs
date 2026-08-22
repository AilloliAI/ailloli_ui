//! Immediate-mode fixed-size confirmation modal drawing.

use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

// For correct z-order: merge returned `DrawCmd` into an overlay layer (see `scene_base_overlay`).
// (voir `crate::overlay::layered::scene_base_overlay`).

/// Generic confirmation modal dialog style.
///
/// Title and button labels are caller-defined; no app-specific copy in this crate.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::overlay::modal::ConfirmDialogStyle;
/// let style = ConfirmDialogStyle::default();
/// assert!(style.overlay.a > 0.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ConfirmDialogStyle {
    /// Full-viewport scrim color.
    pub overlay: Color,
    /// Dialog panel fill.
    pub panel_bg: Color,
    /// One-logical-pixel top and bottom panel rule color.
    pub panel_border: Color,
    /// Title glyph color.
    pub title_fg: Color,
    /// Body glyph color.
    pub body_fg: Color,
    /// Deny-button fill.
    pub btn_bg: Color,
    /// Allow-button fill.
    pub btn_bg_primary: Color,
    /// Shared button-label glyph color.
    pub btn_fg: Color,
}

/// Supplies the dark fixed-confirmation palette.
impl Default for ConfirmDialogStyle {
    fn default() -> Self {
        Self {
            overlay: Color::rgba(0, 0, 0, 0.66),
            panel_bg: Color::rgba(17, 24, 40, 1.0),
            panel_border: Color::rgba(31, 38, 55, 1.0),
            title_fg: Color::rgba(243, 246, 251, 1.0),
            body_fg: Color::rgba(243, 246, 251, 1.0),
            btn_bg: Color::rgba(31, 38, 55, 1.0),
            btn_bg_primary: Color::rgba(37, 99, 235, 1.0),
            btn_fg: Color::rgba(243, 246, 251, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// Logical-pixel hit rectangles returned with confirmation draw commands.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_widgets::overlay::modal::ConfirmDialogLayout;
/// let layout = ConfirmDialogLayout { panel: Rect::new(0.0, 0.0, 520.0, 220.0), allow: Rect::new(0.0, 0.0, 84.0, 34.0), deny: Rect::new(0.0, 0.0, 84.0, 34.0) };
/// assert_eq!(layout.panel.w, 520.0);
/// ```
pub struct ConfirmDialogLayout {
    /// Centered fixed 520×220 panel rectangle.
    pub panel: Rect,
    /// Rightmost 84×34 allow-button hit rectangle.
    pub allow: Rect,
    /// 84×34 deny-button hit rectangle immediately left of allow.
    pub deny: Rect,
}

#[derive(Debug, Clone, Copy)]
/// Borrowed caller-owned copy for a confirmation dialog.
///
/// Empty strings are valid; labels are drawn without wrapping or truncation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::overlay::modal::ConfirmDialogTexts;
/// let texts = ConfirmDialogTexts { title: "Delete?", body: "This cannot be undone", deny_label: "Cancel", allow_label: "Delete" };
/// assert_eq!(texts.allow_label, "Delete");
/// ```
pub struct ConfirmDialogTexts<'a> {
    /// UI-font title.
    pub title: &'a str,
    /// Mono-font body line.
    pub body: &'a str,
    /// Deny-button label.
    pub deny_label: &'a str,
    /// Allow-button label.
    pub allow_label: &'a str,
}

/// Draws a fixed confirmation panel centered in `full`.
///
/// The panel remains 520×220 logical pixels even when `full` is smaller, so it
/// may extend outside the viewport. The function returns ten commands in paint
/// order (scrim, surfaces, then four text commands) plus button hit geometry.
/// Text is unwrapped and the function performs shaping allocations/cache access.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_text::TextSystem;
/// use ailloli_ui_widgets::overlay::modal::{draw_confirm_dialog, ConfirmDialogStyle, ConfirmDialogTexts};
/// let mut text_system = TextSystem::new();
/// let texts = ConfirmDialogTexts { title: "Continue?", body: "Review the action", deny_label: "No", allow_label: "Yes" };
/// let (commands, layout) = draw_confirm_dialog(Rect::new(0.0, 0.0, 800.0, 600.0), texts, ConfirmDialogStyle::default(), &mut text_system);
/// assert_eq!(commands.len(), 10);
/// assert_eq!((layout.panel.w, layout.panel.h), (520.0, 220.0));
/// ```
pub fn draw_confirm_dialog(
    full: Rect,
    texts: ConfirmDialogTexts<'_>,
    style: ConfirmDialogStyle,
    text: &mut TextSystem,
) -> (Vec<DrawCmd>, ConfirmDialogLayout) {
    let panel = Rect::new(
        full.x + (full.w - 520.0) / 2.0,
        full.y + (full.h - 220.0) / 2.0,
        520.0,
        220.0,
    );
    let deny = Rect::new(
        panel.x + panel.w - 190.0,
        panel.y + panel.h - 54.0,
        84.0,
        34.0,
    );
    let allow = Rect::new(
        panel.x + panel.w - 96.0,
        panel.y + panel.h - 54.0,
        84.0,
        34.0,
    );

    let mut out = Vec::new();
    /// Shapes one unwrapped label and appends its baseline-positioned command.
    fn push_text(
        out: &mut Vec<DrawCmd>,
        text: &mut TextSystem,
        pos: [f32; 2],
        color: Color,
        font: FontId,
        px_size: u16,
        s: &str,
    ) {
        out.push(DrawCmd::Text(DrawText {
            pos,
            color,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: text.layout_cached(TextLayoutParams {
                text: s,
                style: TextStyle::new(font, px_size, color),
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            }),
        }));
    }

    out.extend([
        DrawCmd::Rect(DrawRect {
            rect: full,
            color: style.overlay,
        }),
        DrawCmd::RRect(DrawRRect {
            rect: panel,
            radius: 10.0,
            color: style.panel_bg,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(panel.x, panel.y, panel.w, 1.0),
            color: style.panel_border,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(panel.x, panel.y + panel.h - 1.0, panel.w, 1.0),
            color: style.panel_border,
        }),
        DrawCmd::RRect(DrawRRect {
            rect: deny,
            radius: 8.0,
            color: style.btn_bg,
        }),
        DrawCmd::RRect(DrawRRect {
            rect: allow,
            radius: 8.0,
            color: style.btn_bg_primary,
        }),
    ]);

    push_text(
        &mut out,
        text,
        [panel.x + 16.0, panel.y + 30.0],
        style.title_fg,
        FontId::Ui,
        16,
        texts.title,
    );
    push_text(
        &mut out,
        text,
        [panel.x + 16.0, panel.y + 58.0],
        style.body_fg,
        FontId::Mono,
        13,
        texts.body,
    );
    push_text(
        &mut out,
        text,
        [deny.x + 12.0, deny.y + deny.h / 2.0 + 4.0],
        style.btn_fg,
        FontId::Ui,
        13,
        texts.deny_label,
    );
    push_text(
        &mut out,
        text,
        [allow.x + 12.0, allow.y + allow.h / 2.0 + 4.0],
        style.btn_fg,
        FontId::Ui,
        13,
        texts.allow_label,
    );

    (out, ConfirmDialogLayout { panel, allow, deny })
}
