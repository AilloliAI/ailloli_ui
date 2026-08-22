//! Editor padding, gutter allocation, scrolling, and coordinate conversion.

use ailloli_ui_core::{Point, Rect};
use ailloli_ui_text::{TextEditState, WrapMode};

use crate::code::GutterConfig;
use crate::{EditorConfig, EditorStyle, EditorWrapMode};

/// Computes the text content viewport inside the editor bounds.
///
/// Applies uniform logical-pixel padding and clamps width/height at zero. The
/// origin still advances by padding when bounds are smaller or negative.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_editor::{editor_content_rect, EditorStyle};
/// let rect = editor_content_rect(Rect::new(5.0, 7.0, 100.0, 60.0), EditorStyle::default());
/// assert_eq!(rect, Rect::new(15.0, 17.0, 80.0, 40.0));
/// ```
pub fn editor_content_rect(bounds: Rect, style: EditorStyle) -> Rect {
    Rect::new(
        bounds.x + style.pad,
        bounds.y + style.pad,
        (bounds.w - style.pad * 2.0).max(0.0),
        (bounds.h - style.pad * 2.0).max(0.0),
    )
}

/// Current editor viewport in widget/screen coordinates.
///
/// All geometry and scroll values are logical pixels. Soft wrap forces
/// horizontal scroll to zero; vertical scroll is clamped non-negative.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_editor::{EditorConfig, EditorViewport};
/// use ailloli_ui_text::TextEditState;
/// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 100.0, 60.0), EditorConfig::default(), &TextEditState::new());
/// assert_eq!(viewport.content_rect, Rect::new(10.0, 10.0, 80.0, 40.0));
/// assert!(viewport.gutter_rect.is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorViewport {
    /// Complete widget bounds.
    pub bounds: Rect,
    /// Bounds after editor padding, before optional gutter split.
    pub content_rect: Rect,
    /// Text clip/layout viewport after optional gutter allocation.
    pub text_rect: Rect,
    /// Optional left gutter, or `None` when disabled/non-positive.
    pub gutter_rect: Option<Rect>,
    /// Non-negative horizontal text offset; always zero under soft wrap.
    pub scroll_x: f32,
    /// Non-negative vertical content offset.
    pub scroll_y: f32,
    /// Wrap policy used to derive width and scrolling behavior.
    pub wrap_mode: EditorWrapMode,
}

/// Viewport construction and coordinate conversion operations.
impl EditorViewport {
    /// Resolves a viewport without a gutter.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorConfig, EditorViewport};
    /// use ailloli_ui_text::TextEditState;
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 50.0, 50.0), EditorConfig::default(), &TextEditState::new());
    /// assert_eq!(viewport.text_rect, viewport.content_rect);
    /// ```
    pub fn new(bounds: Rect, config: EditorConfig, edit: &TextEditState) -> Self {
        Self::with_gutter(bounds, config, edit, None)
    }

    /// Resolves padding, optional left gutter, and clamped scroll offsets.
    ///
    /// An enabled gutter with positive width is clamped to content width; zero,
    /// negative, or disabled gutter values behave as `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{CodeEditorConfig, EditorConfig, EditorViewport};
    /// use ailloli_ui_text::TextEditState;
    /// let gutter = CodeEditorConfig::default().gutter;
    /// let viewport = EditorViewport::with_gutter(Rect::new(0.0, 0.0, 120.0, 60.0), EditorConfig::default(), &TextEditState::new(), Some(gutter));
    /// assert!(viewport.gutter_rect.is_some());
    /// assert_eq!(viewport.gutter_rect.unwrap().w, gutter.width);
    /// ```
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

    /// Returns the unscrolled text origin minus horizontal scroll.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorConfig, EditorViewport};
    /// use ailloli_ui_text::TextEditState;
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
    /// assert_eq!(viewport.text_origin_x(), 10.0);
    /// ```
    pub fn text_origin_x(self) -> f32 {
        self.text_rect.x - self.scroll_x
    }

    /// Returns the fixed top of the text viewport in logical pixels.
    ///
    /// Vertical scroll is applied per run, not to this origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorConfig, EditorViewport};
    /// use ailloli_ui_text::TextEditState;
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
    /// assert_eq!(viewport.text_origin_y(), 10.0);
    /// ```
    pub fn text_origin_y(self) -> f32 {
        self.text_rect.y
    }

    /// Maps editor wrapping to the text engine's concrete wrap mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorConfig, EditorViewport};
    /// use ailloli_ui_text::{TextEditState, WrapMode};
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
    /// assert_eq!(viewport.text_wrap_mode(), WrapMode::WordOrAnywhere);
    /// ```
    pub fn text_wrap_mode(self) -> WrapMode {
        match self.wrap_mode {
            EditorWrapMode::SoftWrap => WrapMode::WordOrAnywhere,
            EditorWrapMode::NoWrap => WrapMode::NoWrap,
        }
    }

    /// Returns the text viewport width for soft wrap, or `None` for no wrap.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_editor::{EditorConfig, EditorViewport};
    /// use ailloli_ui_text::TextEditState;
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
    /// assert_eq!(viewport.max_text_width(), Some(60.0));
    /// ```
    pub fn max_text_width(self) -> Option<f32> {
        match self.wrap_mode {
            EditorWrapMode::SoftWrap => Some(self.text_rect.w),
            EditorWrapMode::NoWrap => None,
        }
    }

    /// Converts a screen point into scrolled text-local coordinates.
    ///
    /// The x result is clamped at zero; y may be negative above the text rect.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect};
    /// use ailloli_ui_editor::{EditorConfig, EditorViewport};
    /// use ailloli_ui_text::TextEditState;
    /// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
    /// assert_eq!(viewport.local_point(Point::new(15.0, 17.0)), (5.0, 7.0));
    /// ```
    pub fn local_point(self, pos: Point) -> (f32, f32) {
        (
            (pos.x - self.text_rect.x + self.scroll_x).max(0.0),
            pos.y - self.text_rect.y,
        )
    }
}

/// Splits an enabled positive-width left gutter from the content rectangle.
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
