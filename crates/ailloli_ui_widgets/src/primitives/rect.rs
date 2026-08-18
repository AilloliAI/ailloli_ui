use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{DrawCmd, DrawRect};

pub fn draw_rect(rect: Rect, color: Color) -> DrawCmd {
    DrawCmd::Rect(DrawRect { rect, color })
}
