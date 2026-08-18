use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

// For correct z-order: merge returned `DrawCmd` into an overlay layer (see `scene_base_overlay`).
// (voir `crate::overlay::layered::scene_base_overlay`).

/// Generic confirmation modal dialog style.
///
/// Title and button labels are caller-defined; no app-specific copy in this crate.
#[derive(Debug, Clone, Copy)]
pub struct ConfirmDialogStyle {
    pub overlay: Color,
    pub panel_bg: Color,
    pub panel_border: Color,
    pub title_fg: Color,
    pub body_fg: Color,
    pub btn_bg: Color,
    pub btn_bg_primary: Color,
    pub btn_fg: Color,
}

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
pub struct ConfirmDialogLayout {
    pub panel: Rect,
    pub allow: Rect,
    pub deny: Rect,
}

#[derive(Debug, Clone, Copy)]
pub struct ConfirmDialogTexts<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub deny_label: &'a str,
    pub allow_label: &'a str,
}

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
