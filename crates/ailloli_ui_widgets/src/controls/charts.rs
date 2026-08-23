//! Non-interactive bar, line, and radial-gauge data visualizations.

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{
    auto_x_range, auto_y_range, ChartRange, ChartSeries, Color, FontId, LineCap, LineJoin, Point,
    StrokeStyle, TextStyle, Theme,
};
use ailloli_ui_runtime::component::{Binding, IntoView, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{
    DrawBorder, DrawCmd, DrawPolyline, DrawRRect, DrawRect, DrawRingProgress, DrawText,
};
use ailloli_ui_text::{TextLayoutParams, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Built-in chart canvas and stroke sizes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ChartSize;
/// let sizes = [ChartSize::Compact, ChartSize::Default, ChartSize::Large];
/// assert_eq!(sizes.len(), 3);
/// assert_eq!(ChartSize::default(), ChartSize::Default);
/// ```
pub enum ChartSize {
    /// 180 by 124 logical-pixel canvas.
    Compact,
    /// 240 by 164 logical-pixel canvas; the default.
    #[default]
    Default,
    /// 320 by 220 logical-pixel canvas.
    Large,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Semantic series color for line charts and radial gauges.
///
/// Bar charts instead cycle through all six colors in [`ChartStyle::colors`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ChartTone;
/// let tones = [
///     ChartTone::Accent,
///     ChartTone::Success,
///     ChartTone::Warning,
///     ChartTone::Danger,
///     ChartTone::Info,
///     ChartTone::Neutral,
/// ];
/// assert_eq!(tones.len(), 6);
/// assert_eq!(ChartTone::default(), ChartTone::Accent);
/// ```
pub enum ChartTone {
    /// Accent-brand color; the default.
    #[default]
    Accent,
    /// Successful-state color.
    Success,
    /// Warning-state color.
    Warning,
    /// Destructive or error color.
    Danger,
    /// Informational color.
    Info,
    /// Muted neutral color.
    Neutral,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved chart colors, typography, and logical-pixel geometry.
///
/// Custom geometry is not validated. Padding larger than the canvas collapses
/// the content area to zero; non-finite values can propagate to draw commands.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{ChartSize, ChartStyle};
/// let style = ChartStyle::from_theme(Theme::dark(), ChartSize::Compact);
/// assert_eq!((style.width, style.height, style.padding), (180.0, 124.0, 10.0));
/// assert_eq!(style.colors.len(), 6);
/// ```
pub struct ChartStyle {
    /// Rounded outer canvas fill.
    pub background: Color,
    /// Plot area and radial-gauge track fill.
    pub plot_background: Color,
    /// Horizontal grid-line color.
    pub grid: Color,
    /// Left and bottom axis color.
    pub axis: Color,
    /// Outer canvas border.
    pub border: Border,
    /// Title and gauge percentage typography.
    pub text: TextStyle,
    /// Empty-state, x-label, and gauge-label typography.
    pub muted_text: TextStyle,
    /// Accent, success, info, warning, danger, and neutral colors in that order.
    pub colors: [Color; 6],
    /// Intrinsic canvas width in logical pixels.
    pub width: f32,
    /// Intrinsic canvas height in logical pixels.
    pub height: f32,
    /// Inner inset on every side in logical pixels.
    pub padding: f32,
    /// Outer canvas corner radius in logical pixels.
    pub radius: f32,
    /// Horizontal space reserved within each bar group in logical pixels.
    pub bar_gap: f32,
    /// Line-chart stroke width in logical pixels.
    pub line_thickness: f32,
    /// Line-chart point diameter in logical pixels.
    pub point_size: f32,
    /// Preferred radial-gauge ring thickness in logical pixels.
    pub gauge_thickness: f32,
}

impl Default for ChartStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), ChartSize::Default)
    }
}

impl ChartStyle {
    /// Resolves all chart styling from a theme and built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{ChartSize, ChartStyle};
    /// let style = ChartStyle::from_theme(Theme::default(), ChartSize::Large);
    /// assert_eq!((style.width, style.height), (320.0, 220.0));
    /// assert_eq!((style.line_thickness, style.point_size), (3.0, 6.0));
    /// ```
    pub fn from_theme(theme: Theme, size: ChartSize) -> Self {
        let palette = theme.palette();
        let (width, height, text_px, padding, line_thickness, point_size, gauge_thickness) =
            match size {
                ChartSize::Compact => (180.0, 124.0, 10, 10.0, 2.0, 4.0, 6.0),
                ChartSize::Default => (240.0, 164.0, 11, 12.0, 2.5, 5.0, 8.0),
                ChartSize::Large => (320.0, 220.0, 13, 16.0, 3.0, 6.0, 10.0),
            };
        Self {
            background: palette.surface,
            plot_background: palette.surface_elevated.with_alpha(0.42),
            grid: palette.border.with_alpha(0.40),
            axis: palette.border.with_alpha(0.72),
            border: Border::new(1.0, palette.border.with_alpha(0.72)),
            text: TextStyle::new(FontId::Ui, text_px, palette.text),
            muted_text: TextStyle::new(FontId::Ui, text_px, palette.text_muted),
            colors: [
                palette.accent,
                palette.success,
                palette.info,
                palette.warning,
                palette.danger,
                palette.text_muted,
            ],
            width,
            height,
            padding,
            radius: theme.radius().md,
            bar_gap: 4.0,
            line_thickness,
            point_size,
            gauge_thickness,
        }
    }

    /// Returns the configured intrinsic canvas size.
    fn intrinsic_size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// Resolves a semantic tone through the fixed six-color palette.
    fn tone_color(&self, tone: ChartTone) -> Color {
        match tone {
            ChartTone::Accent => self.colors[0],
            ChartTone::Success => self.colors[1],
            ChartTone::Info => self.colors[2],
            ChartTone::Warning => self.colors[3],
            ChartTone::Danger => self.colors[4],
            ChartTone::Neutral => self.colors[5],
        }
    }
}

/// A grouped vertical bar chart with optional x-axis labels.
///
/// Each added series forms one bar per point index. Point `x` coordinates are
/// ignored; non-finite `y` values are skipped. The longest series determines
/// group count, and labels are independently distributed across the plot, so a
/// mismatched label count does not automatically align with groups. The first
/// series name is used as the title; no legend is drawn.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::BarChart;
/// let chart = BarChart::new()
///     .series("Requests", [12.0, 18.0, 9.0])
///     .labels(["Mon", "Tue", "Wed"]);
/// let _ = chart;
/// ```
pub struct BarChart {
    /// Layout configuration applied to the intrinsic canvas.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Series in insertion/color-cycle order.
    series: Vec<ChartSeries>,
    /// Independently spaced x-axis labels.
    labels: Vec<String>,
    /// Optional explicit sanitized y-domain.
    y_range: Option<ChartRange>,
    /// Resolved paint and geometry.
    style: ChartStyle,
    /// Empty-state text.
    empty_text: String,
}

/// A multi-series polyline chart using one semantic color.
///
/// All series share the selected tone; the first series name is the title and
/// no legend is drawn. Empty series display the empty text. A non-empty series
/// containing only non-finite coordinates is not considered empty, but yields
/// no valid polyline. When points are enabled, callers should provide finite
/// coordinates because point markers are emitted without an additional filter.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{ChartTone, LineChart};
/// let chart = LineChart::new()
///     .series("Latency", [(0.0, 12.0), (1.0, 8.0)])
///     .show_points(true)
///     .tone(ChartTone::Info);
/// let _ = chart;
/// ```
pub struct LineChart {
    /// Layout configuration applied to the intrinsic canvas.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Series in insertion order.
    series: Vec<ChartSeries>,
    /// Optional explicit sanitized x-domain.
    x_range: Option<ChartRange>,
    /// Optional explicit sanitized y-domain.
    y_range: Option<ChartRange>,
    /// Whether to paint a circular marker for every source point.
    show_points: bool,
    /// Resolved paint and geometry.
    style: ChartStyle,
    /// Shared semantic series color.
    tone: ChartTone,
    /// Empty-state text.
    empty_text: String,
}

/// A radial normalized value indicator with optional percentage and label.
///
/// Values are mapped through [`ChartRange`], clamped to `0.0..=1.0`, and
/// non-finite values map to zero. The optional percentage is rounded to a whole
/// number. The default range is `0.0..=1.0`, value is zero, and text is hidden.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::RadialGauge;
/// let gauge = RadialGauge::new()
///     .value(75.0)
///     .range(0.0, 100.0)
///     .show_value(true)
///     .label("Complete");
/// let _ = gauge;
/// ```
pub struct RadialGauge {
    /// Layout configuration applied to the intrinsic canvas.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Live data value in the range's caller-defined unit.
    value: Binding<f32>,
    /// Sanitized inclusive data domain.
    range: ChartRange,
    /// Optional live label below the ring.
    label: Option<Binding<String>>,
    /// Whether to paint the rounded percentage inside the ring.
    show_value: bool,
    /// Resolved paint and geometry.
    style: ChartStyle,
    /// Semantic ring fill color.
    tone: ChartTone,
}

crate::impl_layout_builders_unit!(BarChart);
crate::impl_layout_builders_unit!(LineChart);
crate::impl_layout_builders_unit!(RadialGauge);

impl Default for BarChart {
    fn default() -> Self {
        Self::new()
    }
}

impl BarChart {
    /// Creates an empty chart with automatic y-range and `"No data"` text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::BarChart;
    /// let chart = BarChart::new();
    /// let _ = chart;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            series: Vec::new(),
            labels: Vec::new(),
            y_range: None,
            style: ChartStyle::default(),
            empty_text: "No data".to_string(),
        }
    }

    /// Appends a named series whose x coordinates are zero-based indices.
    ///
    /// Values, including non-finite values, are retained; the painter skips
    /// non-finite y values. Empty series are allowed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::BarChart;
    /// let chart = BarChart::new().series("CPU", [20.0, 35.0]);
    /// let _ = chart;
    /// ```
    pub fn series(
        mut self,
        name: impl Into<String>,
        values: impl IntoIterator<Item = f32>,
    ) -> Self {
        self.series.push(ChartSeries::from_values(name, values));
        self
    }

    /// Appends an existing series without normalization.
    ///
    /// Its x coordinates are ignored by bar placement.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartSeries;
    /// use ailloli_ui_widgets::controls::BarChart;
    /// let chart = BarChart::new().chart_series(ChartSeries::from_values("CPU", [1.0, 2.0]));
    /// let _ = chart;
    /// ```
    pub fn chart_series(mut self, series: ChartSeries) -> Self {
        self.series.push(series);
        self
    }

    /// Replaces all x-axis labels with values collected in iterator order.
    ///
    /// Labels are spread according to their own count, independently of bar
    /// groups. Empty text remains a label; an empty iterator hides the label row.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::BarChart;
    /// let chart = BarChart::new().labels(["Q1", "Q2"]);
    /// let _ = chart;
    /// ```
    pub fn labels<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// Sets an explicit y-domain after finite/order/degeneracy sanitization.
    ///
    /// Reversed bounds are swapped, non-finite bounds fall back to `0`/`1`, and
    /// nearly equal bounds are widened when representable. Values outside the
    /// domain are clamped to the plot edges.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::BarChart;
    /// let chart = BarChart::new().range(-100.0, 100.0);
    /// let _ = chart;
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.y_range = Some(ChartRange::new(min, max));
        self
    }

    /// Replaces complete chart style and intrinsic geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BarChart, ChartStyle};
    /// let chart = BarChart::new().chart_style(ChartStyle::default());
    /// let _ = chart;
    /// ```
    pub fn chart_style(mut self, style: ChartStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces style with a default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BarChart, ChartSize};
    /// let chart = BarChart::new().chart_size(ChartSize::Large);
    /// let _ = chart;
    /// ```
    pub fn chart_size(mut self, size: ChartSize) -> Self {
        self.style = ChartStyle::from_theme(Theme::default(), size);
        self
    }

    /// Replaces text painted when there are no points or no plot area.
    ///
    /// Empty text is valid and paints no glyphs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::BarChart;
    /// let chart = BarChart::new().empty_text("Awaiting samples");
    /// let _ = chart;
    /// ```
    pub fn empty_text(mut self, text: impl Into<String>) -> Self {
        self.empty_text = text.into();
        self
    }
}

impl Default for LineChart {
    fn default() -> Self {
        Self::new()
    }
}

impl LineChart {
    /// Creates an empty accent chart with automatic axes and hidden points.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::LineChart;
    /// let chart = LineChart::new();
    /// let _ = chart;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            series: Vec::new(),
            x_range: None,
            y_range: None,
            show_points: false,
            style: ChartStyle::default(),
            tone: ChartTone::Accent,
            empty_text: "No data".to_string(),
        }
    }

    /// Appends a named series of `(x, y)` points without normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::LineChart;
    /// let chart = LineChart::new().series("CPU", [(10.0, 0.4), (20.0, 0.8)]);
    /// let _ = chart;
    /// ```
    pub fn series(
        mut self,
        name: impl Into<String>,
        points: impl IntoIterator<Item = (f32, f32)>,
    ) -> Self {
        self.series.push(ChartSeries::from_xy(name, points));
        self
    }

    /// Appends an existing series without normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartSeries;
    /// use ailloli_ui_widgets::controls::LineChart;
    /// let chart = LineChart::new().chart_series(ChartSeries::from_xy("CPU", [(0.0, 1.0)]));
    /// let _ = chart;
    /// ```
    pub fn chart_series(mut self, series: ChartSeries) -> Self {
        self.series.push(series);
        self
    }

    /// Sets an explicit sanitized y-domain.
    ///
    /// Reversed bounds are swapped, non-finite bounds fall back to `0`/`1`, and
    /// values outside the result are clamped to the plot edges.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::LineChart;
    /// let chart = LineChart::new().range(0.0, 100.0);
    /// let _ = chart;
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.y_range = Some(ChartRange::new(min, max));
        self
    }

    /// Sets an explicit sanitized x-domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::LineChart;
    /// let chart = LineChart::new().x_range(1_000.0, 2_000.0);
    /// let _ = chart;
    /// ```
    pub fn x_range(mut self, min: f32, max: f32) -> Self {
        self.x_range = Some(ChartRange::new(min, max));
        self
    }

    /// Shows or hides a marker for every source point.
    ///
    /// The default is `false`. Point markers do not filter non-finite source
    /// coordinates, so finite coordinates are required when enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::LineChart;
    /// let chart = LineChart::new().show_points(true);
    /// let _ = chart;
    /// ```
    pub fn show_points(mut self, show: bool) -> Self {
        self.show_points = show;
        self
    }

    /// Sets the single semantic color shared by all series.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ChartTone, LineChart};
    /// let chart = LineChart::new().tone(ChartTone::Success);
    /// let _ = chart;
    /// ```
    pub fn tone(mut self, tone: ChartTone) -> Self {
        self.tone = tone;
        self
    }

    /// Replaces complete chart style and intrinsic geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ChartStyle, LineChart};
    /// let chart = LineChart::new().chart_style(ChartStyle::default());
    /// let _ = chart;
    /// ```
    pub fn chart_style(mut self, style: ChartStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces style with a default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ChartSize, LineChart};
    /// let chart = LineChart::new().chart_size(ChartSize::Compact);
    /// let _ = chart;
    /// ```
    pub fn chart_size(mut self, size: ChartSize) -> Self {
        self.style = ChartStyle::from_theme(Theme::default(), size);
        self
    }

    /// Replaces text painted only when every series has no points.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::LineChart;
    /// let chart = LineChart::new().empty_text("No samples");
    /// let _ = chart;
    /// ```
    pub fn empty_text(mut self, text: impl Into<String>) -> Self {
        self.empty_text = text.into();
        self
    }
}

impl Default for RadialGauge {
    fn default() -> Self {
        Self::new()
    }
}

impl RadialGauge {
    /// Creates a zero-valued accent gauge over `0.0..=1.0` with no text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadialGauge;
    /// let gauge = RadialGauge::new();
    /// let _ = gauge;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: Binding::Static(0.0),
            range: ChartRange::default(),
            label: None,
            show_value: false,
            style: ChartStyle::default(),
            tone: ChartTone::Accent,
        }
    }

    /// Sets the static or reactive data value.
    ///
    /// Painting maps it through the configured range, clamps out-of-range
    /// values, and treats NaN or infinity as the range minimum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadialGauge;
    /// let gauge = RadialGauge::new().value(0.75);
    /// let _ = gauge;
    /// ```
    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self
    }

    /// Sets the inclusive sanitized data domain.
    ///
    /// Reversed bounds are swapped and non-finite bounds become `0`/`1`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadialGauge;
    /// let gauge = RadialGauge::new().range(0.0, 100.0).value(25.0);
    /// let _ = gauge;
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.range = ChartRange::new(min, max);
        self
    }

    /// Sets static or reactive text painted below the ring.
    ///
    /// Empty text remains present but paints no glyphs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadialGauge;
    /// let gauge = RadialGauge::new().label("Storage");
    /// let _ = gauge;
    /// ```
    pub fn label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Shows or hides a whole-number percentage inside the ring.
    ///
    /// The default is `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadialGauge;
    /// let gauge = RadialGauge::new().show_value(true);
    /// let _ = gauge;
    /// ```
    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    /// Sets the semantic ring fill color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ChartTone, RadialGauge};
    /// let gauge = RadialGauge::new().tone(ChartTone::Warning);
    /// let _ = gauge;
    /// ```
    pub fn tone(mut self, tone: ChartTone) -> Self {
        self.tone = tone;
        self
    }

    /// Replaces complete gauge style and intrinsic geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ChartStyle, RadialGauge};
    /// let gauge = RadialGauge::new().chart_style(ChartStyle::default());
    /// let _ = gauge;
    /// ```
    pub fn chart_style(mut self, style: ChartStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces style with a default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ChartSize, RadialGauge};
    /// let gauge = RadialGauge::new().chart_size(ChartSize::Large);
    /// let _ = gauge;
    /// ```
    pub fn chart_size(mut self, size: ChartSize) -> Self {
        self.style = ChartStyle::from_theme(Theme::default(), size);
        self
    }
}

/// Retained leaf widget that paints grouped bar series.
struct BarChartWidget {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Ordered bar series; each series is aligned with `labels` by index.
    series: Vec<ChartSeries>,
    /// Ordered category labels along the horizontal axis.
    labels: Vec<String>,
    /// Optional explicit vertical domain; otherwise finite data determines it.
    y_range: Option<ChartRange>,
    /// Shared colors and logical-pixel chart geometry.
    style: ChartStyle,
    /// Message painted when no finite samples are available.
    empty_text: String,
}

/// Retained leaf widget that paints one-color polyline series.
struct LineChartWidget {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Ordered polyline series.
    series: Vec<ChartSeries>,
    /// Optional explicit horizontal domain.
    x_range: Option<ChartRange>,
    /// Optional explicit vertical domain.
    y_range: Option<ChartRange>,
    /// Whether finite samples receive point markers.
    show_points: bool,
    /// Shared colors and logical-pixel chart geometry.
    style: ChartStyle,
    /// Semantic palette tone applied to fallback series colors.
    tone: ChartTone,
    /// Message painted when no finite samples are available.
    empty_text: String,
}

/// Retained leaf widget that reads and paints a reactive gauge value/label.
struct RadialGaugeWidget {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Reactive gauge value; non-finite values use the range minimum.
    value: Binding<f32>,
    /// Inclusive value domain mapped onto the arc sweep.
    range: ChartRange,
    /// Optional reactive center label.
    label: Option<Binding<String>>,
    /// Whether a formatted numeric value is painted below the label.
    show_value: bool,
    /// Shared colors and logical-pixel gauge geometry.
    style: ChartStyle,
    /// Semantic palette tone used for the filled arc.
    tone: ChartTone,
}

impl<A: 'static> Widget<A> for BarChartWidget {
    fn debug_name(&self) -> &'static str {
        "BarChart"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        chart_layout_result(self.style.intrinsic_size(), self.layout, constraints)
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_bar_chart(
            ctx,
            bounds,
            &self.series,
            &self.labels,
            self.y_range,
            &self.style,
            &self.empty_text,
        );
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> Widget<A> for LineChartWidget {
    fn debug_name(&self) -> &'static str {
        "LineChart"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        chart_layout_result(self.style.intrinsic_size(), self.layout, constraints)
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_line_chart(
            ctx,
            bounds,
            &self.series,
            self.x_range,
            self.y_range,
            self.show_points,
            self.tone,
            &self.style,
            &self.empty_text,
        );
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> Widget<A> for RadialGaugeWidget {
    fn debug_name(&self) -> &'static str {
        "RadialGauge"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        chart_layout_result(self.style.intrinsic_size(), self.layout, constraints)
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_radial_gauge(
            ctx,
            bounds,
            self.range.fraction_for_value(self.value.read()),
            self.label.as_ref(),
            self.show_value,
            self.tone,
            &self.style,
        );
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> IntoView<A> for BarChart {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(BarChartWidget {
                layout: self.layout,
                series: self.series,
                labels: self.labels,
                y_range: self.y_range,
                style: self.style,
                empty_text: self.empty_text,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

impl<A: 'static> IntoView<A> for LineChart {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(LineChartWidget {
                layout: self.layout,
                series: self.series,
                x_range: self.x_range,
                y_range: self.y_range,
                show_points: self.show_points,
                style: self.style,
                tone: self.tone,
                empty_text: self.empty_text,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

impl<A: 'static> IntoView<A> for RadialGauge {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(RadialGaugeWidget {
                layout: self.layout,
                value: self.value,
                range: self.range,
                label: self.label,
                show_value: self.show_value,
                style: self.style,
                tone: self.tone,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Builds childless layout from intrinsic style size and layout overrides.
fn chart_layout_result(
    intrinsic: Size,
    layout: LayoutStyle,
    constraints: Constraints,
) -> LayoutResult {
    let size = apply_layout_size(intrinsic, layout, constraints);
    let rect = Rect::new(0.0, 0.0, size.w, size.h);
    LayoutResult {
        size,
        children: Vec::new(),
        paint_bounds: rect,
        visual_bounds: rect,
        overlay_hit_bounds: Vec::new(),
        clip: None,
        is_window_root_clip: false,
        artifact: None,
    }
}

/// Paints the rounded canvas/border and returns its non-negative content inset.
fn paint_chart_frame(ctx: &mut PaintCtx<'_>, bounds: Rect, style: &ChartStyle) -> Rect {
    if bounds.w <= 0.0 || bounds.h <= 0.0 {
        return bounds;
    }
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: bounds,
        radius: style.radius,
        color: style.background,
    }));
    if style.border.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: Radius::uniform(style.radius),
            border: style.border,
        }));
    }
    Rect::new(
        bounds.x + style.padding,
        bounds.y + style.padding,
        (bounds.w - style.padding * 2.0).max(0.0),
        (bounds.h - style.padding * 2.0).max(0.0),
    )
}

/// Paints grouped indexed bars, axes, title, labels, or the empty state.
fn paint_bar_chart(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    series: &[ChartSeries],
    labels: &[String],
    y_range: Option<ChartRange>,
    style: &ChartStyle,
    empty_text: &str,
) {
    let content = paint_chart_frame(ctx, bounds, style);
    let title_h = 16.0;
    let label_h = if labels.is_empty() { 4.0 } else { 18.0 };
    let plot = Rect::new(
        content.x,
        content.y + title_h,
        content.w,
        (content.h - title_h - label_h).max(0.0),
    );
    paint_chart_title(ctx, content, series.first().map(|s| s.name.as_str()), style);
    paint_plot_background(ctx, plot, style);

    let max_points = series.iter().map(|s| s.points.len()).max().unwrap_or(0);
    if series.is_empty() || max_points == 0 || plot.w <= 0.0 || plot.h <= 0.0 {
        paint_centered_text(ctx, plot, empty_text, style.muted_text);
        return;
    }

    let range = y_range.unwrap_or_else(|| auto_y_range(series.iter()));
    paint_grid(ctx, plot, style);

    let group_w = plot.w / max_points as f32;
    let series_count = series.len().max(1);
    let bar_w = ((group_w - style.bar_gap) / series_count as f32).max(1.0);
    let zero_y = value_y(plot, range, 0.0);
    for (series_idx, chart_series) in series.iter().enumerate() {
        let color = style.colors[series_idx % style.colors.len()];
        for (idx, point) in chart_series.points.iter().enumerate() {
            if !point.y.is_finite() {
                continue;
            }
            let x = plot.x + idx as f32 * group_w + style.bar_gap * 0.5 + series_idx as f32 * bar_w;
            let y = value_y(plot, range, point.y);
            let rect_y = y.min(zero_y);
            let h = (zero_y - y).abs().max(1.0);
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(x, rect_y, (bar_w - 1.0).max(1.0), h),
                radius: 3.0,
                color,
            }));
        }
    }
    paint_axis(ctx, plot, style);
    paint_x_labels(ctx, plot, labels, style);
}

#[allow(clippy::too_many_arguments)]
/// Paints clipped polyline series, optional markers, axes, or the empty state.
fn paint_line_chart(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    series: &[ChartSeries],
    x_range: Option<ChartRange>,
    y_range: Option<ChartRange>,
    show_points: bool,
    tone: ChartTone,
    style: &ChartStyle,
    empty_text: &str,
) {
    let content = paint_chart_frame(ctx, bounds, style);
    let title_h = 16.0;
    let plot = Rect::new(
        content.x,
        content.y + title_h,
        content.w,
        (content.h - title_h - 4.0).max(0.0),
    );
    paint_chart_title(ctx, content, series.first().map(|s| s.name.as_str()), style);
    paint_plot_background(ctx, plot, style);

    if series.iter().all(ChartSeries::is_empty) || plot.w <= 0.0 || plot.h <= 0.0 {
        paint_centered_text(ctx, plot, empty_text, style.muted_text);
        return;
    }

    let x_range = x_range.unwrap_or_else(|| auto_x_range(series.iter()));
    let y_range = y_range.unwrap_or_else(|| auto_y_range(series.iter()));
    let color = style.tone_color(tone);
    paint_grid(ctx, plot, style);
    ctx.with_clip(plot, |ctx| {
        for chart_series in series {
            let points: Vec<Point> = chart_series
                .points
                .iter()
                .filter_map(|point| {
                    let [x, y] = point_to_screen(plot, x_range, y_range, *point);
                    if x.is_finite() && y.is_finite() {
                        Some(Point::new(x, y))
                    } else {
                        None
                    }
                })
                .collect();
            if points.len() >= 2 {
                ctx.push(DrawCmd::Polyline(DrawPolyline {
                    points,
                    stroke: StrokeStyle {
                        color,
                        width: style.line_thickness,
                        cap: LineCap::Butt,
                        join: LineJoin::Bevel,
                        miter_limit: 4.0,
                    },
                }));
            }
            if show_points {
                for point in &chart_series.points {
                    let [x, y] = point_to_screen(plot, x_range, y_range, *point);
                    let size = style.point_size;
                    ctx.push(DrawCmd::RRect(DrawRRect {
                        rect: Rect::new(x - size * 0.5, y - size * 0.5, size, size),
                        radius: size * 0.5,
                        color,
                    }));
                }
            }
        }
    });
    paint_axis(ctx, plot, style);
}

/// Paints the clamped ring fraction and optional percentage/label text.
fn paint_radial_gauge(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    fraction: f32,
    label: Option<&Binding<String>>,
    show_value: bool,
    tone: ChartTone,
    style: &ChartStyle,
) {
    let content = paint_chart_frame(ctx, bounds, style);
    let side = content.w.min(content.h - 28.0).max(0.0);
    let ring = Rect::new(content.x + (content.w - side) * 0.5, content.y, side, side);
    let fill = style.tone_color(tone);
    ctx.push(DrawCmd::RingProgress(DrawRingProgress {
        rect: ring,
        thickness: style.gauge_thickness.min(side * 0.5).max(1.0),
        fraction: fraction.clamp(0.0, 1.0),
        track_color: style.plot_background,
        fill_color: fill,
        start_angle: -std::f32::consts::FRAC_PI_2,
    }));

    if show_value {
        let value = format!("{:.0}%", fraction.clamp(0.0, 1.0) * 100.0);
        paint_centered_text(ctx, ring, &value, style.text);
    }
    if let Some(label) = label {
        let label_rect = Rect::new(content.x, ring.bottom() + 6.0, content.w, 18.0);
        paint_centered_text(ctx, label_rect, &label.read(), style.muted_text);
    }
}

/// Paints the rounded plot fill when both dimensions are positive.
fn paint_plot_background(ctx: &mut PaintCtx<'_>, plot: Rect, style: &ChartStyle) {
    if plot.w > 0.0 && plot.h > 0.0 {
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: plot,
            radius: 5.0,
            color: style.plot_background,
        }));
    }
}

/// Paints a non-empty first-series title in the reserved title row.
fn paint_chart_title(
    ctx: &mut PaintCtx<'_>,
    content: Rect,
    title: Option<&str>,
    style: &ChartStyle,
) {
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        paint_text(
            ctx,
            [content.x, content.y + 11.0],
            title,
            style.text,
            Some(content.w),
        );
    }
}

/// Paints three evenly spaced horizontal grid lines.
fn paint_grid(ctx: &mut PaintCtx<'_>, plot: Rect, style: &ChartStyle) {
    for idx in 1..=3 {
        let y = plot.y + plot.h * idx as f32 / 4.0;
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(plot.x, y, plot.w, 1.0),
            color: style.grid,
        }));
    }
}

/// Paints one-pixel bottom and left axes.
fn paint_axis(ctx: &mut PaintCtx<'_>, plot: Rect, style: &ChartStyle) {
    ctx.push(DrawCmd::Rect(DrawRect {
        rect: Rect::new(plot.x, plot.bottom() - 1.0, plot.w, 1.0),
        color: style.axis,
    }));
    ctx.push(DrawCmd::Rect(DrawRect {
        rect: Rect::new(plot.x, plot.y, 1.0, plot.h),
        color: style.axis,
    }));
}

/// Distributes and centers labels across equal-width slots below the plot.
fn paint_x_labels(ctx: &mut PaintCtx<'_>, plot: Rect, labels: &[String], style: &ChartStyle) {
    if labels.is_empty() || plot.w <= 0.0 {
        return;
    }
    let group_w = plot.w / labels.len() as f32;
    for (idx, label) in labels.iter().enumerate() {
        let x = plot.x + idx as f32 * group_w;
        let rect = Rect::new(x, plot.bottom() + 4.0, group_w, 14.0);
        paint_centered_text(ctx, rect, label, style.muted_text);
    }
}

/// Maps a data value to a bottom-origin clamped plot y coordinate.
fn value_y(plot: Rect, range: ChartRange, value: f32) -> f32 {
    plot.bottom() - range.fraction_for_value(value) * plot.h
}

/// Maps a chart point through x/y domains into plot coordinates.
fn point_to_screen(
    plot: Rect,
    x_range: ChartRange,
    y_range: ChartRange,
    point: ailloli_ui_core::ChartPoint,
) -> [f32; 2] {
    [
        plot.x + x_range.fraction_for_value(point.x) * plot.w,
        value_y(plot, y_range, point.y),
    ]
}

/// Shapes one unwrapped line and centers it within a rectangle.
fn paint_centered_text(ctx: &mut PaintCtx<'_>, rect: Rect, text: &str, style: TextStyle) {
    let Some(ts) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let layout = ts.layout_cached(TextLayoutParams {
        text,
        style,
        max_width: Some(rect.w.max(0.0)),
        wrap_mode: WrapMode::NoWrap,
    });
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let x = rect.x + (rect.w - layout.metrics.width).max(0.0) * 0.5;
    let y = rect.y + (rect.h - layout.metrics.height).max(0.0) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style.color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}

/// Shapes and paints one unwrapped line at an explicit baseline position.
fn paint_text(
    ctx: &mut PaintCtx<'_>,
    pos: [f32; 2],
    text: &str,
    style: TextStyle,
    max_width: Option<f32>,
) {
    let Some(ts) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let layout = ts.layout_cached(TextLayoutParams {
        text,
        style,
        max_width,
        wrap_mode: WrapMode::NoWrap,
    });
    ctx.push(DrawCmd::Text(DrawText {
        pos,
        color: style.color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}
