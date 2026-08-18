use ailloli_ui_core::{Point, Rect};
use ailloli_ui_text::{TextEditState, WrapMode};

use crate::code::GutterConfig;
use crate::{EditorConfig, EditorStyle, EditorWrapMode};

/// Computes the text content viewport inside the editor bounds.
pub fn editor_content_rect(bounds: Rect, style: EditorStyle) -> Rect {
    Rect::new(
        bounds.x + style.pad,
        bounds.y + style.pad,
        (bounds.w - style.pad * 2.0).max(0.0),
        (bounds.h - style.pad * 2.0).max(0.0),
    )
}

/// Current editor viewport in widget/screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorViewport {
    pub bounds: Rect,
    pub content_rect: Rect,
    pub text_rect: Rect,
    pub gutter_rect: Option<Rect>,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub wrap_mode: EditorWrapMode,
}

impl EditorViewport {
    pub fn new(bounds: Rect, config: EditorConfig, edit: &TextEditState) -> Self {
        Self::with_gutter(bounds, config, edit, None)
    }

    pub fn with_gutter(
        bounds: Rect,
        config: EditorConfig,
        edit: &TextEditState,
        gutter: Option<GutterConfig>,
    ) -> Self {
        let scroll_x = match config.wrap_mode {
            EditorWrapMode::SoftWrap => 0.0,
            EditorWrapMode::NoWrap => edit.scroll_x.max(0.0),
        };
        let content_rect = editor_content_rect(bounds, config.style);
        let (gutter_rect, text_rect) = split_gutter(content_rect, gutter);
        Self {
            bounds,
            content_rect,
            text_rect,
            gutter_rect,
            scroll_x,
            scroll_y: edit.scroll_y.max(0.0),
            wrap_mode: config.wrap_mode,
        }
    }

    pub fn text_origin_x(self) -> f32 {
        self.text_rect.x - self.scroll_x
    }

    pub fn text_origin_y(self) -> f32 {
        self.text_rect.y
    }

    pub fn text_wrap_mode(self) -> WrapMode {
        match self.wrap_mode {
            EditorWrapMode::SoftWrap => WrapMode::WordOrAnywhere,
            EditorWrapMode::NoWrap => WrapMode::NoWrap,
        }
    }

    pub fn max_text_width(self) -> Option<f32> {
        match self.wrap_mode {
            EditorWrapMode::SoftWrap => Some(self.text_rect.w),
            EditorWrapMode::NoWrap => None,
        }
    }

    pub fn local_point(self, pos: Point) -> (f32, f32) {
        (
            (pos.x - self.text_rect.x + self.scroll_x).max(0.0),
            pos.y - self.text_rect.y,
        )
    }
}

fn split_gutter(content_rect: Rect, gutter: Option<GutterConfig>) -> (Option<Rect>, Rect) {
    let Some(gutter) = gutter.filter(|gutter| gutter.enabled && gutter.width > 0.0) else {
        return (None, content_rect);
    };
    let width = gutter.width.min(content_rect.w.max(0.0));
    let gutter_rect = Rect::new(content_rect.x, content_rect.y, width, content_rect.h);
    let text_rect = Rect::new(
        content_rect.x + width,
        content_rect.y,
        (content_rect.w - width).max(0.0),
        content_rect.h,
    );
    (Some(gutter_rect), text_rect)
}
