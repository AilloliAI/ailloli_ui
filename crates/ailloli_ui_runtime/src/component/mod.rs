//! Declarative UI: views, components, and reactive bindings.

/// Binding implementation details.
pub mod binding;
/// Context implementation details.
pub mod context;
/// Node implementation details.
pub mod node;
/// Props implementation details.
pub mod props;
/// Hidden retained reactive dependency contracts shared inside the framework.
#[doc(hidden)]
pub mod reactive;
/// Signal implementation details.
pub mod signal;
/// State implementation details.
pub mod state;
/// View implementation details.
pub mod view;

pub use binding::Binding;
pub use context::Context;
pub use node::{Node, NodeKind};
pub use props::Props;
pub use signal::{Memo, Signal};
pub use state::State;
pub use view::{Component, ComponentNode, IntoView, IntoViewKeyExt, View, ViewKind, Widget};

/// Builds a component [`View`] from cloneable props and a render function.
///
/// The render function is not invoked until reconciliation builds the component.
/// Props are cloned on every build. This is shorthand for
/// `Component::new(props, render).into_view()`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{component, Context, View, ViewKind};
/// fn render(_: &mut Context<()>, _: u8) -> View<()> { View::empty() }
/// let view = component::<(), u8>(3, render);
/// assert!(matches!(view.kind, ViewKind::Component(_)));
/// ```
pub fn component<A: 'static, P: Props>(
    props: P,
    render: impl Fn(&mut Context<A>, P) -> View<A> + 'static,
) -> View<A> {
    Component::new(props, render).into_view()
}
