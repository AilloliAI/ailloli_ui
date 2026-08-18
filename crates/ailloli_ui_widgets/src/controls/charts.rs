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
pub enum ChartSize {
    Compact,
    #[default]
    Default,
    Large,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartTone {
    #[default]
    Accent,
    Success,
    Warning,
    Danger,
    Info,
    Neutral,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartStyle {
    pub background: Color,
    pub plot_background: Color,
    pub grid: Color,
    pub axis: Color,
    pub border: Border,
    pub text: TextStyle,
    pub muted_text: TextStyle,
    pub colors: [Color; 6],
    pub width: f32,
    pub height: f32,
    pub padding: f32,
    pub radius: f32,
    pub bar_gap: f32,
    pub line_thickness: f32,
    pub point_size: f32,
    pub gauge_thickness: f32,
}

impl Default for ChartStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), ChartSize::Default)
    }
}

impl ChartStyle {
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

    fn intrinsic_size(&self) -> Size {
        Size::new(self.width, self.height)
    }

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

pub struct BarChart {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    series: Vec<ChartSeries>,
    labels: Vec<String>,
    y_range: Option<ChartRange>,
    style: ChartStyle,
    empty_text: String,
}

pub struct LineChart {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    series: Vec<ChartSeries>,
    x_range: Option<ChartRange>,
    y_range: Option<ChartRange>,
    show_points: bool,
    style: ChartStyle,
    tone: ChartTone,
    empty_text: String,
}

pub struct RadialGauge {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Binding<f32>,
    range: ChartRange,
    label: Option<Binding<String>>,
    show_value: bool,
    style: ChartStyle,
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

    pub fn series(
        mut self,
        name: impl Into<String>,
        values: impl IntoIterator<Item = f32>,
    ) -> Self {
        self.series.push(ChartSeries::from_values(name, values));
        self
    }

    pub fn chart_series(mut self, series: ChartSeries) -> Self {
        self.series.push(series);
        self
    }

    pub fn labels<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.y_range = Some(ChartRange::new(min, max));
        self
    }

    pub fn chart_style(mut self, style: ChartStyle) -> Self {
        self.style = style;
        self
    }

    pub fn chart_size(mut self, size: ChartSize) -> Self {
        self.style = ChartStyle::from_theme(Theme::default(), size);
        self
    }

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

    pub fn series(
        mut self,
        name: impl Into<String>,
        points: impl IntoIterator<Item = (f32, f32)>,
    ) -> Self {
        self.series.push(ChartSeries::from_xy(name, points));
        self
    }

    pub fn chart_series(mut self, series: ChartSeries) -> Self {
        self.series.push(series);
        self
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.y_range = Some(ChartRange::new(min, max));
        self
    }

    pub fn x_range(mut self, min: f32, max: f32) -> Self {
        self.x_range = Some(ChartRange::new(min, max));
        self
    }

    pub fn show_points(mut self, show: bool) -> Self {
        self.show_points = show;
        self
    }

    pub fn tone(mut self, tone: ChartTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn chart_style(mut self, style: ChartStyle) -> Self {
        self.style = style;
        self
    }

    pub fn chart_size(mut self, size: ChartSize) -> Self {
        self.style = ChartStyle::from_theme(Theme::default(), size);
        self
    }

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

    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.range = ChartRange::new(min, max);
        self
    }

    pub fn label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    pub fn tone(mut self, tone: ChartTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn chart_style(mut self, style: ChartStyle) -> Self {
        self.style = style;
        self
    }

    pub fn chart_size(mut self, size: ChartSize) -> Self {
        self.style = ChartStyle::from_theme(Theme::default(), size);
        self
    }
}

struct BarChartWidget {
    layout: LayoutStyle,
    series: Vec<ChartSeries>,
    labels: Vec<String>,
    y_range: Option<ChartRange>,
    style: ChartStyle,
    empty_text: String,
}

struct LineChartWidget {
    layout: LayoutStyle,
    series: Vec<ChartSeries>,
    x_range: Option<ChartRange>,
    y_range: Option<ChartRange>,
    show_points: bool,
    style: ChartStyle,
    tone: ChartTone,
    empty_text: String,
}

struct RadialGaugeWidget {
    layout: LayoutStyle,
    value: Binding<f32>,
    range: ChartRange,
    label: Option<Binding<String>>,
    show_value: bool,
    style: ChartStyle,
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

fn paint_plot_background(ctx: &mut PaintCtx<'_>, plot: Rect, style: &ChartStyle) {
    if plot.w > 0.0 && plot.h > 0.0 {
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: plot,
            radius: 5.0,
            color: style.plot_background,
        }));
    }
}

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

fn paint_grid(ctx: &mut PaintCtx<'_>, plot: Rect, style: &ChartStyle) {
    for idx in 1..=3 {
        let y = plot.y + plot.h * idx as f32 / 4.0;
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(plot.x, y, plot.w, 1.0),
            color: style.grid,
        }));
    }
}

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

fn value_y(plot: Rect, range: ChartRange, value: f32) -> f32 {
    plot.bottom() - range.fraction_for_value(value) * plot.h
}

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
        layout,
    }));
}

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
        layout,
    }));
}
