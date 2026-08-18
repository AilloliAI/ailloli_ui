//! Declarative UI: views, components, and reactive bindings.

pub mod binding;
pub mod context;
pub mod node;
pub mod props;
pub mod signal;
pub mod state;
pub mod view;

pub use binding::Binding;
pub use context::Context;
pub use node::{Node, NodeKind};
pub use props::Props;
pub use signal::{Memo, Signal};
pub use state::State;
pub use view::{Component, ComponentNode, IntoView, IntoViewKeyExt, View, ViewKind, Widget};

/// Builds a component [`View`] from cloneable props and a render function.
pub fn component<A: 'static, P: Props>(
    props: P,
    render: fn(&mut Context<A>, P) -> View<A>,
) -> View<A> {
    Component::new(props, render).into_view()
}
