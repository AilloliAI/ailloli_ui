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
pub struct View<A> {
    /// Stable identity for reconciliation (see `IntoViewKeyExt::key`).
    pub key: Option<String>,
    pub kind: ViewKind<A>,
    pub children: Vec<View<A>>,
    /// Flex item style when this view is a direct child of `Row` / `Column`.
    pub flex_item: FlexItemStyle,
    /// Declarative width/height from builders; used by parent flex for main-axis `Fill`.
    pub size_hint: LayoutSizeHint,
}

impl<A> View<A> {
    pub fn key_ref(&self) -> Option<&str> {
        self.key.as_deref()
    }
}

impl<A> Clone for View<A> {
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
pub enum ViewKind<A> {
    /// Placeholder with no content.
    Empty,
    /// Built-in or custom widget implementing [`Widget`].
    Widget(Rc<dyn Widget<A>>),
    /// Stateful component with a render function.
    Component(Rc<dyn ComponentNode<A>>),
}

impl<A> Clone for ViewKind<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Widget(widget) => Self::Widget(widget.clone()),
            Self::Component(component) => Self::Component(component.clone()),
        }
    }
}

impl<A> View<A> {
    pub fn empty() -> Self {
        Self {
            key: None,
            kind: ViewKind::Empty,
            children: Vec::new(),
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    pub fn leaf(widget: impl Widget<A> + 'static) -> Self {
        Self {
            key: None,
            kind: ViewKind::Widget(Rc::new(widget)),
            children: Vec::new(),
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    pub fn node(widget: impl Widget<A> + 'static, children: Vec<View<A>>) -> Self {
        Self {
            key: None,
            kind: ViewKind::Widget(Rc::new(widget)),
            children,
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    pub fn component(component: impl ComponentNode<A> + 'static) -> Self {
        Self {
            key: None,
            kind: ViewKind::Component(Rc::new(component)),
            children: Vec::new(),
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
        }
    }

    pub fn with_flex_item(mut self, flex_item: FlexItemStyle) -> Self {
        self.flex_item = flex_item;
        self
    }

    pub fn with_size_hint(mut self, size_hint: LayoutSizeHint) -> Self {
        self.size_hint = size_hint;
        self
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Wraps a declarative builder into a [`View`] (same as [`IntoView::into_view`]).
    pub fn from<V: IntoView<A>>(value: V) -> Self {
        value.into_view()
    }
}

/// Converts a declarative widget builder into a runtime [`View`] node.
///
/// Application code returns [`View<A>`] and calls [`.into_view()`](Self::into_view) on the root builder
/// (re-export this trait in the app `view/prelude`, not in `ailloli_ui::prelude`). Framework uses it
/// for `.child`, `Window::content`, and custom widgets.
pub trait IntoView<A> {
    fn into_view(self) -> View<A>;
}

impl<A> IntoView<A> for View<A> {
    fn into_view(self) -> View<A> {
        self
    }
}

/// Allows `widget.key("id")` instead of `widget.into_view().key("id")`.
pub trait IntoViewKeyExt<A>: IntoView<A> + Sized {
    fn key(self, key: impl Into<String>) -> View<A> {
        self.into_view().key(key)
    }
}

impl<T, A> IntoViewKeyExt<A> for T where T: IntoView<A> {}

/// Widget contract for the `View` tree (separate from `layout::Widget` retained nodes).
pub trait Widget<A>: 'static {
    fn debug_name(&self) -> &'static str;

    /// Revision of widget-owned inputs that can change its layout result.
    ///
    /// Runtime-created signals normally invalidate their owning element. This
    /// hook also covers externally created reactive bindings when a layout pass
    /// is requested for another reason, without invalidating stable siblings.
    /// Implementations must return a cheap, stable and monotone value.
    fn layout_dependency_revision(&self) -> u64 {
        0
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult;

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult);

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }

    /// Policy used for provider-neutral focus-only pointer gestures.
    ///
    /// The default inherits from the parent; the input router falls back to
    /// suppression at the root so custom action widgets are safe by default.
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::Inherit
    }

    fn input_role(&self) -> InputRole {
        InputRole::None
    }

    fn hover_cursor_role(&self) -> HoverCursorRole {
        match self.input_role() {
            InputRole::TextSingleLine | InputRole::TextMultiLine => HoverCursorRole::Text,
            InputRole::None => HoverCursorRole::Inherit,
        }
    }

    fn hover_cursor_role_at(
        &self,
        _bounds: Rect,
        _layout: &LayoutResult,
        _pos: Point,
    ) -> HoverCursorRole {
        self.hover_cursor_role()
    }

    fn ime_cursor_rect(&self, _bounds: Rect, _layout: &LayoutResult) -> Option<Rect> {
        None
    }
}

pub trait ComponentNode<A>: 'static {
    fn build(&self, context: &mut Context<A>) -> View<A>;
}

pub struct Component<P, A> {
    props: P,
    render: fn(&mut Context<A>, P) -> View<A>,
}

impl<P, A> Component<P, A>
where
    P: Clone + 'static,
    A: 'static,
{
    pub fn new(props: P, render: fn(&mut Context<A>, P) -> View<A>) -> Self {
        Self { props, render }
    }
}

impl<P, A> ComponentNode<A> for Component<P, A>
where
    P: Clone + 'static,
    A: 'static,
{
    fn build(&self, context: &mut Context<A>) -> View<A> {
        (self.render)(context, self.props.clone())
    }
}

impl<P, A> IntoView<A> for Component<P, A>
where
    P: Clone + 'static,
    A: 'static,
{
    fn into_view(self) -> View<A> {
        View::component(self)
    }
}
