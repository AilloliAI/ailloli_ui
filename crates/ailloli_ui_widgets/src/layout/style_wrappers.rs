use super::{Margin, Padding};
use ailloli_ui_core::style::LayoutStyle;
use ailloli_ui_core::EdgeInsets;
use ailloli_ui_runtime::component::View;

/// Applies margin/padding only (sizing lives on the leaf widget).
pub fn apply_margin_padding<A: 'static>(mut content: View<A>, layout: LayoutStyle) -> View<A> {
    if layout.padding != EdgeInsets::default() {
        content = View::node(Padding::new(layout.padding), vec![content]);
    }

    if layout.margin != EdgeInsets::default() {
        content = View::node(Margin::new(layout.margin), vec![content]);
    }

    content
}
