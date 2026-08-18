use ailloli_ui_core::{Color, IconId, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawImage};

pub fn draw_icon(rect: Rect, icon: IconId, tint: Color) -> DrawCmd {
    DrawCmd::Image(DrawImage {
        rect,
        icon,
        tint,
        rotation_rad: 0.0,
    })
}
