//! Pure data types shared across the Ailloli UI framework.
//!
//! # Boundaries
//!
//! This crate intentionally has:
//!
//! - **No** `winit` / `wgpu` dependencies,
//! - **No** runtime widget tree (`Element`, `Widget` live in `ailloli_ui_runtime`),
//! - **Only** portable values: geometry, styling, identifiers, input events, and DPI math.
//!
//! Higher crates (`ailloli_ui_runtime`, `ailloli_ui_widgets`, `ailloli_ui_render_wgpu`, `ailloli_ui_winit`)
//! build behavior on top of these types.

/// Portable application identity and conventional icon descriptors.
pub mod app_identity;
/// Pure chart value mapping and series helpers.
pub mod chart;
/// Pure transport-agnostic chat session and message state.
pub mod chat;
/// Pure color picker values and conversions.
pub mod color_picker;
/// Pure date picker values and calendar helpers.
pub mod date_picker;
/// Platform-neutral input events (pointer, keyboard, IME, window).
pub mod event;
/// Logical clip shapes and 2D primitives (`Rect`, `Size`, …).
pub mod geometry;
/// Stable identifiers for elements, widgets, fonts, and icons.
pub mod ids;
/// Logical ↔ physical coordinate conversion and snapping.
pub mod math;
/// Pure progress value mapping.
pub mod progress;
/// Pure scroll state, metrics, and wheel normalization.
pub mod scroll;
/// Pure slider value mapping, snapping, and range helpers.
pub mod slider;
/// Colors, layout lengths, flex, theme, and box styling.
pub mod style;
/// Pure time picker values and formatting helpers.
pub mod time_picker;
/// Pure upload/dropzone metadata and accept matching.
pub mod upload;

pub use app_identity::{
    AppIcon, AppIconMetadata, AppIdentity, AppIdentityError, AppIdentityMetadata, ApplicationId,
    ValidatedAppIdentity, AILLOLI_UI_PACKAGE_METADATA_PATH_ENV, APP_IDENTITY_METADATA_VERSION,
    CONVENTIONAL_APP_ICON_PATH, OCTAVUI_PACKAGE_METADATA_PATH_ENV,
};
pub use chart::{auto_x_range, auto_y_range, ChartPoint, ChartRange, ChartSeries};
pub use chat::{
    ChatEvent, ChatItemId, ChatMessage, ChatMessageKind, ChatMessageStatus, ChatRequestId,
    ChatRole, ChatSessionId, ChatSessionState, ChatSessionStatus, ChatSessionSummary,
};
pub use color_picker::{color_to_hsv, format_hex_rgb, hsv_to_color, parse_hex_rgb, HsvColor};
pub use date_picker::{DateValue, MonthValue, WeekStart};
pub use event::Event;
pub use geometry::{ClipShape, Constraints, EdgeInsets, Offset, Point, Rect, Size};
pub use ids::{ElementId, FontId, IconId, ImageId, SvgSource, WidgetId};
pub use math::{PhysicalRectI32, Scale};
pub use progress::ProgressSpec;
pub use scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollOutcome, ScrollState};
pub use slider::{SliderRangeValue, SliderSpec, SliderThumb};
#[allow(deprecated)]
pub use style::Rgba;
#[allow(deprecated)]
pub use style::Shadow;
pub use style::{
    Border, BorderStyle, BoxShadow, Color, ColorParseError, EdgeColors, LineCap, LineJoin, Radius,
    StrokeStyle, TextDecoration, TextStyle, Theme, ThemePalette, ThemeRadius, ThemeShadows,
    ThemeSpacing, ThemeState, ThemeTypography,
};
pub use time_picker::{TimeFormat, TimeValue};
pub use upload::{UploadAccept, UploadFile};
