//! Retained view contract and type-erased view adapters.

use crate::component::context::Context;
use crate::input::EventCtx;
use crate::input::{ActivationPolicy, FocusPolicy, HoverCursorRole, InputRole};
use crate::layout::LayoutEngine;
use crate::layout::LayoutResult;
use crate::layout::{LayoutChild, LayoutCtx};
use crate::scene::PaintCtx;
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Point, Rect};
use ailloli_ui_core::style::FlexItemStyle;
use ailloli_ui_core::style::LayoutSizeHint;
use std::rc::Rc;

/// Declarative view tree node (widget, component, or empty).
///
/// A view is transient input to reconciliation. Keys are optional owned UTF-8
/// strings, children preserve insertion order, and flex/size metadata matters
/// only when interpreted by a compatible parent widget. Cloning shares widget
/// and component implementations through `Rc` but recursively clones metadata
/// and the child vector.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{View, ViewKind};
/// let view = View::<()>::empty().key("root");
/// assert_eq!(view.key_ref(), Some("root"));
/// assert!(matches!(view.kind, ViewKind::Empty));
/// ```
pub struct View<A> {
    /// Stable identity for reconciliation (see `IntoViewKeyExt::key`).
    pub key: Option<String>,
    /// Empty, widget, or component payload.
    pub kind: ViewKind<A>,
    /// Direct declarative children in reconciliation and paint order.
    pub children: Vec<View<A>>,
    /// Flex item style when this view is a direct child of `Row` / `Column`.
    pub flex_item: FlexItemStyle,
    /// Declarative width/height from builders; used by parent flex for main-axis `Fill`.
    pub size_hint: LayoutSizeHint,
}

/// Provides the operations defined for `View<A>`.
impl<A> View<A> {
    /// Borrows the reconciliation key as `&str` without allocating.
    ///
    /// `None` means positional reconciliation; `Some("")` is a real empty key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::View;
    /// let view = View::<()>::empty().key(String::from("item-1"));
    /// assert_eq!(view.key_ref(), Some("item-1"));
    /// ```
    pub fn key_ref(&self) -> Option<&str> {
        self.key.as_deref()
    }
}

/// Implements the `Clone` contract for `View<A>`.
impl<A> Clone for View<A> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            kind: self.kind.clone(),
            children: self.children.clone(),
            flex_item: self.flex_item,
            size_hint: self.size_hint,
        }
    }
}

/// Discriminant of a [`View`] node.
///
/// Widget and component variants share their implementation through a
/// UI-thread-local `Rc`; cloning a kind does not clone the underlying object.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::ViewKind;
/// let kind: ViewKind<()> = ViewKind::Empty;
/// assert!(matches!(kind, ViewKind::Empty));
/// ```
pub enum ViewKind<A> {
    /// Placeholder with no content.
    Empty,
    /// Built-in or custom widget implementing [`Widget`].
    Widget(Rc<dyn Widget<A>>),
    /// Stateful component with a render function.
    Component(Rc<dyn ComponentNode<A>>),
}

/// Implements the `Clone` contract for `ViewKind<A>`.
impl<A> Clone for ViewKind<A> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Widget(widget) => Self::Widget(widget.clone()),
            Self::Component(component) => Self::Component(component.clone()),
        }
    }
}

/// Provides the operations defined for `View<A>`.
impl<A> View<A> {
    /// Creates an unkeyed empty view with no children and default metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{View, ViewKind};
    /// let view = View::<()>::empty();
    /// assert!(view.key.is_none() && view.children.is_empty());
    /// assert!(matches!(view.kind, ViewKind::Empty));
    /// ```
    pub fn empty() -> Self {
        Self {
            key: None,
            kind: ViewKind::Empty,
            children: Vec::new(),
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    /// Wraps a widget in an unkeyed view with no declarative children.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Rect};
    /// use ailloli_ui_runtime::component::{View, Widget};
    /// use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// struct W;
    /// impl Widget<()> for W {
    ///     fn debug_name(&self) -> &'static str { "W" }
    ///     fn layout(&self, _: &mut LayoutEngine<'_, ()>, _: &mut LayoutCtx<'_>, _: &mut [LayoutChild], _: Constraints) -> LayoutResult { LayoutResult::empty() }
    ///     fn paint(&self, _: &mut PaintCtx<'_>, _: Rect, _: &LayoutResult) {}
    /// }
    /// assert!(View::leaf(W).children.is_empty());
    /// ```
    pub fn leaf(widget: impl Widget<A> + 'static) -> Self {
        Self {
            key: None,
            kind: ViewKind::Widget(Rc::new(widget)),
            children: Vec::new(),
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    /// Wraps a widget and an ordered vector of declarative children.
    ///
    /// Children are moved without cloning; no parent pointer or key validation
    /// occurs until reconciliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Rect};
    /// use ailloli_ui_runtime::component::{View, Widget};
    /// use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// struct W;
    /// impl Widget<()> for W {
    ///     fn debug_name(&self) -> &'static str { "W" }
    ///     fn layout(&self, _: &mut LayoutEngine<'_, ()>, _: &mut LayoutCtx<'_>, _: &mut [LayoutChild], _: Constraints) -> LayoutResult { LayoutResult::empty() }
    ///     fn paint(&self, _: &mut PaintCtx<'_>, _: Rect, _: &LayoutResult) {}
    /// }
    /// let view = View::node(W, vec![View::empty(), View::empty()]);
    /// assert_eq!(view.children.len(), 2);
    /// ```
    pub fn node(widget: impl Widget<A> + 'static, children: Vec<View<A>>) -> Self {
        Self {
            key: None,
            kind: ViewKind::Widget(Rc::new(widget)),
            children,
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    /// Wraps a stateful component in an unkeyed view.
    ///
    /// Declarative children start empty because reconciliation obtains the
    /// component's single built child by invoking [`ComponentNode::build`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{ComponentNode, Context, View, ViewKind};
    /// struct C;
    /// impl ComponentNode<()> for C {
    ///     fn build(&self, _: &mut Context<()>) -> View<()> { View::empty() }
    /// }
    /// let view = View::component(C);
    /// assert!(matches!(view.kind, ViewKind::Component(_)));
    /// ```
    pub fn component(component: impl ComponentNode<A> + 'static) -> Self {
        Self {
            key: None,
            kind: ViewKind::Component(Rc::new(component)),
            children: Vec::new(),
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    /// Replaces this view's flex-child metadata and returns it.
    ///
    /// Values have already been normalized only if the caller used
    /// `FlexItemStyle` builders; direct public-field values are preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::FlexItemStyle;
    /// use ailloli_ui_runtime::component::View;
    /// let view = View::<()>::empty().with_flex_item(FlexItemStyle::new().flex_grow(2.0));
    /// assert_eq!(view.flex_item.flex_grow, 2.0);
    /// ```
    pub fn with_flex_item(mut self, flex_item: FlexItemStyle) -> Self {
        self.flex_item = flex_item;
        self
    }

    /// Replaces declarative width/height hints and returns this view.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutSizeHint, Length};
    /// use ailloli_ui_runtime::component::View;
    /// let hint = LayoutSizeHint::new(Length::Fill, Length::px(20.0));
    /// let view = View::<()>::empty().with_size_hint(hint);
    /// assert_eq!(view.size_hint, hint);
    /// ```
    pub fn with_size_hint(mut self, size_hint: LayoutSizeHint) -> Self {
        self.size_hint = size_hint;
        self
    }

    /// Sets an owned reconciliation key and returns this view.
    ///
    /// Empty and duplicate strings are accepted here. Reconciliation compares
    /// exact UTF-8 text and has compatibility behavior for duplicates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::View;
    /// assert_eq!(View::<()>::empty().key("toolbar").key_ref(), Some("toolbar"));
    /// ```
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Wraps a declarative builder into a [`View`] (same as [`IntoView::into_view`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::View;
    /// let view = View::<()>::from(View::empty().key("root"));
    /// assert_eq!(view.key_ref(), Some("root"));
    /// ```
    pub fn from<V: IntoView<A>>(value: V) -> Self {
        value.into_view()
    }
}

/// Converts a declarative widget builder into a runtime [`View`] node.
///
/// Application code returns [`View<A>`] and calls [`.into_view()`](Self::into_view) on the root builder
/// (re-export this trait in the app `view/prelude`, not in `ailloli_ui::prelude`). Framework uses it
/// for `.child`, `Window::content`, and custom widgets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{IntoView, View};
/// let view: View<()> = View::empty().into_view();
/// assert!(view.children.is_empty());
/// ```
pub trait IntoView<A> {
    /// Consumes a builder/value and returns its declarative view tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// let view: View<()> = IntoView::into_view(View::empty());
    /// assert!(view.key.is_none());
    /// ```
    fn into_view(self) -> View<A>;
}

/// Implements the `IntoView<A>` contract for `View<A>`.
impl<A> IntoView<A> for View<A> {
    /// Converts this value into its retained view representation.
    fn into_view(self) -> View<A> {
        self
    }
}

/// Allows `widget.key("id")` instead of `widget.into_view().key("id")`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{IntoViewKeyExt, View};
/// let view: View<()> = IntoViewKeyExt::key(View::empty(), "item");
/// assert_eq!(view.key_ref(), Some("item"));
/// ```
pub trait IntoViewKeyExt<A>: IntoView<A> + Sized {
    /// Converts `self`, assigns the owned key, and returns the view.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{IntoViewKeyExt, View};
    /// let view: View<()> = IntoViewKeyExt::key(View::empty(), String::from("x"));
    /// assert_eq!(view.key_ref(), Some("x"));
    /// ```
    fn key(self, key: impl Into<String>) -> View<A> {
        self.into_view().key(key)
    }
}

/// Implements the `IntoViewKeyExt<A>` contract for `T where T: IntoView<A>`.
impl<T, A> IntoViewKeyExt<A> for T where T: IntoView<A> {}

/// Widget contract for the `View` tree (separate from `layout::Widget` retained nodes).
///
/// Callbacks run synchronously on the UI thread and panics propagate. Layout
/// and paint use logical pixels. Implementations must not retain borrowed
/// engines, contexts, child slices, events, bounds, or results.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, Rect};
/// use ailloli_ui_runtime::component::Widget;
/// use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
/// use ailloli_ui_runtime::scene::PaintCtx;
/// struct Spacer;
/// impl Widget<()> for Spacer {
///     fn debug_name(&self) -> &'static str { "Spacer" }
///     fn layout(&self, _: &mut LayoutEngine<'_, ()>, _: &mut LayoutCtx<'_>, _: &mut [LayoutChild], _: Constraints) -> LayoutResult { LayoutResult::empty() }
///     fn paint(&self, _: &mut PaintCtx<'_>, _: Rect, _: &LayoutResult) {}
/// }
/// assert_eq!(Spacer.debug_name(), "Spacer");
/// ```
pub trait Widget<A>: 'static {
    /// Returns a stable, non-sensitive static name for diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Rect};
    /// use ailloli_ui_runtime::component::Widget;
    /// use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// use ailloli_ui_runtime::scene::PaintCtx;
    /// struct W;
    /// impl Widget<()> for W {
    ///     fn debug_name(&self) -> &'static str { "W" }
    ///     fn layout(&self, _: &mut LayoutEngine<'_, ()>, _: &mut LayoutCtx<'_>, _: &mut [LayoutChild], _: Constraints) -> LayoutResult { LayoutResult::empty() }
    ///     fn paint(&self, _: &mut PaintCtx<'_>, _: Rect, _: &LayoutResult) {}
    /// }
    /// assert_eq!(W.debug_name(), "W");
    /// ```
    fn debug_name(&self) -> &'static str;

    /// Revision of widget-owned inputs that can change its layout result.
    ///
    /// Runtime-created signals normally invalidate their owning element. This
    /// hook also covers externally created reactive bindings when a layout pass
    /// is requested for another reason, without invalidating stable siblings.
    /// Implementations must return a cheap, stable and monotone value.
    /// The default is the zero sentinel for no external dependency.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// assert_eq!(W.layout_dependency_revision(), 0);
    /// ```
    fn layout_dependency_revision(&self) -> u64 {
        0
    }

    /// Measures and positions this widget's direct retained children.
    ///
    /// The returned child vector must correspond by index to `children` for
    /// commit, paint, and hit testing. Implementations define constraint and
    /// non-finite handling and may recursively call [`LayoutChild::layout`].
    /// [`LayoutCtx::layout_pass`] distinguishes speculative measurement from
    /// authoritative allocation. Implementations must not persist effects
    /// derived from geometry during a measurement pass.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, Rect, Scale};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::element::ElementTree;
    /// # use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// let mut tree = ElementTree::<()>::new();
    /// let mut engine = LayoutEngine::new(&mut tree);
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// assert_eq!(W.layout(&mut engine, &mut ctx, &mut [], Constraints::tight(1.0, 1.0)).size.w, 0.0);
    /// ```
    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult;

    /// Emits base draw commands for absolute logical-pixel `bounds`.
    ///
    /// Base paint runs before descendants. `layout` is the cached result from
    /// this element's layout pass and may contain a reusable artifact.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// let mut ctx = PaintCtx::new();
    /// W.paint(&mut ctx, Rect::new(0.0, 0.0, 1.0, 1.0), &LayoutResult::empty());
    /// assert_eq!(ctx.layers[0].cmds.len(), 0);
    /// ```
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult);

    /// Observes newly committed absolute bounds after layout.
    ///
    /// The default does nothing. It runs only when geometry/layout changed and
    /// before descendants are committed; it should not mutate tree topology.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, Rect, Scale};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// W.layout_committed(&mut ctx, Rect::new(0.0, 0.0, 1.0, 1.0), &LayoutResult::empty());
    /// ```
    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Emits top-level paint commands after this widget's descendants.
    ///
    /// The default does nothing. Use [`PaintCtx::push_overlay`] for commands
    /// that must be globally ordered after base content.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// let mut ctx = PaintCtx::new();
    /// W.paint_overlay(&mut ctx, Rect::new(0.0, 0.0, 1.0, 1.0), &LayoutResult::empty());
    /// assert!(ctx.overlay_layers.is_empty());
    /// ```
    fn paint_overlay(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Handles one routed event for absolute bounds and cached layout.
    ///
    /// The default does nothing and leaves propagation enabled. Implementations
    /// may dispatch actions, invalidate work, or stop bubbling through `ctx`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, ElementId, Event, Rect};
    /// # use ailloli_ui_core::event::FocusEvent;
    /// # use ailloli_ui_runtime::app::RuntimeHandle;
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::input::EventCtx;
    /// # use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// let mut ctx = EventCtx::new(RuntimeHandle::new(), ElementId(1));
    /// W.event(&mut ctx, &Event::Focus(FocusEvent::new(true)), Rect::new(0.0,0.0,1.0,1.0), &LayoutResult::empty());
    /// assert!(!ctx.is_propagation_stopped());
    /// ```
    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    /// Returns whether the widget may receive keyboard focus.
    ///
    /// The default is [`FocusPolicy::NotFocusable`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::input::FocusPolicy;
    /// # use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// assert_eq!(W.focus_policy(), FocusPolicy::NotFocusable);
    /// ```
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }

    /// Policy used for provider-neutral focus-only pointer gestures.
    ///
    /// The default inherits from the parent; the input router falls back to
    /// suppression at the root so custom action widgets are safe by default.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints, Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::input::ActivationPolicy;
    /// # use ailloli_ui_runtime::layout::{LayoutChild,LayoutCtx,LayoutEngine,LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// assert_eq!(W.activation_policy(), ActivationPolicy::Inherit);
    /// ```
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::Inherit
    }

    /// Returns provider-neutral text-input semantics.
    ///
    /// The default is [`InputRole::None`] and does not imply focusability.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints,Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::input::InputRole;
    /// # use ailloli_ui_runtime::layout::{LayoutChild,LayoutCtx,LayoutEngine,LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// assert_eq!(W.input_role(), InputRole::None);
    /// ```
    fn input_role(&self) -> InputRole {
        InputRole::None
    }

    /// Returns the widget-wide hover cursor role.
    ///
    /// By default text input roles map to `Text` and `None` maps to `Inherit`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints,Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::input::HoverCursorRole;
    /// # use ailloli_ui_runtime::layout::{LayoutChild,LayoutCtx,LayoutEngine,LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// assert_eq!(W.hover_cursor_role(), HoverCursorRole::Inherit);
    /// ```
    fn hover_cursor_role(&self) -> HoverCursorRole {
        match self.input_role() {
            InputRole::TextSingleLine | InputRole::TextMultiLine => HoverCursorRole::Text,
            InputRole::None => HoverCursorRole::Inherit,
        }
    }

    /// Returns the hover cursor role at an absolute logical-pixel position.
    ///
    /// The default ignores position/bounds/layout and delegates to
    /// [`Self::hover_cursor_role`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints,Point,Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::input::HoverCursorRole;
    /// # use ailloli_ui_runtime::layout::{LayoutChild,LayoutCtx,LayoutEngine,LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// assert_eq!(W.hover_cursor_role_at(Rect::new(0.0,0.0,1.0,1.0), &LayoutResult::empty(), Point::new(0.5,0.5)), HoverCursorRole::Inherit);
    /// ```
    fn hover_cursor_role_at(
        &self,
        _bounds: Rect,
        _layout: &LayoutResult,
        _pos: Point,
    ) -> HoverCursorRole {
        self.hover_cursor_role()
    }

    /// Returns the absolute logical-pixel IME candidate/caret rectangle.
    ///
    /// `None`, the default, means the widget does not expose an IME cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// # use ailloli_ui_core::{Constraints,Rect};
    /// # use ailloli_ui_runtime::component::Widget;
    /// # use ailloli_ui_runtime::layout::{LayoutChild,LayoutCtx,LayoutEngine,LayoutResult};
    /// # use ailloli_ui_runtime::scene::PaintCtx;
    /// # struct W;
    /// # impl Widget<()> for W { fn debug_name(&self)->&'static str{"W"} fn layout(&self,_:&mut LayoutEngine<'_,()>,_:&mut LayoutCtx<'_>,_:&mut[LayoutChild],_:Constraints)->LayoutResult{LayoutResult::empty()} fn paint(&self,_:&mut PaintCtx<'_>,_:Rect,_:&LayoutResult){} }
    /// assert_eq!(W.ime_cursor_rect(Rect::new(0.0,0.0,1.0,1.0), &LayoutResult::empty()), None);
    /// ```
    fn ime_cursor_rect(&self, _bounds: Rect, _layout: &LayoutResult) -> Option<Rect> {
        None
    }
}

/// Stateful declarative component contract.
///
/// Each build runs synchronously with a fresh hook cursor and returns exactly
/// one root view. Panics propagate and reconciliation is not transactional.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{ComponentNode, Context, View};
/// struct Empty;
/// impl ComponentNode<()> for Empty {
///     fn build(&self, _: &mut Context<()>) -> View<()> { View::empty() }
/// }
/// ```
pub trait ComponentNode<A>: 'static {
    /// Builds the component's current declarative root view.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::component::{ComponentNode, Context, View};
    /// struct Empty;
    /// impl ComponentNode<()> for Empty { fn build(&self, _: &mut Context<()>) -> View<()> { View::empty() } }
    /// let mut ctx = Context::new(ElementId(1), RuntimeHandle::new());
    /// assert!(Empty.build(&mut ctx).children.is_empty());
    /// ```
    fn build(&self, context: &mut Context<A>) -> View<A>;
}

/// Function-backed component storing cloneable props.
///
/// Every build clones `props` and passes the clone by value to `render`. The
/// function pointer cannot capture an environment; use a custom
/// [`ComponentNode`] when retained captures are needed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{Component, Context, IntoView, View};
/// fn render(_: &mut Context<()>, label: String) -> View<()> { View::empty().key(label) }
/// let view = Component::new(String::from("root"), render).into_view();
/// assert!(matches!(view.kind, ailloli_ui_runtime::component::ViewKind::Component(_)));
/// ```
pub struct Component<P, A> {
    /// Cloneable properties passed by value to every render invocation.
    props: P,
    /// Non-capturing render function used to build the retained view.
    render: fn(&mut Context<A>, P) -> View<A>,
}

impl<P, A> Component<P, A>
where
    P: Clone + 'static,
    A: 'static,
{
    /// Creates a function-backed component without running `render`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{Component, Context, IntoView, View};
    /// fn render(_: &mut Context<()>, _: u8) -> View<()> { View::empty() }
    /// let view = Component::new(7, render).into_view();
    /// assert!(view.children.is_empty());
    /// ```
    pub fn new(props: P, render: fn(&mut Context<A>, P) -> View<A>) -> Self {
        Self { props, render }
    }
}

impl<P, A> ComponentNode<A> for Component<P, A>
where
    P: Clone + 'static,
    A: 'static,
{
    /// Builds the retained view required by this component.
    fn build(&self, context: &mut Context<A>) -> View<A> {
        (self.render)(context, self.props.clone())
    }
}

impl<P, A> IntoView<A> for Component<P, A>
where
    P: Clone + 'static,
    A: 'static,
{
    /// Converts this value into its retained view representation.
    fn into_view(self) -> View<A> {
        View::component(self)
    }
}
