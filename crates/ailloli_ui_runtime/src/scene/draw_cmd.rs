//! Renderer-neutral drawing commands emitted by retained views.

use std::sync::Arc;

use ailloli_ui_core::{
    Border, BoxShadow, Color, IconId, Point, Radius, Rect, StrokeStyle, TextDecoration,
};
use ailloli_ui_text::PreparedTextLayout;

/// Filled axis-aligned rectangle draw primitive.
///
/// Coordinates are window-space logical pixels; color is linear RGBA.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_runtime::scene::DrawRect;
/// let draw = DrawRect { rect: Rect::new(1.0, 2.0, 30.0, 10.0), color: Color::WHITE };
/// assert_eq!(draw.rect.w, 30.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRect {
    /// Filled rectangle in logical pixels.
    pub rect: Rect,
    /// Linear-RGBA fill color.
    pub color: Color,
}

/// Filled rounded-rectangle draw primitive.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_runtime::scene::DrawRRect;
/// let draw = DrawRRect { rect: Rect::new(0.0, 0.0, 20.0, 10.0), radius: 4.0, color: Color::WHITE };
/// assert_eq!(draw.radius, 4.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRRect {
    /// Filled rectangle in window-space logical pixels.
    pub rect: Rect,
    /// Corner radius in logical pixels (clamped by the renderer).
    pub radius: f32,
    /// Linear-RGBA fill color.
    pub color: Color,
}

/// Layout-aware border drawn around a rounded rectangle.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Border, Color, Radius, Rect};
/// use ailloli_ui_runtime::scene::DrawBorder;
/// let draw = DrawBorder {
///     rect: Rect::new(0.0, 0.0, 40.0, 20.0),
///     radius: Radius::uniform(3.0),
///     border: Border::new(1.0, Color::WHITE),
/// };
/// assert_eq!(draw.border.widths.left, 1.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawBorder {
    /// Border box in window-space logical pixels.
    pub rect: Rect,
    /// Per-corner radii in logical pixels.
    pub radius: Radius,
    /// Per-edge widths, colors, and stroke style.
    pub border: Border,
}

/// Paint-only shadow associated with a rounded source rectangle.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{BoxShadow, Radius, Rect};
/// use ailloli_ui_runtime::scene::DrawBoxShadow;
/// let draw = DrawBoxShadow {
///     rect: Rect::new(0.0, 0.0, 40.0, 20.0),
///     radius: Radius::uniform(2.0),
///     shadow: BoxShadow::sm(),
/// };
/// assert_eq!(draw.shadow.blur_radius, 2.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawBoxShadow {
    /// Source box in window-space logical pixels.
    pub rect: Rect,
    /// Source-box corner radii in logical pixels.
    pub radius: Radius,
    /// Shadow offset, blur, spread, color, and inset flag.
    pub shadow: BoxShadow,
}

/// Circular progress-ring primitive.
///
/// Numeric inputs are stored verbatim. The conventional fraction range is
/// `0.0..=1.0`, but this type does not clamp it or reject negative/non-finite
/// thicknesses and angles.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_runtime::scene::DrawRingProgress;
/// let ring = DrawRingProgress {
///     rect: Rect::new(0.0, 0.0, 24.0, 24.0), thickness: 2.0, fraction: 0.5,
///     track_color: Color::BLACK, fill_color: Color::WHITE,
///     start_angle: -std::f32::consts::FRAC_PI_2,
/// };
/// assert_eq!(ring.fraction, 0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRingProgress {
    /// Outer ring rectangle in window-space logical pixels.
    pub rect: Rect,
    /// Requested stroke thickness in logical pixels.
    pub thickness: f32,
    /// Requested filled revolution fraction; conventionally `0.0..=1.0`.
    pub fraction: f32,
    /// Linear-RGBA color of the complete background track.
    pub track_color: Color,
    /// Linear-RGBA color of the fractional foreground arc.
    pub fill_color: Color,
    /// Radians, with `-FRAC_PI_2` meaning top-center start.
    pub start_angle: f32,
}

/// Connected line segments rendered with one stroke style.
///
/// Zero or one point cannot form a segment but is accepted; allocation size is
/// proportional to the caller-provided point count.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Point, StrokeStyle};
/// use ailloli_ui_runtime::scene::DrawPolyline;
/// let line = DrawPolyline {
///     points: vec![Point::new(0.0, 0.0), Point::new(10.0, 5.0)],
///     stroke: StrokeStyle::new(2.0, Color::WHITE),
/// };
/// assert_eq!(line.points.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct DrawPolyline {
    /// Vertices in window-space logical pixels, in traversal order.
    pub points: Vec<Point>,
    /// Width, color, cap, join, and miter settings for every segment.
    pub stroke: StrokeStyle,
}

/// Prepared shaped text positioned by its first baseline.
///
/// The shared layout keeps source text, glyphs, and line metrics alive. Color
/// is the uniform paint color; styled layouts may additionally carry per-glyph
/// colors. All positions and metrics are logical pixels.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use ailloli_ui_core::{Color, FontId, TextDecoration, TextStyle};
/// use ailloli_ui_runtime::scene::DrawText;
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new(
///     "hello", TextStyle::new(FontId::Ui, 14, Color::WHITE),
/// ));
/// let draw = DrawText { pos: [5.0, 20.0], color: Color::WHITE,
///     decoration: TextDecoration::None, layout };
/// assert_eq!(draw.layout.text(), "hello");
/// ```
#[derive(Debug, Clone)]
pub struct DrawText {
    /// `[x, first_baseline_y]` in window-space logical pixels.
    pub pos: [f32; 2],
    /// Uniform linear-RGBA paint color.
    pub color: Color,
    /// Requested paint-only text decoration.
    pub decoration: TextDecoration,
    /// Shared, renderer-ready shaped layout.
    pub layout: Arc<PreparedTextLayout>,
}

/// Provides the operations defined for DrawText.
impl DrawText {
    /// Returns logical-pixel underline rectangles for non-empty visual lines.
    ///
    /// Decorations other than underline return an empty vector. `dpr` is
    /// clamped to at least `0.01` using floating-point `max`; NaN therefore also
    /// becomes `0.01`, while positive infinity can yield non-finite snapped
    /// coordinates. Each underline is at least one physical pixel thick, uses
    /// the line width, and is vertically snapped to the effective DPR.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextDecoration, TextStyle};
    /// use ailloli_ui_runtime::scene::DrawText;
    /// use ailloli_ui_text::{TextLayoutParams, TextSystem};
    /// let mut system = TextSystem::new();
    /// let layout = system.layout_cached(TextLayoutParams::new(
    ///     "underlined", TextStyle::new(FontId::Ui, 14, Color::WHITE),
    /// ));
    /// let draw = DrawText { pos: [0.0, layout.lines[0].baseline_y], color: Color::WHITE,
    ///     decoration: TextDecoration::Underline, layout };
    /// assert_eq!(draw.decoration_rects(2.0).len(), 1);
    /// assert!(draw.decoration_rects(2.0)[0].h >= 0.5);
    /// ```
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

/// Icon or image draw primitive.
///
/// Rotation is in radians around the renderer-defined image center. Values are
/// stored verbatim and tint uses linear RGBA.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, IconId, Rect};
/// use ailloli_ui_runtime::scene::DrawImage;
/// let image = DrawImage {
///     rect: Rect::new(0.0, 0.0, 16.0, 16.0), icon: IconId::Check,
///     tint: Color::WHITE, rotation_rad: 0.0,
/// };
/// assert_eq!(image.rotation_rad, 0.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct DrawImage {
    /// Destination rectangle in window-space logical pixels.
    pub rect: Rect,
    /// Provider-neutral image/icon identifier.
    pub icon: IconId,
    /// Linear-RGBA multiplicative tint.
    pub tint: Color,
    /// Clockwise rotation in radians; zero means unrotated.
    pub rotation_rad: f32,
}

/// Single paint primitive emitted during the paint phase.
///
/// Commands retain their data and are executed later by a renderer. The enum
/// does not validate geometry, clamp colors, or guarantee backend support.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_runtime::scene::{DrawCmd, DrawRect};
/// let cmd = DrawCmd::Rect(DrawRect { rect: Rect::new(0.0, 0.0, 8.0, 8.0), color: Color::WHITE });
/// assert!(matches!(cmd, DrawCmd::Rect(_)));
/// ```
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
