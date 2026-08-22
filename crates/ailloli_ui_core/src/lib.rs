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
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::app_identity::ApplicationId;
/// assert_eq!(ApplicationId::parse("org.example.app")?.as_str(), "org.example.app");
/// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
/// ```
pub mod app_identity;
/// Pure chart value mapping and series helpers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::chart::ChartRange;
/// assert_eq!(ChartRange::new(0.0, 10.0).fraction_for_value(5.0), 0.5);
/// ```
pub mod chart;
/// Pure transport-agnostic chat session and message state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::chat::{ChatSessionId, ChatSessionState};
/// assert!(ChatSessionState::new(ChatSessionId::new("1"), "Demo").messages.is_empty());
/// ```
pub mod chat;
/// Pure color picker values and conversions.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::color_picker::color_to_hsv;
/// assert_eq!(color_to_hsv(ailloli_ui_core::Color::rgb(255, 0, 0)).h, 0.0);
/// ```
pub mod color_picker;
/// Pure date picker values and calendar helpers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::date_picker::DateValue;
/// assert_eq!(DateValue::new(2024, 2, 31).day, 29);
/// ```
pub mod date_picker;
/// Platform-neutral input events (pointer, keyboard, IME, window).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{FocusEvent, Event};
/// assert!(matches!(Event::Focus(FocusEvent::new(true)), Event::Focus(_)));
/// ```
pub mod event;
/// Logical clip shapes and 2D primitives (`Rect`, `Size`, …).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::geometry::Rect;
/// assert!(Rect::new(0.0, 0.0, 10.0, 10.0).contains(5.0, 5.0));
/// ```
pub mod geometry;
/// Stable identifiers for elements, widgets, fonts, and icons.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ids::WidgetId;
/// assert_eq!(WidgetId(3).0, 3);
/// ```
pub mod ids;
/// Logical ↔ physical coordinate conversion and snapping.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::math::{to_physical_f32, Scale};
/// assert_eq!(to_physical_f32(2.0, Scale::new(2.0)), 4.0);
/// ```
pub mod math;
/// Pure progress value mapping.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::progress::ProgressSpec;
/// assert_eq!(ProgressSpec::new(0.0, 10.0).fraction_for_value(5.0), 0.5);
/// ```
pub mod progress;
/// Pure scroll state, metrics, and wheel normalization.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::scroll::{ScrollAxes, ScrollState};
/// assert_eq!(ScrollState::new().offset, ailloli_ui_core::Offset::default());
/// assert!(ScrollAxes::BOTH.vertical);
/// ```
pub mod scroll;
/// Pure slider value mapping, snapping, and range helpers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::slider::SliderSpec;
/// assert_eq!(SliderSpec::new(0.0, 10.0).with_step(2.0).snap_value(3.0), 4.0);
/// ```
pub mod slider;
/// Colors, layout lengths, flex, theme, and box styling.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{Color, Length};
/// assert_eq!(Length::percent(50.0).resolve(100.0), Some(50.0));
/// assert_eq!(Color::WHITE.as_rgba8(), (255, 255, 255, 255));
/// ```
pub mod style;
/// Pure time picker values and formatting helpers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::time_picker::{TimeFormat, TimeValue};
/// assert_eq!(TimeValue::new(13, 5).format(TimeFormat::Hour12), "1:05 PM");
/// ```
pub mod time_picker;
/// Pure upload/dropzone metadata and accept matching.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::upload::{UploadAccept, UploadFile};
/// assert!(UploadAccept::new([".png"]).accepts(&UploadFile::named("icon.png")));
/// ```
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
pub use ids::{ElementId, FontId, IconId, ImageId, LogicalWindowId, SvgSource, WidgetId};
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
