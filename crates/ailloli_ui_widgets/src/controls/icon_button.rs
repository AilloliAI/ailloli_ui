#![allow(deprecated)]

use ailloli_ui_core::{Color, IconId, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect};

use crate::primitives::draw_icon;

#[deprecated(note = "use Button::new().child(Icon::new(...)) instead")]
#[derive(Debug, Clone, Copy)]
pub struct IconButtonStyle {
    pub bg: Color,
    pub radius: f32,
    pub icon_size: f32,
    pub padding: f32,
}

impl Default for IconButtonStyle {
    fn default() -> Self {
        Self {
            bg: Color::rgba(31, 38, 55, 1.0),
            radius: 8.0,
            icon_size: 16.0,
            padding: 6.0,
        }
    }
}

#[deprecated(note = "use Button::new().child(Icon::new(...)) instead")]
pub fn draw_icon_button(
    rect: Rect,
    icon: IconId,
    tint: Color,
    style: IconButtonStyle,
) -> Vec<DrawCmd> {
    vec![
        DrawCmd::RRect(DrawRRect {
            rect,
            radius: style.radius,
            color: style.bg,
        }),
        draw_icon(
            Rect::new(
                rect.x + style.padding,
                rect.y + style.padding,
                style.icon_size,
                style.icon_size,
            ),
            icon,
            tint,
        ),
    ]
}
