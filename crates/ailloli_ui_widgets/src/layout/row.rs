//! Declarative horizontal flex container.

use super::flex::{layout_insets_only, layout_sizing_only, FlexWidget};
use super::layout_ext::{finish_view_sized, FlexLayoutExt};
use super::style_wrappers::apply_margin_padding;
use ailloli_ui_core::style::{FlexStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_runtime::component::{IntoView, View};

/// Horizontal flex container with ordered retained children.
///
/// Defaults to automatic sizing, zero gap, start alignment, and no children.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::{layout::Row, text::Text};
/// let row: Row<()> = Row::new().gap(4.0).child(Text::new("left")).child(Text::new("right"));
/// let _ = row;
/// ```
pub struct Row<A = ()> {
    /// Outer logical sizing policy.
    pub(crate) layout: LayoutStyle,
    /// Horizontal direction, gap, and cross-axis alignment.
    pub(crate) flex: FlexStyle,
    /// Parent-flex participation metadata.
    pub(crate) flex_item: ailloli_ui_core::style::FlexItemStyle,
    /// Ordered retained children laid out from left to right.
    children: Vec<View<A>>,
}

crate::impl_layout_builders!(Row);
crate::impl_flex_builders!(Row);

/// Creates the same empty row as [`Row::new`].
impl<A: 'static> Default for Row<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Row<A> {
    /// Creates an empty horizontal flex container.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Row;
    /// let row: Row<()> = Row::new();
    /// let _ = row;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex: FlexStyle::row(),
            flex_item: ailloli_ui_core::style::FlexItemStyle::default(),
            children: Vec::new(),
        }
    }

    /// Appends one child after existing children.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::Row, text::Text};
    /// let row: Row<()> = Row::new().child(Text::new("cell"));
    /// let _ = row;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.children.push(child.into_view());
        self
    }
}

/// Converts declarative flex state and children into retained layout wrappers.
impl<A: 'static> IntoView<A> for Row<A> {
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
