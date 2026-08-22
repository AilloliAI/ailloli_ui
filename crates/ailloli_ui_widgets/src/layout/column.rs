//! Declarative vertical flex container.

use super::flex::{layout_insets_only, layout_sizing_only, FlexWidget};
use super::layout_ext::{finish_view_sized, FlexLayoutExt};
use super::style_wrappers::apply_margin_padding;
use ailloli_ui_core::style::{FlexStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_runtime::component::{IntoView, View};

/// Vertical flex container with ordered retained children.
///
/// Defaults to automatic sizing, zero gap, start alignment, and no children.
/// Margin/padding are realized as outer wrappers while sizing remains on the
/// inner flex widget.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::{layout::Column, text::Text};
/// let column: Column<()> = Column::new().gap(4.0).child(Text::new("first")).child(Text::new("second"));
/// let _ = column;
/// ```
pub struct Column<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex: FlexStyle,
    pub(crate) flex_item: ailloli_ui_core::style::FlexItemStyle,
    children: Vec<View<A>>,
}

crate::impl_layout_builders!(Column);
crate::impl_flex_builders!(Column);

/// Creates the same empty column as [`Column::new`].
impl<A: 'static> Default for Column<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Column<A> {
    /// Creates an empty vertical flex container.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Column;
    /// let column: Column<()> = Column::new();
    /// let _ = column;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex: FlexStyle::column(),
            flex_item: ailloli_ui_core::style::FlexItemStyle::default(),
            children: Vec::new(),
        }
    }

    /// Appends one child after existing children.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::Column, text::Text};
    /// let column: Column<()> = Column::new().child(Text::new("row"));
    /// let _ = column;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.children.push(child.into_view());
        self
    }
}

/// Converts declarative flex state and children into retained layout wrappers.
impl<A: 'static> IntoView<A> for Column<A> {
    fn into_view(self) -> View<A> {
        let items: Vec<_> = self.children.iter().map(|c| c.flex_item).collect();
        let child_hints: Vec<_> = self.children.iter().map(|c| c.size_hint).collect();
        let widget = FlexWidget {
            layout: layout_sizing_only(self.layout),
            flex: self.flex,
            items,
            child_hints,
        };
        let content = View::node(widget, self.children);
        let content = apply_margin_padding(content, layout_insets_only(self.layout));
        finish_view_sized(
            content,
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}
