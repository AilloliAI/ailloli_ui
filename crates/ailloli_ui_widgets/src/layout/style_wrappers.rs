//! Composition helper that realizes declarative margin and padding as nodes.

use super::{Margin, Padding};
use ailloli_ui_core::style::LayoutStyle;
use ailloli_ui_core::EdgeInsets;
use ailloli_ui_runtime::component::View;

/// Applies margin/padding only (sizing lives on the leaf widget).
///
/// Non-default padding becomes the inner wrapper and non-default margin the
/// outer wrapper. Default insets return the original view without extra nodes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{IntoView, View};
/// use ailloli_ui_widgets::{layout::Container, text::Text};
/// let wrapped: View<()> = Container::new().padding(8.0).margin(4.0).child(Text::new("content")).into_view();
/// let _ = wrapped; // conversion applies the same private wrapper ordering.
/// ```
pub fn apply_margin_padding<A: 'static>(mut content: View<A>, layout: LayoutStyle) -> View<A> {
    if layout.padding != EdgeInsets::default() {
        content = View::node(Padding::new(layout.padding), vec![content]);
    }

    if layout.margin != EdgeInsets::default() {
        content = View::node(Margin::new(layout.margin), vec![content]);
    }

    content
}
