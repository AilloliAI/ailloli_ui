use super::flex::{layout_insets_only, layout_sizing_only, FlexWidget};
use super::layout_ext::{finish_view_sized, FlexLayoutExt};
use super::style_wrappers::apply_margin_padding;
use ailloli_ui_core::style::{FlexStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_runtime::component::{IntoView, View};

/// Vertical flex container.
pub struct Column<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex: FlexStyle,
    pub(crate) flex_item: ailloli_ui_core::style::FlexItemStyle,
    children: Vec<View<A>>,
}

crate::impl_layout_builders!(Column);
crate::impl_flex_builders!(Column);

impl<A: 'static> Default for Column<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Column<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex: FlexStyle::column(),
            flex_item: ailloli_ui_core::style::FlexItemStyle::default(),
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.children.push(child.into_view());
        self
    }
}

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
