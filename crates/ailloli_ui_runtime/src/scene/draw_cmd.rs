use std::sync::Arc;

use ailloli_ui_core::{
    Border, BoxShadow, Color, IconId, Point, Radius, Rect, StrokeStyle, TextDecoration,
};
use ailloli_ui_text::PreparedTextLayout;

/// Filled axis-aligned rectangle draw primitive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRect {
    pub rect: Rect,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRRect {
    pub rect: Rect,
    /// Corner radius in logical pixels (clamped by the renderer).
    pub radius: f32,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawBorder {
    pub rect: Rect,
    pub radius: Radius,
    pub border: Border,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawBoxShadow {
    pub rect: Rect,
    pub radius: Radius,
    pub shadow: BoxShadow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRingProgress {
    pub rect: Rect,
    pub thickness: f32,
    pub fraction: f32,
    pub track_color: Color,
    pub fill_color: Color,
    /// Radians, with `-FRAC_PI_2` meaning top-center start.
    pub start_angle: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawPolyline {
    pub points: Vec<Point>,
    pub stroke: StrokeStyle,
}

#[derive(Debug, Clone)]
pub struct DrawText {
    pub pos: [f32; 2],
    pub color: Color,
    pub decoration: TextDecoration,
    pub layout: Arc<PreparedTextLayout>,
}

impl DrawText {
    /// Logical underline rectangles for all visual lines.
    pub fn decoration_rects(&self, dpr: f32) -> Vec<Rect> {
        if self.decoration != TextDecoration::Underline {
            return Vec::new();
        }
        let dpr = dpr.max(0.01);
        let first_baseline = self
            .layout
            .lines
            .first()
            .map(|line| line.baseline_y)
            .unwrap_or(0.0);
        let origin_y = self.pos[1] - first_baseline;
        self.layout
            .lines
            .iter()
            .filter(|line| line.width > 0.0)
            .map(|line| {
                let thickness = (line.descent / 3.0).max(1.0 / dpr);
                let y = origin_y + line.baseline_y + (line.descent * 0.5).max(thickness * 0.5);
                let snapped_y = (y * dpr).round() / dpr;
                Rect::new(self.pos[0], snapped_y, line.width, thickness)
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DrawImage {
    pub rect: Rect,
    pub icon: IconId,
    pub tint: Color,
    pub rotation_rad: f32,
}

/// Single paint primitive emitted during the paint phase.
#[derive(Debug, Clone)]
pub enum DrawCmd {
    /// Solid rectangle.
    Rect(DrawRect),
    /// Rounded rectangle.
    RRect(DrawRRect),
    /// Layout-aware box border.
    Border(DrawBorder),
    /// Paint-only box shadow.
    BoxShadow(DrawBoxShadow),
    /// Circular progress ring with a complete track and fractional fill.
    RingProgress(DrawRingProgress),
    /// Stroked polyline.
    Polyline(DrawPolyline),
    /// Shaped text at a baseline position.
    Text(DrawText),
    /// Icon or image in a rect.
    Image(DrawImage),
}
