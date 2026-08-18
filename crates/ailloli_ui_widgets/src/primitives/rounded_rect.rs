use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRRect};

pub fn draw_rounded_rect(rect: Rect, radius: f32, color: Color) -> DrawCmd {
    DrawCmd::RRect(DrawRRect {
        rect,
        radius,
        color,
    })
}
