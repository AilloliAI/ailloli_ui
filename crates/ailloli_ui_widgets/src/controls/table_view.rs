use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Length, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Memo, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{
    DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText,
};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, TextSystem, WrapMode};

use super::badge::BadgeTone;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TableViewSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum TableColumnWidth {
    Fixed(f32),
    Flex(f32),
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TableAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableViewStyle {
    pub background: Color,
    pub header_background: Color,
    pub row_background: Color,
    pub row_alt_background: Color,
    pub row_hover_background: Color,
    pub row_active_background: Color,
    pub row_selected_background: Color,
    pub grid_color: Color,
    pub border: Border,
    pub focus_ring: Border,
    pub shadows: Vec<BoxShadow>,
    pub text: TextStyle,
    pub muted_text: TextStyle,
    pub header_text: TextStyle,
    pub disabled_text: TextStyle,
    pub icon_tint: Color,
    pub selected_icon_tint: Color,
    pub badge_text: TextStyle,
    pub progress_track: Color,
    pub progress_fill: Color,
    pub radius: Radius,
    pub row_height: f32,
    pub header_height: f32,
    pub cell_padding_x: f32,
    pub min_column_width: f32,
    pub icon_size: f32,
    pub icon_gap: f32,
    pub progress_width: f32,
    pub progress_height: f32,
    pub disabled_opacity: f32,
}

impl Default for TableViewStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), TableViewSize::Default)
    }
}

impl TableViewStyle {
    pub fn from_theme(theme: Theme, size: TableViewSize) -> Self {
        let palette = theme.palette();
        let (row_height, header_height, padding, text_size, min_column_width) = match size {
            TableViewSize::Compact => (30.0, 30.0, 8.0, 12, 72.0),
            TableViewSize::Default => (36.0, 34.0, 10.0, 13, 88.0),
        };
        Self {
            background: palette.surface,
            header_background: palette.surface_elevated,
            row_background: Color::TRANSPARENT,
            row_alt_background: palette.surface_elevated.with_alpha(0.28),
            row_hover_background: palette.surface_elevated,
            row_active_background: palette.accent.with_alpha(0.10),
            row_selected_background: palette.accent.with_alpha(0.18),
            grid_color: palette.border,
            border: Border::new(1.0, palette.border),
            focus_ring: Border::new(1.0, palette.focus),
            shadows: vec![theme.shadows().sm],
            text: TextStyle::new(FontId::Ui, text_size, palette.text),
            muted_text: TextStyle::new(FontId::Ui, text_size, palette.text_muted),
            header_text: TextStyle::new(FontId::Ui, 11, palette.text_muted),
            disabled_text: TextStyle::new(
                FontId::Ui,
                text_size,
                palette.text_muted.with_alpha(0.68),
            ),
            icon_tint: palette.text_muted,
            selected_icon_tint: palette.accent,
            badge_text: TextStyle::new(FontId::Ui, 11, palette.text),
            progress_track: palette.surface_elevated,
            progress_fill: palette.accent,
            radius: Radius::uniform(theme.radius().md),
            row_height,
            header_height,
            cell_padding_x: padding,
            min_column_width,
            icon_size: 16.0,
            icon_gap: 6.0,
            progress_width: 82.0,
            progress_height: 4.0,
            disabled_opacity: 0.42,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
    label: String,
    width: TableColumnWidth,
    align: TableAlign,
}

impl TableColumn {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            width: TableColumnWidth::Auto,
            align: TableAlign::Start,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = TableColumnWidth::Fixed(width.max(0.0));
        self
    }

    pub fn column_width(mut self, width: TableColumnWidth) -> Self {
        self.width = width;
        self
    }

    pub fn flex(mut self, weight: f32) -> Self {
        self.width = TableColumnWidth::Flex(weight.max(0.0));
        self
    }

    pub fn auto(mut self) -> Self {
        self.width = TableColumnWidth::Auto;
        self
    }

    pub fn align(mut self, align: TableAlign) -> Self {
        self.align = align;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TableCellKind {
    Text,
    Muted,
    Badge(BadgeTone),
    Progress(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableCell {
    label: String,
    kind: TableCellKind,
    align: Option<TableAlign>,
}

impl TableCell {
    pub fn text(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: TableCellKind::Text,
            align: None,
        }
    }

    pub fn muted(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: TableCellKind::Muted,
            align: None,
        }
    }

    pub fn badge(label: impl Into<String>, tone: BadgeTone) -> Self {
        Self {
            label: label.into(),
            kind: TableCellKind::Badge(tone),
            align: None,
        }
    }

    pub fn progress(value: f32) -> Self {
        Self {
            label: String::new(),
            kind: TableCellKind::Progress(value),
            align: None,
        }
    }

    pub fn align(mut self, align: TableAlign) -> Self {
        self.align = Some(align);
        self
    }
}

#[derive(Clone)]
pub struct TableRow<T> {
    id: T,
    cells: Vec<TableCell>,
    selected: Binding<bool>,
    disabled: Binding<bool>,
    leading_icon: Option<IconId>,
}

impl<T> TableRow<T> {
    pub fn new(id: T) -> Self {
        Self {
            id,
            cells: Vec::new(),
            selected: Binding::Static(false),
            disabled: Binding::Static(false),
            leading_icon: None,
        }
    }

    pub fn cell(mut self, cell: TableCell) -> Self {
        self.cells.push(cell);
        self
    }

    pub fn cells(mut self, cells: impl IntoIterator<Item = TableCell>) -> Self {
        self.cells.extend(cells);
        self
    }

    pub fn selected(mut self, selected: impl Into<Binding<bool>>) -> Self {
        self.selected = selected.into();
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn id(&self) -> &T {
        &self.id
    }

    pub fn cells_ref(&self) -> &[TableCell] {
        &self.cells
    }
}

type TableSelectHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

pub struct TableView<T, A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow<T>>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_select: Option<TableSelectHandler<T, A>>,
    style: TableViewStyle,
    max_body_height: Option<f32>,
    zebra: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Default for TableView<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> LayoutExt for TableView<T, A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> TableView<T, A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            columns: Vec::new(),
            rows: Vec::new(),
            selected: None,
            bound_selected: None,
            disabled: Binding::Static(false),
            on_select: None,
            style: TableViewStyle::default(),
            max_body_height: None,
            zebra: true,
        }
    }

    pub fn column(mut self, column: TableColumn) -> Self {
        self.columns.push(column);
        self
    }

    pub fn row(mut self, row: TableRow<T>) -> Self {
        self.rows.push(row);
        self
    }

    pub fn rows(mut self, rows: impl IntoIterator<Item = TableRow<T>>) -> Self {
        self.rows.extend(rows);
        self
    }

    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self.bound_selected = None;
        self
    }

    pub fn bind_selected(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound_selected = Some(signal);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn on_select(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn table_style(mut self, style: TableViewStyle) -> Self {
        self.style = style;
        self
    }

    pub fn table_size(mut self, size: TableViewSize) -> Self {
        self.style = TableViewStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn max_body_height(mut self, height: f32) -> Self {
        self.max_body_height = height.is_finite().then_some(height.max(0.0));
        self
    }

    pub fn zebra(mut self, enabled: bool) -> Self {
        self.zebra = enabled;
        self
    }

    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    pub fn height(mut self, value: impl Into<Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    pub fn min_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    pub fn max_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.max_width = value.into();
        self
    }

    pub fn min_height(mut self, value: impl Into<Length>) -> Self {
        self.layout.min_height = value.into();
        self
    }

    pub fn max_height(mut self, value: impl Into<Length>) -> Self {
        self.layout.max_height = value.into();
        self
    }

    pub fn fill(mut self) -> Self {
        self.layout.width = Length::Fill;
        self.layout.height = Length::Fill;
        self
    }

    pub fn fill_width(mut self) -> Self {
        self.layout.width = Length::Fill;
        self
    }

    pub fn fill_height(mut self) -> Self {
        self.layout.height = Length::Fill;
        self
    }

    pub fn margin(mut self, value: f32) -> Self {
        self.layout = self.layout.margin(value);
        self
    }

    pub fn padding(mut self, value: f32) -> Self {
        self.layout = self.layout.padding(value);
        self
    }

    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }

    pub fn flex_grow_by(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_grow(value);
        self
    }

    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_shrink(value);
        self
    }

    pub fn flex_basis(mut self, value: impl Into<Length>) -> Self {
        self.flex_item = self.flex_item.flex_basis(value);
        self
    }

    pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
        self.flex_item = self.flex_item.align_self(value);
        self
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for TableView<T, A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(TableViewComponent {
                layout: self.layout,
                columns: self.columns,
                rows: self.rows,
                selected: self.selected,
                bound_selected: self.bound_selected,
                disabled: self.disabled,
                on_select: self.on_select,
                style: self.style,
                max_body_height: self.max_body_height,
                zebra: self.zebra,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct TableViewComponent<T, A> {
    layout: LayoutStyle,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow<T>>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_select: Option<TableSelectHandler<T, A>>,
    style: TableViewStyle,
    max_body_height: Option<f32>,
    zebra: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for TableViewComponent<T, A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(TableViewWidget {
            layout: self.layout,
            columns: self.columns.clone(),
            rows: self.rows.clone(),
            selected: self.selected.clone(),
            bound_selected: self.bound_selected.clone(),
            disabled: self.disabled.clone(),
            on_select: self.on_select.clone(),
            style: self.style.clone(),
            max_body_height: self.max_body_height,
            zebra: self.zebra,
            scroll: context.signal(ScrollState::new()),
            active_index: context.signal(None),
            metrics: context.signal(TableMetrics::default()),
            behavior: ScrollBehavior::new(ScrollAxes::BOTH),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct TableMetrics {
    viewport: Size,
    content: Size,
}

#[derive(Debug, Clone)]
struct ResolvedColumn {
    label: String,
    x: f32,
    width: f32,
    align: TableAlign,
}

struct TableGeometry {
    columns: Vec<ResolvedColumn>,
    content_width: f32,
    header_rect: Rect,
    body_rect: Rect,
}

#[derive(Debug, Clone, Copy)]
struct CellPaint {
    rect: Rect,
    align: TableAlign,
    opacity: f32,
    disabled: bool,
}

struct TableViewWidget<T, A> {
    layout: LayoutStyle,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow<T>>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_select: Option<TableSelectHandler<T, A>>,
    style: TableViewStyle,
    max_body_height: Option<f32>,
    zebra: bool,
    scroll: Signal<ScrollState>,
    active_index: Signal<Option<usize>>,
    metrics: Signal<TableMetrics>,
    behavior: ScrollBehavior,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for TableViewWidget<T, A> {
    fn debug_name(&self) -> &'static str {
        "TableView"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic_columns = self.resolve_columns_layout(ctx, constraints.max_w);
        let content_width = sum_column_widths(&intrinsic_columns);
        let body_content_height = self.body_content_height();
        let body_intrinsic = self
            .max_body_height
            .map(|max_h| body_content_height.min(max_h))
            .unwrap_or(body_content_height);
        let intrinsic = Size::new(
            content_width.max(self.style.min_column_width),
            self.style.header_height + body_intrinsic,
        );
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let final_columns = self.resolve_columns_layout(ctx, size.w);
        let final_content_width = sum_column_widths(&final_columns).max(size.w);
        let body_h = (size.h - self.style.header_height).max(0.0);
        let metrics = TableMetrics {
            viewport: Size::new(size.w, body_h),
            content: Size::new(final_content_width, body_content_height),
        };
        self.metrics.set(metrics);
        let out = self.scroll.read().clamp_to(
            ScrollMetrics::new(metrics.viewport, metrics.content),
            ScrollAxes::BOTH,
        );
        if out.changed {
            self.scroll.set(out.state());
        }

        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: self.visual_bounds(paint_bounds),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let disabled = self.disabled.read();
        let geometry = self.geometry_paint(ctx, bounds);
        let scroll = self.scroll.read().offset;

        for shadow in self.style.shadows.iter().copied().filter(|s| !s.inset) {
            if shadow.color.a > 0.0 {
                ctx.push(DrawCmd::BoxShadow(DrawBoxShadow {
                    rect: bounds,
                    radius: self.style.radius,
                    shadow,
                }));
            }
        }

        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius: self.style.radius.tl,
            color: self.style.background,
        }));

        self.paint_header(ctx, &geometry, scroll.x, disabled);
        self.paint_body(ctx, &geometry, scroll, disabled);

        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: self.style.radius,
            border: self.style.border,
        }));

        if ctx.is_focused() && !disabled {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: self.style.radius,
                border: self.style.focus_ring,
            }));
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        match event {
            Event::Pointer(PointerEvent::Wheel { pos, delta, .. }) => {
                let body = self.body_rect(bounds);
                if !body.contains(pos.x, pos.y) {
                    return;
                }
                let metrics = self.metrics.read();
                let out = self.scroll.read().scroll_by(
                    self.behavior.wheel_delta(*delta),
                    ScrollMetrics::new(metrics.viewport, metrics.content),
                    ScrollAxes::BOTH,
                );
                if out.changed {
                    self.scroll.set(out.state());
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                if let Some(idx) = self.row_index_at(bounds, pos.y) {
                    if self.set_active(Some(idx), ctx) {
                        ctx.stop_propagation();
                    }
                    if self.select_row(ctx, idx) {
                        ctx.stop_propagation();
                    }
                }
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, key);
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() || self.rows.iter().all(|row| row.disabled.read()) {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> TableViewWidget<T, A> {
    fn resolve_columns_layout(
        &self,
        ctx: &mut LayoutCtx<'_>,
        available_width: f32,
    ) -> Vec<ResolvedColumn> {
        let text_system = ctx.text_system.as_deref_mut();
        self.resolve_columns_with(text_system, available_width)
    }

    fn geometry_paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect) -> TableGeometry {
        let columns = {
            let text_system = ctx.text_system.as_deref_mut();
            self.resolve_columns_with(text_system, bounds.w)
        };
        let content_width = sum_column_widths(&columns).max(bounds.w);
        TableGeometry {
            columns,
            content_width,
            header_rect: Rect::new(
                bounds.x,
                bounds.y,
                bounds.w,
                self.style.header_height.min(bounds.h),
            ),
            body_rect: self.body_rect(bounds),
        }
    }

    fn resolve_columns_with(
        &self,
        mut text_system: Option<&mut TextSystem>,
        available_width: f32,
    ) -> Vec<ResolvedColumn> {
        let mut base_widths = Vec::with_capacity(self.columns.len());
        let mut flex_total = 0.0f32;
        let mut fixed_auto_total = 0.0f32;

        for (col_idx, column) in self.columns.iter().enumerate() {
            let width = match column.width {
                TableColumnWidth::Fixed(width) => width.max(0.0),
                TableColumnWidth::Auto => {
                    self.auto_column_width(text_system.as_deref_mut(), col_idx, column)
                }
                TableColumnWidth::Flex(weight) => {
                    flex_total += weight.max(0.0);
                    self.style.min_column_width
                }
            };
            if !matches!(column.width, TableColumnWidth::Flex(_)) {
                fixed_auto_total += width;
            }
            base_widths.push(width.max(self.style.min_column_width));
        }

        if flex_total > 0.0 && available_width.is_finite() {
            let flex_indices = self.columns.iter().enumerate().filter_map(|(idx, col)| {
                if let TableColumnWidth::Flex(weight) = col.width {
                    Some((idx, weight.max(0.0)))
                } else {
                    None
                }
            });
            let remaining = (available_width - fixed_auto_total).max(0.0);
            for (idx, weight) in flex_indices {
                let share = if flex_total > 0.0 {
                    remaining * (weight / flex_total)
                } else {
                    0.0
                };
                base_widths[idx] = share.max(self.style.min_column_width);
            }
        }

        let mut x = 0.0;
        self.columns
            .iter()
            .zip(base_widths)
            .map(|(column, width)| {
                let out = ResolvedColumn {
                    label: column.label.clone(),
                    x,
                    width,
                    align: column.align,
                };
                x += width;
                out
            })
            .collect()
    }

    fn auto_column_width(
        &self,
        mut text_system: Option<&mut TextSystem>,
        col_idx: usize,
        column: &TableColumn,
    ) -> f32 {
        let mut width = measure_text_system(
            text_system.as_deref_mut(),
            &column.label,
            self.style.header_text,
        )
        .unwrap_or(48.0);
        for row in &self.rows {
            if let Some(cell) = row.cells.get(col_idx) {
                width = width.max(self.measure_cell(text_system.as_deref_mut(), cell));
            }
        }
        if col_idx == 0 && self.rows.iter().any(|row| row.leading_icon.is_some()) {
            width += self.style.icon_size + self.style.icon_gap;
        }
        width + self.style.cell_padding_x * 2.0
    }

    fn measure_cell(&self, text_system: Option<&mut TextSystem>, cell: &TableCell) -> f32 {
        match cell.kind {
            TableCellKind::Text => {
                measure_text_system(text_system, &cell.label, self.style.text).unwrap_or(48.0)
            }
            TableCellKind::Muted => {
                measure_text_system(text_system, &cell.label, self.style.muted_text).unwrap_or(48.0)
            }
            TableCellKind::Badge(_) => {
                measure_text_system(text_system, &cell.label, self.style.badge_text).unwrap_or(32.0)
                    + 18.0
            }
            TableCellKind::Progress(_) => self.style.progress_width,
        }
    }

    fn paint_header(
        &self,
        ctx: &mut PaintCtx<'_>,
        geometry: &TableGeometry,
        scroll_x: f32,
        disabled: bool,
    ) {
        let header = geometry.header_rect;
        if header.w <= 0.0 || header.h <= 0.0 {
            return;
        }
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: header,
            color: self.style.header_background,
        }));
        ctx.with_clip(header, |ctx| {
            for col in &geometry.columns {
                let cell = Rect::new(header.x + col.x - scroll_x, header.y, col.width, header.h);
                paint_text_cell(
                    ctx,
                    &col.label,
                    self.style.header_text,
                    cell,
                    col.align,
                    self.style.cell_padding_x,
                    if disabled {
                        self.style.disabled_opacity
                    } else {
                        1.0
                    },
                );
                if cell.right() < header.x || cell.x > header.right() {
                    continue;
                }
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: Rect::new(cell.right() - 1.0, header.y, 1.0, header.h),
                    color: self.style.grid_color,
                }));
            }
        });
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(header.x, header.bottom() - 1.0, header.w, 1.0),
            color: self.style.grid_color,
        }));
    }

    fn paint_body(
        &self,
        ctx: &mut PaintCtx<'_>,
        geometry: &TableGeometry,
        scroll: Offset,
        disabled: bool,
    ) {
        let body = geometry.body_rect;
        if body.w <= 0.0 || body.h <= 0.0 {
            return;
        }
        let active = self.normalized_active_index();
        let selected = self.selected_value();
        ctx.with_clip(body, |ctx| {
            for (idx, row) in self.rows.iter().enumerate() {
                let y = body.y + idx as f32 * self.style.row_height - scroll.y;
                let row_rect = Rect::new(body.x, y, body.w, self.style.row_height);
                if row_rect.bottom() < body.y || row_rect.y > body.bottom() {
                    continue;
                }
                let row_disabled = disabled || row.disabled.read();
                let row_selected = row.selected.read()
                    || selected
                        .as_ref()
                        .is_some_and(|selected| selected == &row.id);
                let is_active = active == Some(idx);
                let opacity = if row_disabled {
                    self.style.disabled_opacity
                } else {
                    1.0
                };
                let bg = if row_selected {
                    self.style.row_selected_background
                } else if is_active {
                    self.style.row_active_background
                } else if self.zebra && idx % 2 == 1 {
                    self.style.row_alt_background
                } else {
                    self.style.row_background
                };
                if bg.a > 0.0 {
                    ctx.push(DrawCmd::Rect(DrawRect {
                        rect: row_rect,
                        color: bg.with_alpha(bg.a * opacity),
                    }));
                }
                self.paint_row_cells(
                    ctx,
                    geometry,
                    row,
                    idx,
                    row_rect,
                    scroll.x,
                    opacity,
                    row_selected,
                    row_disabled,
                );
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: Rect::new(row_rect.x, row_rect.bottom() - 1.0, row_rect.w, 1.0),
                    color: self.style.grid_color,
                }));
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_row_cells(
        &self,
        ctx: &mut PaintCtx<'_>,
        geometry: &TableGeometry,
        row: &TableRow<T>,
        row_idx: usize,
        row_rect: Rect,
        scroll_x: f32,
        opacity: f32,
        selected: bool,
        disabled: bool,
    ) {
        for (col_idx, col) in geometry.columns.iter().enumerate() {
            let cell_rect = Rect::new(
                row_rect.x + col.x - scroll_x,
                row_rect.y,
                col.width,
                row_rect.h,
            );
            if cell_rect.right() < geometry.body_rect.x || cell_rect.x > geometry.body_rect.right()
            {
                continue;
            }
            if col_idx > 0 {
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: Rect::new(cell_rect.x, row_rect.y, 1.0, row_rect.h),
                    color: self
                        .style
                        .grid_color
                        .with_alpha(self.style.grid_color.a * 0.72),
                }));
            }
            let mut content_rect = cell_rect;
            if col_idx == 0 {
                if let Some(icon) = &row.leading_icon {
                    let icon_rect = Rect::new(
                        cell_rect.x + self.style.cell_padding_x,
                        row_rect.y + (row_rect.h - self.style.icon_size) * 0.5,
                        self.style.icon_size,
                        self.style.icon_size,
                    );
                    let tint = if selected {
                        self.style.selected_icon_tint
                    } else {
                        self.style.icon_tint
                    };
                    ctx.push(DrawCmd::Image(DrawImage {
                        rect: icon_rect,
                        icon: icon.clone(),
                        tint: tint.with_alpha(tint.a * opacity),
                        rotation_rad: 0.0,
                    }));
                    content_rect.x += self.style.icon_size + self.style.icon_gap;
                    content_rect.w =
                        (content_rect.w - self.style.icon_size - self.style.icon_gap).max(0.0);
                }
            }
            let Some(cell) = row.cells.get(col_idx) else {
                continue;
            };
            let align = cell.align.unwrap_or(col.align);
            ctx.with_clip(cell_rect, |ctx| {
                self.paint_cell(
                    ctx,
                    cell,
                    CellPaint {
                        rect: content_rect,
                        align,
                        opacity,
                        disabled,
                    },
                );
            });
        }
        if row_idx == 0 {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: Rect::new(
                    row_rect.x,
                    row_rect.y,
                    geometry.content_width.max(row_rect.w),
                    1.0,
                ),
                color: self.style.grid_color,
            }));
        }
    }

    fn paint_cell(&self, ctx: &mut PaintCtx<'_>, cell: &TableCell, paint: CellPaint) {
        match cell.kind {
            TableCellKind::Text => paint_text_cell(
                ctx,
                &cell.label,
                if paint.disabled {
                    self.style.disabled_text
                } else {
                    self.style.text
                },
                paint.rect,
                paint.align,
                self.style.cell_padding_x,
                paint.opacity,
            ),
            TableCellKind::Muted => paint_text_cell(
                ctx,
                &cell.label,
                if paint.disabled {
                    self.style.disabled_text
                } else {
                    self.style.muted_text
                },
                paint.rect,
                paint.align,
                self.style.cell_padding_x,
                paint.opacity,
            ),
            TableCellKind::Badge(tone) => {
                self.paint_badge_cell(ctx, &cell.label, tone, paint);
            }
            TableCellKind::Progress(value) => {
                self.paint_progress_cell(ctx, value, paint);
            }
        }
    }

    fn paint_badge_cell(
        &self,
        ctx: &mut PaintCtx<'_>,
        label: &str,
        tone: BadgeTone,
        paint: CellPaint,
    ) {
        let Some(layout) = layout_text(ctx, label, self.style.badge_text) else {
            return;
        };
        let badge_w = layout.metrics.width.max(10.0) + 18.0;
        let badge_h = 18.0;
        let x = aligned_x(paint.rect, badge_w, paint.align, self.style.cell_padding_x);
        let y = paint.rect.y + (paint.rect.h - badge_h) * 0.5;
        let color =
            tone_color(tone).with_alpha(if paint.disabled { 0.18 } else { 0.28 } * paint.opacity);
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: Rect::new(x, y, badge_w, badge_h),
            radius: badge_h * 0.5,
            color,
        }));
        let baseline = layout
            .lines
            .first()
            .map(|line| line.baseline_y)
            .unwrap_or(0.0);
        ctx.push(DrawCmd::Text(DrawText {
            pos: [
                x + (badge_w - layout.metrics.width) * 0.5,
                y + (badge_h - layout.metrics.height) * 0.5 + baseline,
            ],
            color: self
                .style
                .badge_text
                .color
                .with_alpha(self.style.badge_text.color.a * paint.opacity),
            decoration: ailloli_ui_core::TextDecoration::None,
            layout,
        }));
    }

    fn paint_progress_cell(&self, ctx: &mut PaintCtx<'_>, value: f32, paint: CellPaint) {
        let w = self
            .style
            .progress_width
            .min((paint.rect.w - self.style.cell_padding_x * 2.0).max(0.0));
        let h = self.style.progress_height;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let x = aligned_x(paint.rect, w, paint.align, self.style.cell_padding_x);
        let y = paint.rect.y + (paint.rect.h - h) * 0.5;
        let track = Rect::new(x, y, w, h);
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: track,
            radius: h * 0.5,
            color: self
                .style
                .progress_track
                .with_alpha(self.style.progress_track.a * paint.opacity),
        }));
        let fill_w = w * value.clamp(0.0, 1.0);
        if fill_w > 0.0 {
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(x, y, fill_w, h),
                radius: h * 0.5,
                color: if paint.disabled {
                    self.style.progress_fill.with_alpha(0.36 * paint.opacity)
                } else {
                    self.style
                        .progress_fill
                        .with_alpha(self.style.progress_fill.a * paint.opacity)
                },
            }));
        }
    }

    fn handle_keyboard(&self, ctx: &mut EventCtx<A>, key: &ailloli_ui_core::event::KeyEvent) {
        match &key.key {
            Key::Named(NamedKey::ArrowDown) => self.move_active(ctx, 1),
            Key::Named(NamedKey::ArrowUp) => self.move_active(ctx, -1),
            Key::Named(NamedKey::Home) => {
                let next = self.rows.iter().position(|row| !row.disabled.read());
                if self.set_active(next, ctx) {
                    ctx.stop_propagation();
                }
            }
            Key::Named(NamedKey::End) => {
                let next = self.rows.iter().rposition(|row| !row.disabled.read());
                if self.set_active(next, ctx) {
                    ctx.stop_propagation();
                }
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                let idx = self.normalized_active_index();
                if let Some(idx) = idx {
                    if self.select_row(ctx, idx) {
                        ctx.stop_propagation();
                    }
                }
            }
            _ => {}
        }
    }

    fn move_active(&self, ctx: &mut EventCtx<A>, direction: isize) {
        let next = next_enabled(&self.rows, self.normalized_active_index(), direction);
        if self.set_active(next, ctx) {
            ctx.stop_propagation();
        }
    }

    fn set_active(&self, next: Option<usize>, ctx: &mut EventCtx<A>) -> bool {
        if self.active_index.read() == next {
            return false;
        }
        self.active_index.set(next);
        if let Some(idx) = next {
            self.reveal_row(idx, ctx);
        }
        ctx.request_repaint();
        true
    }

    fn select_row(&self, ctx: &mut EventCtx<A>, idx: usize) -> bool {
        let Some(row) = self.rows.get(idx) else {
            return false;
        };
        if row.disabled.read() {
            return false;
        }
        let mut changed = false;
        if let Some(selected) = &self.bound_selected {
            selected.set(row.id.clone());
            changed = true;
        }
        if let Some(handler) = &self.on_select {
            handler(ctx, row.id.clone());
            changed = true;
        }
        if changed {
            self.active_index.set(Some(idx));
            self.reveal_row(idx, ctx);
            ctx.request_repaint();
        }
        changed
    }

    fn reveal_row(&self, idx: usize, ctx: &mut EventCtx<A>) {
        let metrics = self.metrics.read();
        let row = Rect::new(
            0.0,
            idx as f32 * self.style.row_height,
            metrics.content.w,
            self.style.row_height,
        );
        let out = self.scroll.read().reveal_rect(
            row,
            ScrollMetrics::new(metrics.viewport, metrics.content),
            ScrollAxes::VERTICAL,
        );
        if out.changed {
            self.scroll.set(out.state());
            ctx.request_repaint();
        }
    }

    fn normalized_active_index(&self) -> Option<usize> {
        if let Some(idx) = self.active_index.read() {
            if idx < self.rows.len() && !self.rows[idx].disabled.read() {
                return Some(idx);
            }
        }
        if let Some(selected) = self.selected_value() {
            if let Some(idx) = self
                .rows
                .iter()
                .position(|row| !row.disabled.read() && row.id == selected)
            {
                return Some(idx);
            }
        }
        self.rows.iter().position(|row| !row.disabled.read())
    }

    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    fn row_index_at(&self, bounds: Rect, y: f32) -> Option<usize> {
        let body = self.body_rect(bounds);
        if y < body.y || y > body.bottom() {
            return None;
        }
        let content_y = y - body.y + self.scroll.read().offset.y;
        let idx = (content_y / self.style.row_height).floor() as usize;
        (idx < self.rows.len()).then_some(idx)
    }

    fn body_rect(&self, bounds: Rect) -> Rect {
        let header_h = self.style.header_height.min(bounds.h);
        Rect::new(
            bounds.x,
            bounds.y + header_h,
            bounds.w,
            (bounds.h - header_h).max(0.0),
        )
    }

    fn body_content_height(&self) -> f32 {
        self.rows.len() as f32 * self.style.row_height
    }

    fn visual_bounds(&self, rect: Rect) -> Rect {
        self.style.shadows.iter().fold(rect, |bounds, shadow| {
            union_rect(bounds, shadow.paint_bounds(rect))
        })
    }
}

fn next_enabled<T>(rows: &[TableRow<T>], active: Option<usize>, direction: isize) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let start = active.unwrap_or(if direction >= 0 { 0 } else { rows.len() - 1 });
    for step in 1..=rows.len() {
        let idx = if direction >= 0 {
            (start + step) % rows.len()
        } else {
            (start + rows.len() - (step % rows.len())) % rows.len()
        };
        if !rows[idx].disabled.read() {
            return Some(idx);
        }
    }
    None
}

fn sum_column_widths(columns: &[ResolvedColumn]) -> f32 {
    columns.iter().map(|column| column.width).sum()
}

fn measure_text_system(
    text_system: Option<&mut TextSystem>,
    text: &str,
    style: TextStyle,
) -> Option<f32> {
    text_system.map(|text_system| {
        text_system
            .layout_cached(TextLayoutParams {
                text,
                style,
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            })
            .metrics
            .width
    })
}

fn layout_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
) -> Option<Arc<PreparedTextLayout>> {
    ctx.text_system.as_deref_mut().map(|text_system| {
        text_system.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        })
    })
}

fn paint_text_cell(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    rect: Rect,
    align: TableAlign,
    padding_x: f32,
    opacity: f32,
) {
    let Some(layout) = layout_text(ctx, text, style) else {
        return;
    };
    let x = aligned_x(rect, layout.metrics.width, align, padding_x);
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let y = rect.y + (rect.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style.color.with_alpha(style.color.a * opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}

fn aligned_x(rect: Rect, content_w: f32, align: TableAlign, padding_x: f32) -> f32 {
    match align {
        TableAlign::Start => rect.x + padding_x,
        TableAlign::Center => rect.x + (rect.w - content_w) * 0.5,
        TableAlign::End => rect.right() - padding_x - content_w,
    }
}

fn tone_color(tone: BadgeTone) -> Color {
    let palette = Theme::default().palette();
    match tone {
        BadgeTone::Neutral => palette.text_muted,
        BadgeTone::Accent => palette.accent,
        BadgeTone::Danger => palette.danger,
        BadgeTone::Success => palette.success,
        BadgeTone::Warning => palette.warning,
        BadgeTone::Info => palette.info,
        BadgeTone::Muted => palette.text_muted,
    }
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
