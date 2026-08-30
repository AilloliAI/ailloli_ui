//! Retained, scrollable data table with typed row selection and rich cells.
//!
//! Column widths combine fixed, flex, and content-measured policies. The header
//! remains fixed while the body scrolls on both axes; disabled rows are skipped
//! by pointer and keyboard selection.

use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{
    ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometry,
    ScrollbarGeometrySpec,
};
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
    DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText, Invalidation,
};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, TextSystem, WrapMode};

use super::badge::BadgeTone;
use crate::scrollbar::{thumb_color_for_state, ScrollbarInteraction};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset used to derive [`TableViewStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TableViewSize;
/// assert_eq!(TableViewSize::default(), TableViewSize::Default);
/// assert_ne!(TableViewSize::Compact, TableViewSize::Default);
/// ```
pub enum TableViewSize {
    /// 30-pixel header/rows and 72-pixel minimum columns.
    Compact,
    #[default]
    /// 34-pixel header, 36-pixel rows, and 88-pixel minimum columns.
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Column-width resolution policy.
///
/// Every resolved width is floored by `TableViewStyle::min_column_width`. Raw
/// negative fixed widths and flex weights become zero during resolution; flex
/// columns divide remaining finite width by positive weight.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TableColumnWidth;
/// let widths = [TableColumnWidth::Fixed(120.0), TableColumnWidth::Flex(2.0), TableColumnWidth::Auto];
/// assert_eq!(widths.len(), 3);
/// ```
pub enum TableColumnWidth {
    /// Requested logical-pixel width before the style minimum is applied.
    Fixed(f32),
    /// Nonnegative share of finite remaining table width.
    Flex(f32),
    #[default]
    /// Width of the widest header/cell content plus padding.
    Auto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Horizontal content alignment inside a table cell.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TableAlign;
/// assert_eq!(TableAlign::default(), TableAlign::Start);
/// assert_eq!([TableAlign::Start, TableAlign::Center, TableAlign::End].len(), 3);
/// ```
pub enum TableAlign {
    #[default]
    /// Place content after left padding.
    Start,
    /// Center content without enforcing padding.
    Center,
    /// Place content before right padding.
    End,
}

#[derive(Clone, Debug, PartialEq)]
/// Table surfaces, typography, geometry, selection, and rich-cell appearance.
///
/// Dimensions are logical pixels and opacity is a multiplier. Fields are used as
/// supplied without validation. The default is the regular preset of the default
/// theme.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TableViewStyle;
/// let style = TableViewStyle::default();
/// assert_eq!((style.header_height, style.row_height, style.min_column_width), (34.0, 36.0, 88.0));
/// assert_eq!((style.progress_width, style.progress_height), (82.0, 4.0));
/// ```
pub struct TableViewStyle {
    /// Root table surface.
    pub background: Color,
    /// Fixed header surface.
    pub header_background: Color,
    /// Regular row surface.
    pub row_background: Color,
    /// Odd zebra row surface.
    pub row_alt_background: Color,
    /// Reserved hovered-row surface token.
    pub row_hover_background: Color,
    /// Keyboard-active row surface.
    pub row_active_background: Color,
    /// Selected row surface, taking precedence over active/zebra.
    pub row_selected_background: Color,
    /// Horizontal and vertical separator color.
    pub grid_color: Color,
    /// Root table border.
    pub border: Border,
    /// Focused enabled table border.
    pub focus_ring: Border,
    /// Non-inset shadows painted in order.
    pub shadows: Vec<BoxShadow>,
    /// Primary cell text.
    pub text: TextStyle,
    /// Muted cell text.
    pub muted_text: TextStyle,
    /// Header text.
    pub header_text: TextStyle,
    /// Text used when table or row is disabled.
    pub disabled_text: TextStyle,
    /// Regular leading-row icon tint.
    pub icon_tint: Color,
    /// Leading-row icon tint while selected.
    pub selected_icon_tint: Color,
    /// Badge label text.
    pub badge_text: TextStyle,
    /// Progress track color.
    pub progress_track: Color,
    /// Progress fill color.
    pub progress_fill: Color,
    /// Root table corner radii.
    pub radius: Radius,
    /// Body row height in logical pixels.
    pub row_height: f32,
    /// Fixed header height in logical pixels.
    pub header_height: f32,
    /// Horizontal cell padding in logical pixels.
    pub cell_padding_x: f32,
    /// Minimum resolved column width in logical pixels.
    pub min_column_width: f32,
    /// Leading-row icon side length in logical pixels.
    pub icon_size: f32,
    /// Leading icon/text gap in logical pixels.
    pub icon_gap: f32,
    /// Preferred progress track width in logical pixels.
    pub progress_width: f32,
    /// Progress track height in logical pixels.
    pub progress_height: f32,
    /// Disabled row/table alpha multiplier.
    pub disabled_opacity: f32,
}

impl Default for TableViewStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), TableViewSize::Default)
    }
}

impl TableViewStyle {
    /// Derives table style from `theme` and density.
    ///
    /// Compact uses row/header heights `30/30`, padding `8`, 12-pixel body text,
    /// and minimum column width `72`; default uses `36/34`, `10`, `13`, and `88`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{TableViewSize, TableViewStyle};
    /// let style = TableViewStyle::from_theme(Theme::default(), TableViewSize::Compact);
    /// assert_eq!((style.row_height, style.header_height, style.min_column_width), (30.0, 30.0, 72.0));
    /// ```
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
/// Header label, width policy, and default cell alignment for one column.
///
/// Labels are stored unchanged. The default is auto width and start alignment.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TableAlign, TableColumn};
/// let column = TableColumn::new("Name").width(160.0).align(TableAlign::Start);
/// let _ = column;
/// ```
pub struct TableColumn {
    /// Header label.
    label: String,
    /// Resolution policy.
    width: TableColumnWidth,
    /// Header and default body-cell alignment.
    align: TableAlign,
}

impl TableColumn {
    /// Creates an auto-width, start-aligned column.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableColumn;
    /// let column = TableColumn::new("Status");
    /// let _ = column;
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            width: TableColumnWidth::Auto,
            align: TableAlign::Start,
        }
    }

    /// Sets fixed width after `f32::max(width, 0.0)` normalization.
    ///
    /// The table style's minimum column width is still applied during layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableColumn;
    /// let column = TableColumn::new("Name").width(160.0);
    /// let _ = column;
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width = TableColumnWidth::Fixed(width.max(0.0));
        self
    }

    /// Sets an exact width policy without builder-time normalization.
    ///
    /// Fixed widths and flex weights are normalized during layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableColumn, TableColumnWidth};
    /// let column = TableColumn::new("Name").column_width(TableColumnWidth::Auto);
    /// let _ = column;
    /// ```
    pub fn column_width(mut self, width: TableColumnWidth) -> Self {
        self.width = width;
        self
    }

    /// Sets flex width with weight normalized by `f32::max(weight, 0.0)`.
    ///
    /// Zero-weight flex columns receive only the style minimum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableColumn;
    /// let column = TableColumn::new("Description").flex(2.0);
    /// let _ = column;
    /// ```
    pub fn flex(mut self, weight: f32) -> Self {
        self.width = TableColumnWidth::Flex(weight.max(0.0));
        self
    }

    /// Restores content-measured auto width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableColumn;
    /// let column = TableColumn::new("Name").width(100.0).auto();
    /// let _ = column;
    /// ```
    pub fn auto(mut self) -> Self {
        self.width = TableColumnWidth::Auto;
        self
    }

    /// Sets header alignment and fallback alignment for cells in this column.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableAlign, TableColumn};
    /// let column = TableColumn::new("Total").align(TableAlign::End);
    /// let _ = column;
    /// ```
    pub fn align(mut self, align: TableAlign) -> Self {
        self.align = align;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
/// Visual representation of a table cell.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{BadgeTone, TableCellKind};
/// let kinds = [TableCellKind::Text, TableCellKind::Muted,
///              TableCellKind::Badge(BadgeTone::Success), TableCellKind::Progress(0.5)];
/// assert_eq!(kinds.len(), 4);
/// ```
pub enum TableCellKind {
    /// Primary text.
    Text,
    /// Muted text.
    Muted,
    /// Pill badge with the supplied semantic tone.
    Badge(BadgeTone),
    /// Progress track whose fill is clamped to `0.0..=1.0` when painted.
    Progress(f32),
}

#[derive(Clone, Debug, PartialEq)]
/// Cell content and optional alignment override.
///
/// A missing override inherits its [`TableColumn`] alignment. Extra row cells
/// beyond the column count are ignored; missing cells leave blank columns.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TableAlign, TableCell};
/// let cell = TableCell::text("42").align(TableAlign::End);
/// let _ = cell;
/// ```
pub struct TableCell {
    /// Text or badge label; progress cells store an empty label.
    label: String,
    /// Rendering kind.
    kind: TableCellKind,
    /// Per-cell alignment override.
    align: Option<TableAlign>,
}

impl TableCell {
    /// Creates a primary-text cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableCell;
    /// let cell = TableCell::text("Ready");
    /// let _ = cell;
    /// ```
    pub fn text(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: TableCellKind::Text,
            align: None,
        }
    }

    /// Creates a muted-text cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableCell;
    /// let cell = TableCell::muted("Optional");
    /// let _ = cell;
    /// ```
    pub fn muted(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: TableCellKind::Muted,
            align: None,
        }
    }

    /// Creates a semantic-tone badge cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BadgeTone, TableCell};
    /// let cell = TableCell::badge("Healthy", BadgeTone::Success);
    /// let _ = cell;
    /// ```
    pub fn badge(label: impl Into<String>, tone: BadgeTone) -> Self {
        Self {
            label: label.into(),
            kind: TableCellKind::Badge(tone),
            align: None,
        }
    }

    /// Creates an unlabeled progress cell.
    ///
    /// Finite values are clamped to `0.0..=1.0` during paint; NaN paints only
    /// the track because the computed fill is not positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableCell;
    /// let cell = TableCell::progress(0.75);
    /// let _ = cell;
    /// ```
    pub fn progress(value: f32) -> Self {
        Self {
            label: String::new(),
            kind: TableCellKind::Progress(value),
            align: None,
        }
    }

    /// Overrides the owning column's alignment for this cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableAlign, TableCell};
    /// let cell = TableCell::text("Centered").align(TableAlign::Center);
    /// let _ = cell;
    /// ```
    pub fn align(mut self, align: TableAlign) -> Self {
        self.align = Some(align);
        self
    }
}

#[derive(Clone)]
/// Typed row ID, ordered cells, visual selection, availability, and leading icon.
///
/// Row IDs need not be unique. The per-row `selected` binding is visual only;
/// table-level controlled selection compares IDs and may therefore select every
/// duplicate ID visually.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TableCell, TableRow};
/// let row = TableRow::new(7).cell(TableCell::text("Seven"));
/// assert_eq!(*row.id(), 7);
/// ```
pub struct TableRow<T> {
    /// Value written or emitted on row activation.
    id: T,
    /// Cells in column order.
    cells: Vec<TableCell>,
    /// Independent static or reactive visual-selection flag.
    selected: Binding<bool>,
    /// Static or reactive activation-disabled flag.
    disabled: Binding<bool>,
    /// Optional icon painted only in the first column.
    leading_icon: Option<IconId>,
}

impl<T> TableRow<T> {
    /// Creates an enabled, visually unselected row with no cells or icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableRow;
    /// let row = TableRow::new("row-id");
    /// assert_eq!(row.id(), &"row-id");
    /// ```
    pub fn new(id: T) -> Self {
        Self {
            id,
            cells: Vec::new(),
            selected: Binding::Static(false),
            disabled: Binding::Static(false),
            leading_icon: None,
        }
    }

    /// Appends one cell in column order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableCell, TableRow};
    /// let row = TableRow::new(1).cell(TableCell::text("One"));
    /// assert_eq!(row.cells_ref().len(), 1);
    /// ```
    pub fn cell(mut self, cell: TableCell) -> Self {
        self.cells.push(cell);
        self
    }

    /// Extends cells in iterator order without clearing existing cells.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableCell, TableRow};
    /// let row = TableRow::new(1).cells([TableCell::text("One"), TableCell::muted("Aux")]);
    /// assert_eq!(row.cells_ref().len(), 2);
    /// ```
    pub fn cells(mut self, cells: impl IntoIterator<Item = TableCell>) -> Self {
        self.cells.extend(cells);
        self
    }

    /// Sets an independent static or reactive visual-selection flag.
    ///
    /// This does not update `TableView::selected` or invoke selection callbacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableRow;
    /// let row = TableRow::new(1).selected(true);
    /// let _ = row;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<bool>>) -> Self {
        self.selected = selected.into();
        self
    }

    /// Sets static or reactive row-disabled state.
    ///
    /// Disabled rows use reduced opacity and are skipped by hit testing and
    /// keyboard navigation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableRow;
    /// let row = TableRow::new(1).disabled(true);
    /// let _ = row;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Uses a memo as the reactive row-disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::TableRow;
    /// let row = TableRow::new(1).disabled_signal(Memo::new(|| false));
    /// let _ = row;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets the first-column leading icon, replacing any previous one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::TableRow;
    /// let row = TableRow::new(1).leading_icon(IconId::History);
    /// let _ = row;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Borrows the typed row ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableRow;
    /// let row = TableRow::new(42);
    /// assert_eq!(row.id(), &42);
    /// ```
    pub fn id(&self) -> &T {
        &self.id
    }

    /// Borrows cells in insertion order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableCell, TableRow};
    /// let row = TableRow::new(1).cell(TableCell::text("One"));
    /// assert_eq!(row.cells_ref().len(), 1);
    /// ```
    pub fn cells_ref(&self) -> &[TableCell] {
        &self.cells
    }
}

/// Shared context-aware callback for an activated row ID.
type TableSelectHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

/// Scrollable table with typed row activation and controlled visual selection.
///
/// `T` is cloned when a row is activated; `A` is the application action returned
/// by non-context callbacks. [`Self::bind_selected`] installs writable selection,
/// while [`Self::selected`] is read-only. Activating an enabled row writes and/or
/// invokes configured handlers even when its ID already equals the selection.
/// Overflowing body axes paint overlay scrollbars above the clipped rows while
/// the header remains fixed. Thumb dragging and centered track clicks do not
/// activate rows; a disabled table accepts neither interaction.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TableCell, TableColumn, TableRow, TableView};
/// let table: TableView<i32> = TableView::new()
///     .column(TableColumn::new("Value"))
///     .row(TableRow::new(1).cell(TableCell::text("One")));
/// let _ = table;
/// ```
pub struct TableView<T, A = ()> {
    /// Root layout configured by table layout builders.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation.
    pub(crate) flex_item: FlexItemStyle,
    /// Column definitions in display order.
    columns: Vec<TableColumn>,
    /// Rows in display order.
    rows: Vec<TableRow<T>>,
    /// Optional readable table-level selection.
    selected: Option<Binding<T>>,
    /// Optional writable selection signal.
    bound_selected: Option<Signal<T>>,
    /// Whole-table disabled binding.
    disabled: Binding<bool>,
    /// Row activation callback.
    on_select: Option<TableSelectHandler<T, A>>,
    /// Table appearance and geometry.
    style: TableViewStyle,
    /// Optional nonnegative body viewport height cap in logical pixels.
    max_body_height: Option<f32>,
    /// Whether unselected/inactive odd rows use alternate background.
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
    /// Creates an enabled empty table with zebra stripes enabled.
    ///
    /// It has no columns, rows, selection, handler, or body-height cap.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new();
    /// let _ = table;
    /// ```
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

    /// Appends a column in display order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableColumn, TableView};
    /// let table: TableView<i32> = TableView::new().column(TableColumn::new("Name"));
    /// let _ = table;
    /// ```
    pub fn column(mut self, column: TableColumn) -> Self {
        self.columns.push(column);
        self
    }

    /// Appends one row in display order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableRow, TableView};
    /// let table: TableView<i32> = TableView::new().row(TableRow::new(1));
    /// let _ = table;
    /// ```
    pub fn row(mut self, row: TableRow<T>) -> Self {
        self.rows.push(row);
        self
    }

    /// Extends rows in iterator order without clearing existing rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableRow, TableView};
    /// let table: TableView<i32> = TableView::new().rows([TableRow::new(1), TableRow::new(2)]);
    /// let _ = table;
    /// ```
    pub fn rows(mut self, rows: impl IntoIterator<Item = TableRow<T>>) -> Self {
        self.rows.extend(rows);
        self
    }

    /// Sets a read-only static or reactive selection and clears writable binding.
    ///
    /// Every row with an equal ID paints selected. Row activation can still invoke
    /// `on_select`, but cannot mutate this binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().selected(1);
    /// let _ = table;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self.bound_selected = None;
        self
    }

    /// Binds a writable table-level selection signal.
    ///
    /// Activating an enabled row writes its ID even if equal to the current value,
    /// before invoking the optional callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().bind_selected(State::new(1));
    /// let _ = table;
    /// ```
    pub fn bind_selected(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound_selected = Some(signal);
        self
    }

    /// Sets static or reactive whole-table disabled state.
    ///
    /// Disabled tables are not focusable and ignore input; all rows paint with
    /// disabled text/icon/progress treatment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().disabled(true);
    /// let _ = table;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Uses a memo as the reactive whole-table disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().disabled_signal(Memo::new(|| false));
    /// let _ = table;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Dispatches the application action returned for an activated enabled row.
    ///
    /// The handler runs even when the row ID already equals controlled selection.
    /// Disabled or out-of-range rows emit nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32, i32> = TableView::new().on_select(|id| id);
    /// let _ = table;
    /// ```
    pub fn on_select(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    /// Handles an activated enabled row with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().on_select_ctx(|_ctx, _id| {});
    /// let _ = table;
    /// ```
    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Replaces table style without altering explicit layout values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableView, TableViewStyle};
    /// let table: TableView<i32> = TableView::new().table_style(TableViewStyle::default());
    /// let _ = table;
    /// ```
    pub fn table_style(mut self, style: TableViewStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives style from the default theme and requested density.
    ///
    /// This overwrites every prior table-style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TableView, TableViewSize};
    /// let table: TableView<i32> = TableView::new().table_size(TableViewSize::Compact);
    /// let _ = table;
    /// ```
    pub fn table_size(mut self, size: TableViewSize) -> Self {
        self.style = TableViewStyle::from_theme(Theme::default(), size);
        self
    }

    /// Sets an optional maximum intrinsic body height in logical pixels.
    ///
    /// Finite negatives become `0.0`; finite nonnegative values are retained;
    /// NaN and either infinity clear the cap. Explicit layout height can still
    /// override intrinsic sizing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let capped: TableView<i32> = TableView::new().max_body_height(144.0);
    /// let uncapped: TableView<i32> = TableView::new().max_body_height(f32::NAN);
    /// let _ = (capped, uncapped);
    /// ```
    pub fn max_body_height(mut self, height: f32) -> Self {
        self.max_body_height = height.is_finite().then_some(height.max(0.0));
        self
    }

    /// Enables or disables alternate background on odd unselected/inactive rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().zebra(false);
    /// let _ = table;
    /// ```
    pub fn zebra(mut self, enabled: bool) -> Self {
        self.zebra = enabled;
        self
    }

    /// Replaces the preferred table width.
    ///
    /// Numeric inputs are logical pixels; [`Length::Auto`] preserves intrinsic
    /// sizing and [`Length::Fill`] requests the parent-provided width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().width(420.0);
    /// let _ = table;
    /// ```
    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    /// Replaces the preferred table height.
    ///
    /// Numeric inputs are logical pixels. An explicit height controls the whole
    /// table, including its fixed header, and can override intrinsic body sizing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().height(280.0);
    /// let _ = table;
    /// ```
    pub fn height(mut self, value: impl Into<Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    /// Replaces the minimum-width declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().min_width(240.0);
    /// let _ = table;
    /// ```
    pub fn min_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    /// Replaces the maximum-width declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().max_width(960.0);
    /// let _ = table;
    /// ```
    pub fn max_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.max_width = value.into();
        self
    }

    /// Replaces the minimum-height declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().min_height(120.0);
    /// let _ = table;
    /// ```
    pub fn min_height(mut self, value: impl Into<Length>) -> Self {
        self.layout.min_height = value.into();
        self
    }

    /// Replaces the maximum-height declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().max_height(640.0);
    /// let _ = table;
    /// ```
    pub fn max_height(mut self, value: impl Into<Length>) -> Self {
        self.layout.max_height = value.into();
        self
    }

    /// Requests parent-fill sizing on both axes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().fill();
    /// let _ = table;
    /// ```
    pub fn fill(mut self) -> Self {
        self.layout.width = Length::Fill;
        self.layout.height = Length::Fill;
        self
    }

    /// Requests parent-fill width while preserving the height declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().height(240.0).fill_width();
    /// let _ = table;
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.layout.width = Length::Fill;
        self
    }

    /// Requests parent-fill height while preserving the width declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().width(360.0).fill_height();
    /// let _ = table;
    /// ```
    pub fn fill_height(mut self) -> Self {
        self.layout.height = Length::Fill;
        self
    }

    /// Replaces every outer margin edge with `value` logical pixels.
    ///
    /// The value is stored verbatim, including negative and non-finite input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().margin(8.0);
    /// let _ = table;
    /// ```
    pub fn margin(mut self, value: f32) -> Self {
        self.layout = self.layout.margin(value);
        self
    }

    /// Replaces every inner padding edge with `value` logical pixels.
    ///
    /// The value is stored verbatim, including negative and non-finite input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().padding(12.0);
    /// let _ = table;
    /// ```
    pub fn padding(mut self, value: f32) -> Self {
        self.layout = self.layout.padding(value);
        self
    }

    /// Sets this table's flex-grow weight to one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().flex_grow();
    /// let _ = table;
    /// ```
    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }

    /// Sets the dimensionless flex-grow weight, clamped to at least zero.
    ///
    /// NaN and negative infinity become zero; positive infinity is retained.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().flex_grow_by(2.0);
    /// let _ = table;
    /// ```
    pub fn flex_grow_by(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_grow(value);
        self
    }

    /// Sets the dimensionless flex-shrink weight, clamped to at least zero.
    ///
    /// NaN and negative infinity become zero; positive infinity is retained.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().flex_shrink(1.0);
    /// let _ = table;
    /// ```
    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_shrink(value);
        self
    }

    /// Replaces the preferred main-axis size used by a flex parent.
    ///
    /// Numeric inputs are logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().flex_basis(320.0);
    /// let _ = table;
    /// ```
    pub fn flex_basis(mut self, value: impl Into<Length>) -> Self {
        self.flex_item = self.flex_item.flex_basis(value);
        self
    }

    /// Overrides the flex parent's cross-axis alignment for this table.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::AlignItems;
    /// use ailloli_ui_widgets::controls::TableView;
    /// let table: TableView<i32> = TableView::new().align_self(AlignItems::Center);
    /// let _ = table;
    /// ```
    pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
        self.flex_item = self.flex_item.align_self(value);
        self
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for TableView<T, A> {
    /// Builds the retained component and preserves layout/flex metadata.
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

/// Component-stage table configuration copied into the retained widget.
struct TableViewComponent<T, A> {
    /// Root size, bounds, and inset declarations.
    layout: LayoutStyle,
    /// Column definitions in display order.
    columns: Vec<TableColumn>,
    /// Rows in display order.
    rows: Vec<TableRow<T>>,
    /// Optional readable table-level selection.
    selected: Option<Binding<T>>,
    /// Optional writable table-level selection.
    bound_selected: Option<Signal<T>>,
    /// Whole-table disabled state.
    disabled: Binding<bool>,
    /// Optional activation handler.
    on_select: Option<TableSelectHandler<T, A>>,
    /// Appearance and geometry tokens.
    style: TableViewStyle,
    /// Optional body viewport height cap in logical pixels.
    max_body_height: Option<f32>,
    /// Whether eligible odd rows use alternate background.
    zebra: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for TableViewComponent<T, A> {
    /// Allocates scroll, keyboard-active, and measured-geometry signals.
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
            scrollbar_interaction: context
                .signal_with_invalidation(ScrollbarInteraction::default(), Invalidation::Paint),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Last laid-out body viewport and scrollable content extents.
struct TableMetrics {
    /// Visible body size, excluding the fixed header, in logical pixels.
    viewport: Size,
    /// Total scrollable body size in logical pixels.
    content: Size,
}

#[derive(Debug, Clone)]
/// Display-ready column label and resolved horizontal geometry.
struct ResolvedColumn {
    /// Header label copied from the column definition.
    label: String,
    /// Horizontal content offset from the table's left edge.
    x: f32,
    /// Resolved logical-pixel width after minimum enforcement.
    width: f32,
    /// Header and fallback cell alignment.
    align: TableAlign,
}

/// Per-paint table geometry shared by header and body rendering.
struct TableGeometry {
    /// Resolved columns in display order.
    columns: Vec<ResolvedColumn>,
    /// Total horizontal scroll extent in logical pixels.
    content_width: f32,
    /// Fixed header rectangle in window coordinates.
    header_rect: Rect,
    /// Scrollable body viewport in window coordinates.
    body_rect: Rect,
}

#[derive(Debug, Clone, Copy)]
/// Common geometry and state used while painting one rich cell.
struct CellPaint {
    /// Cell rectangle after first-column icon adjustment.
    rect: Rect,
    /// Effective per-cell horizontal alignment.
    align: TableAlign,
    /// Alpha multiplier, normally one or disabled opacity.
    opacity: f32,
    /// Whether disabled typography/progress treatment applies.
    disabled: bool,
}

/// Retained table widget with reactive state and input/paint behavior.
struct TableViewWidget<T, A> {
    /// Root size, bounds, and inset declarations.
    layout: LayoutStyle,
    /// Column definitions in display order.
    columns: Vec<TableColumn>,
    /// Rows in display order.
    rows: Vec<TableRow<T>>,
    /// Optional readable table-level selection.
    selected: Option<Binding<T>>,
    /// Optional writable table-level selection.
    bound_selected: Option<Signal<T>>,
    /// Whole-table disabled state.
    disabled: Binding<bool>,
    /// Optional activation handler.
    on_select: Option<TableSelectHandler<T, A>>,
    /// Appearance and geometry tokens.
    style: TableViewStyle,
    /// Optional body viewport height cap in logical pixels.
    max_body_height: Option<f32>,
    /// Whether eligible odd rows use alternate background.
    zebra: bool,
    /// Two-axis body scroll state.
    scroll: Signal<ScrollState>,
    /// Keyboard-active row index, if explicitly set.
    active_index: Signal<Option<usize>>,
    /// Last layout's body viewport and content extents.
    metrics: Signal<TableMetrics>,
    /// Wheel/reveal policy, constrained to both axes.
    behavior: ScrollBehavior,
    /// Retained hover and captured overlay-scrollbar gesture.
    scrollbar_interaction: Signal<ScrollbarInteraction>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for TableViewWidget<T, A> {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "TableView"
    }

    /// Resolves columns, root size, body metrics, and clamped scroll state.
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
        let geometries = self.scrollbar_geometries(paint_bounds, metrics);
        let interactive_geometries = if self.disabled.read() {
            Vec::new()
        } else {
            geometries
        };
        let mut interaction = self.scrollbar_interaction.read();
        if interaction.reconcile(ctx.layout_pass(), &interactive_geometries) {
            self.scrollbar_interaction.set(interaction);
        }
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: self.visual_bounds(paint_bounds),
            overlay_hit_bounds: interactive_geometries
                .iter()
                .map(|geometry| geometry.hit_track)
                .collect(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Paints shadows, surfaces, fixed header, scrolling body, and borders.
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
        self.paint_scrollbars(ctx, bounds, disabled);

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

    /// Handles body scrolling, enabled-row activation, and keyboard navigation.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        if matches!(event, Event::Pointer(_)) {
            let metrics = self.metrics.read();
            let geometries = self.scrollbar_geometries(bounds, metrics);
            let current = self.scroll.read();
            let mut interaction = self.scrollbar_interaction.read();
            let response = interaction.handle_event(ctx, event, &geometries);
            if response.state_changed {
                self.scrollbar_interaction.set(interaction);
            }
            if let Some((axis, target)) = response.scroll_to {
                let target = match axis {
                    ScrollbarAxis::Horizontal => Offset::new(target, current.offset.y),
                    ScrollbarAxis::Vertical => Offset::new(current.offset.x, target),
                };
                let outcome = current.scroll_to(
                    target,
                    ScrollMetrics::new(metrics.viewport, metrics.content),
                    ScrollAxes::BOTH,
                );
                if outcome.changed {
                    self.scroll.set(outcome.state());
                }
            }
            if response.repaint {
                ctx.request_repaint();
            }
            if response.consumed {
                ctx.stop_propagation();
                return;
            }
        }
        match event {
            Event::Pointer(PointerEvent::Wheel {
                pos,
                delta,
                modifiers,
                ..
            }) => {
                let body = self.body_rect(bounds);
                if !body.contains(pos.x, pos.y) {
                    return;
                }
                let metrics = self.metrics.read();
                let out = self.scroll.read().scroll_by(
                    self.behavior.wheel_delta_with_modifiers(*delta, *modifiers),
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

    /// Is focusable only when the table and at least one row are enabled.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() || self.rows.iter().all(|row| row.disabled.read()) {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> TableViewWidget<T, A> {
    /// Resolves body-overlay bars for each overflowing axis.
    fn scrollbar_geometries(&self, bounds: Rect, table: TableMetrics) -> Vec<ScrollbarGeometry> {
        let body = self.body_rect(bounds);
        let metrics = ScrollMetrics::new(table.viewport, table.content);
        let max = metrics.max_offset();
        let show_horizontal = max.x > 0.5;
        let show_vertical = max.y > 0.5;
        let mut geometries = Vec::with_capacity(2);
        if show_vertical {
            let reserve = if show_horizontal { 9.0 } else { 0.0 };
            if let Some(geometry) = ScrollbarGeometrySpec::new(
                ScrollbarAxis::Vertical,
                body,
                metrics,
                self.scroll.read(),
            )
            .with_paint_metrics(6.0, 24.0, 3.0)
            .with_end_reserve(reserve)
            .resolve()
            {
                geometries.push(geometry);
            }
        }
        if show_horizontal {
            let reserve = if show_vertical { 9.0 } else { 0.0 };
            if let Some(geometry) = ScrollbarGeometrySpec::new(
                ScrollbarAxis::Horizontal,
                body,
                metrics,
                self.scroll.read(),
            )
            .with_paint_metrics(6.0, 24.0, 3.0)
            .with_end_reserve(reserve)
            .resolve()
            {
                geometries.push(geometry);
            }
        }
        geometries
    }

    /// Paints derived-color table scrollbars above body contents.
    fn paint_scrollbars(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, disabled: bool) {
        let interaction = self.scrollbar_interaction.read();
        for geometry in self.scrollbar_geometries(bounds, self.metrics.read()) {
            let visual = interaction.visual_state(geometry.axis, ctx.is_hovered() && !disabled);
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: geometry.track,
                radius: 3.0,
                color: self.style.grid_color.with_alpha(0.22),
            }));
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: geometry.thumb,
                radius: 3.0,
                color: thumb_color_for_state(self.style.muted_text.color.with_alpha(0.58), visual),
            }));
        }
    }

    /// Resolves column widths using the layout context's optional text system.
    fn resolve_columns_layout(
        &self,
        ctx: &mut LayoutCtx<'_>,
        available_width: f32,
    ) -> Vec<ResolvedColumn> {
        let text_system = ctx.text_system.as_deref_mut();
        self.resolve_columns_with(text_system, available_width)
    }

    /// Resolves columns and fixed/body rectangles for the current paint bounds.
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

    /// Resolves fixed, auto, and flex policies and assigns cumulative x offsets.
    ///
    /// Every result is floored by `min_column_width`. Positive flex weights
    /// divide finite remaining width; without finite available width they retain
    /// the minimum.
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

    /// Measures a header and all present cells, with deterministic fallbacks.
    ///
    /// First-column leading-icon allowance and horizontal padding are added to
    /// the widest measured content.
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

    /// Measures one cell's intrinsic content width or returns kind-specific fallback.
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

    /// Paints the horizontally scrolling header inside its fixed viewport.
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

    /// Paints visible rows with vertical/horizontal scroll offsets and state precedence.
    ///
    /// Selection wins over keyboard-active state, which wins over zebra styling.
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
    /// Paints visible cells, leading icon, grid lines, and the first-row top edge.
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

    /// Dispatches a cell to text, badge, or progress rendering.
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

    /// Paints a measured 18-pixel pill; missing text services paint nothing.
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

    /// Paints a bounded progress track and clamped fill.
    ///
    /// A NaN value leaves the track visible but fails the positive-fill test.
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

    /// Handles arrow, boundary, and activation keys for enabled rows.
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

    /// Moves to the next enabled row with wraparound in the requested direction.
    fn move_active(&self, ctx: &mut EventCtx<A>, direction: isize) {
        let next = next_enabled(&self.rows, self.normalized_active_index(), direction);
        if self.set_active(next, ctx) {
            ctx.stop_propagation();
        }
    }

    /// Stores a changed active index, reveals it, and requests repaint.
    ///
    /// Returns `false` without side effects when `next` already matches.
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

    /// Writes and/or emits an enabled row activation.
    ///
    /// Returns whether a binding or handler was configured. Equal selection IDs
    /// are still written, while missing/disabled rows return `false`.
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

    /// Scrolls vertically just enough to reveal the indexed row.
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

    /// Returns a valid enabled active row, selected match, or first enabled row.
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

    /// Clones the current table-level selection when configured.
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    /// Maps a body-window y coordinate through vertical scroll to a row index.
    fn row_index_at(&self, bounds: Rect, y: f32) -> Option<usize> {
        let body = self.body_rect(bounds);
        if y < body.y || y > body.bottom() {
            return None;
        }
        let content_y = y - body.y + self.scroll.read().offset.y;
        let idx = (content_y / self.style.row_height).floor() as usize;
        (idx < self.rows.len()).then_some(idx)
    }

    /// Removes the header from table bounds and floors body height at zero.
    fn body_rect(&self, bounds: Rect) -> Rect {
        let header_h = self.style.header_height.min(bounds.h);
        Rect::new(
            bounds.x,
            bounds.y + header_h,
            bounds.w,
            (bounds.h - header_h).max(0.0),
        )
    }

    /// Returns row count multiplied by the configured logical-pixel row height.
    fn body_content_height(&self) -> f32 {
        self.rows.len() as f32 * self.style.row_height
    }

    /// Expands paint bounds to include every configured shadow.
    fn visual_bounds(&self, rect: Rect) -> Rect {
        self.style.shadows.iter().fold(rect, |bounds, shadow| {
            union_rect(bounds, shadow.paint_bounds(rect))
        })
    }
}

/// Finds the next enabled row with wraparound, or `None` when none exists.
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

/// Sums resolved logical-pixel widths without additional normalization.
fn sum_column_widths(columns: &[ResolvedColumn]) -> f32 {
    columns.iter().map(|column| column.width).sum()
}

/// Measures unwrapped text when a text system is available.
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

/// Produces a cached unwrapped text layout when painting services are available.
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

/// Paints one vertically centered text layout at the requested alignment.
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

/// Resolves a content origin, applying padding only at start/end alignment.
fn aligned_x(rect: Rect, content_w: f32, align: TableAlign, padding_x: f32) -> f32 {
    match align {
        TableAlign::Start => rect.x + padding_x,
        TableAlign::Center => rect.x + (rect.w - content_w) * 0.5,
        TableAlign::End => rect.right() - padding_x - content_w,
    }
}

/// Maps every badge tone to a color from the default theme palette.
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

/// Returns the smallest axis-aligned rectangle containing both inputs.
fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
