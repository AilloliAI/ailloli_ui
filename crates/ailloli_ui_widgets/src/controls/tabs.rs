use ailloli_ui_core::{Color, FontId, IconId, Rect, TextStyle};
use ailloli_ui_runtime::{DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

pub const TABS_BAR_H: f32 = 36.0;

pub trait TabsItem {
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    fn selected(&self) -> bool;

    fn leading_icon(&self) -> Option<&IconId> {
        None
    }

    fn leading_icon_tint(&self) -> Option<Color> {
        None
    }

    /// Scope color strip (empty = neutral gray). Disable with `TabsBarOptions::show_scope_strip`.
    fn scope_kind(&self) -> &str {
        ""
    }

    fn unread(&self) -> bool {
        false
    }

    fn processing(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TabsBarOptions {
    pub show_trailing_actions: bool,
    pub show_tab_close_affordance: bool,
    pub show_scope_strip: bool,
}

impl Default for TabsBarOptions {
    fn default() -> Self {
        Self {
            show_trailing_actions: true,
            show_tab_close_affordance: true,
            show_scope_strip: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TabsStyle {
    pub bar_bg: Color,
    pub tab_bg: Color,
    pub tab_bg_selected: Color,
    pub tab_border: Color,
    pub tab_border_selected: Color,
    pub text_fg: Color,
    pub text_muted: Color,
    pub unread_dot: Color,
}

impl Default for TabsStyle {
    fn default() -> Self {
        Self {
            bar_bg: Color::rgba(11, 16, 32, 1.0),
            tab_bg: Color::rgba(17, 24, 40, 1.0),
            tab_bg_selected: Color::rgba(31, 38, 55, 1.0),
            tab_border: Color::rgba(31, 38, 55, 1.0),
            tab_border_selected: Color::rgba(37, 99, 235, 1.0),
            text_fg: Color::rgba(243, 246, 251, 1.0),
            text_muted: Color::rgba(156, 163, 175, 1.0),
            unread_dot: Color::rgba(245, 158, 11, 1.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TabsLayout {
    pub tab_rects: Vec<(String, Rect, Rect)>,
    pub controls: TabsControls,
}

#[derive(Debug, Clone, Copy)]
pub struct TabsControls {
    pub new_tab_rect: Rect,
    pub history_rect: Rect,
    pub can_create_path_tab: bool,
}

pub fn draw_tabs_bar<T: TabsItem>(
    rect: Rect,
    tabs: &[T],
    can_create_path_tab: bool,
    style: TabsStyle,
    text: &mut TextSystem,
) -> (Vec<DrawCmd>, TabsLayout) {
    draw_tabs_bar_with_options(
        rect,
        tabs,
        can_create_path_tab,
        style,
        text,
        TabsBarOptions::default(),
    )
}

pub fn draw_tabs_bar_with_options<T: TabsItem>(
    rect: Rect,
    tabs: &[T],
    can_create_path_tab: bool,
    style: TabsStyle,
    text: &mut TextSystem,
    options: TabsBarOptions,
) -> (Vec<DrawCmd>, TabsLayout) {
    let mut out = Vec::new();

    out.push(DrawCmd::Rect(DrawRect {
        rect,
        color: style.bar_bg,
    }));

    let pad_x = 8.0;
    let pad_y = 4.0;
    let btn = 28.0;
    let gap = 6.0;
    let mut x = rect.x + pad_x;
    let y = rect.y + pad_y;
    let h = rect.h - pad_y * 2.0;

    let right_controls_w = if options.show_trailing_actions {
        btn * 2.0 + gap
    } else {
        0.0
    };
    let max_tabs_w = (rect.w - pad_x * 2.0 - right_controls_w).max(0.0);
    let x_end = x + max_tabs_w;

    let mut tab_rects = Vec::new();
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

    for tab in tabs {
        if x + 120.0 > x_end {
            break;
        }
        let w = 220.0_f32.min((x_end - x).max(120.0));
        let tab_r = Rect::new(x, y, w, h);
        tab_rects.push((
            tab.id().to_string(),
            tab_r,
            Rect::new(tab_r.x + tab_r.w - 22.0, tab_r.y, 22.0, tab_r.h),
        ));

        let bg = if tab.selected() {
            style.tab_bg_selected
        } else {
            style.tab_bg
        };
        let border = if tab.selected() {
            style.tab_border_selected
        } else {
            style.tab_border
        };

        out.push(DrawCmd::RRect(DrawRRect {
            rect: tab_r,
            radius: 6.0,
            color: bg,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x, tab_r.y, tab_r.w, 1.0),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x, tab_r.y + tab_r.h - 1.0, tab_r.w, 1.0),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x, tab_r.y, 1.0, tab_r.h),
            color: border,
        }));
        out.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(tab_r.x + tab_r.w - 1.0, tab_r.y, 1.0, tab_r.h),
            color: border,
        }));

        if options.show_scope_strip {
            out.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(tab_r.x + 6.0, tab_r.y + 4.0, 6.0, tab_r.h - 8.0),
                radius: 3.0,
                color: scope_color(tab.scope_kind()),
            }));
        }

        let title = if tab.title().is_empty() {
            "Conversation"
        } else {
            tab.title()
        };
        let title_x = if options.show_scope_strip {
            tab_r.x + 18.0
        } else {
            tab_r.x + 8.0
        };
        let title_x = if let Some(icon) = tab.leading_icon() {
            let icon_size = 14.0;
            out.push(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    title_x,
                    tab_r.y + (tab_r.h - icon_size) * 0.5,
                    icon_size,
                    icon_size,
                ),
                icon: icon.clone(),
                tint: tab.leading_icon_tint().unwrap_or(style.text_muted),
                rotation_rad: 0.0,
            }));
            title_x + icon_size + 6.0
        } else {
            title_x
        };
        push_text(
            &mut out,
            text,
            [title_x, tab_r.y + 18.0],
            style.text_fg,
            FontId::Ui,
            12,
            title,
        );

        if tab.unread() || tab.processing() {
            out.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(
                    tab_r.x + tab_r.w - 34.0,
                    tab_r.y + (tab_r.h - 8.0) / 2.0,
                    8.0,
                    8.0,
                ),
                radius: 4.0,
                color: style.unread_dot,
            }));
        }

        if options.show_tab_close_affordance {
            push_text(
                &mut out,
                text,
                [tab_r.x + tab_r.w - 16.0, tab_r.y + 18.0],
                style.text_muted,
                FontId::Ui,
                14,
                "x",
            );
        }

        x += w + gap;
    }

    let (history_rect, new_tab_rect) = if options.show_trailing_actions {
        let history_rect = Rect::new(rect.x + rect.w - pad_x - btn, y, btn, btn);
        let new_tab_rect = Rect::new(history_rect.x - gap - btn, y, btn, btn);
        (history_rect, new_tab_rect)
    } else {
        let empty = Rect::new(0.0, 0.0, 0.0, 0.0);
        (empty, empty)
    };

    let controls = TabsControls {
        new_tab_rect,
        history_rect,
        can_create_path_tab,
    };

    if options.show_trailing_actions {
        out.push(DrawCmd::RRect(DrawRRect {
            rect: new_tab_rect,
            radius: 8.0,
            color: style.tab_bg,
        }));
        out.push(DrawCmd::Image(DrawImage {
            rect: Rect::new(new_tab_rect.x + 6.0, new_tab_rect.y + 6.0, 16.0, 16.0),
            icon: IconId::Plus,
            tint: style.text_fg,
            rotation_rad: 0.0,
        }));

        out.push(DrawCmd::RRect(DrawRRect {
            rect: history_rect,
            radius: 8.0,
            color: style.tab_bg,
        }));
        out.push(DrawCmd::Image(DrawImage {
            rect: Rect::new(history_rect.x + 6.0, history_rect.y + 6.0, 16.0, 16.0),
            icon: IconId::History,
            tint: style.text_fg,
            rotation_rad: 0.0,
        }));
    }

    (
        out,
        TabsLayout {
            tab_rects,
            controls,
        },
    )
}

fn scope_color(kind: &str) -> Color {
    match kind {
        "app_global" => Color::rgba(139, 92, 246, 1.0),
        "workspace_global" => Color::rgba(37, 99, 235, 1.0),
        "server" => Color::rgba(249, 115, 38, 1.0),
        "path" => Color::rgba(16, 185, 129, 1.0),
        "file" => Color::rgba(245, 158, 11, 1.0),
        "task" => Color::rgba(236, 60, 153, 1.0),
        _ => Color::rgba(75, 85, 102, 1.0),
    }
}
