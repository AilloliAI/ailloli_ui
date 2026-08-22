//! Shared size/flex builder traits, view finalization, and implementation macros.

use ailloli_ui_core::geometry::{Constraints, Size};
use ailloli_ui_core::style::{
    resolve_widget_size, AlignItems, FlexItemStyle, FlexStyle, LayoutSizeHint, LayoutStyle, Length,
};
use ailloli_ui_runtime::component::{IntoView, View};

/// Resolves declarative sizing and constraints around an intrinsic size.
///
/// All sizes are logical pixels. Resolution, min/max precedence, and non-finite
/// handling are delegated to [`ailloli_ui_core::style::resolve_widget_size`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, Size};
/// use ailloli_ui_core::style::LayoutStyle;
/// use ailloli_ui_widgets::layout::layout_ext::apply_layout_size;
/// let size = apply_layout_size(Size::new(10.0, 20.0), LayoutStyle::new().width(40.0), Constraints::loose(100.0, 100.0));
/// assert_eq!(size, Size::new(40.0, 20.0));
/// ```
pub fn apply_layout_size(intrinsic: Size, layout: LayoutStyle, constraints: Constraints) -> Size {
    resolve_widget_size(intrinsic, layout, constraints)
}

/// Copies declarative flex-item style onto a built [`View`].
///
/// Existing view flex metadata is replaced; size hints are preserved as their
/// default through [`finish_view_sized`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::FlexItemStyle;
/// use ailloli_ui_runtime::component::{IntoView, View};
/// use ailloli_ui_widgets::{layout::finish_view, text::Text};
/// let view: View<()> = Text::new("grow").into_view();
/// let view = finish_view(view, FlexItemStyle::default().flex_grow(2.0));
/// assert_eq!(view.flex_item.flex_grow, 2.0);
/// ```
pub fn finish_view<A>(view: View<A>, flex_item: FlexItemStyle) -> View<A> {
    finish_view_sized(view, flex_item, LayoutSizeHint::default())
}

/// Copies flex-item style and declarative sizing hints onto a built [`View`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, Length};
/// use ailloli_ui_runtime::component::{IntoView, View};
/// use ailloli_ui_widgets::{layout::layout_ext::finish_view_sized, text::Text};
/// let view: View<()> = Text::new("fill").into_view();
/// let view = finish_view_sized(view, FlexItemStyle::default(), LayoutSizeHint { width: Length::Fill, height: Length::Auto });
/// assert_eq!(view.size_hint.width, Length::Fill);
/// ```
pub fn finish_view_sized<A>(
    view: View<A>,
    flex_item: FlexItemStyle,
    size_hint: LayoutSizeHint,
) -> View<A> {
    view.with_flex_item(flex_item).with_size_hint(size_hint)
}

/// Mutable access to a declarative widget's [`LayoutStyle`].
///
/// This low-level escape hatch supports wrapper composition; normal callers
/// can prefer the consuming builder methods generated for widgets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Length;
/// use ailloli_ui_widgets::layout::{Container, LayoutExt};
/// let mut container: Container<()> = Container::new();
/// LayoutExt::layout_mut(&mut container).width = Length::px(80.0);
/// assert_eq!(LayoutExt::layout_mut(&mut container).width, Length::px(80.0));
/// ```
pub trait LayoutExt: Sized {
    /// Returns the widget's mutable declarative size/inset state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// use ailloli_ui_widgets::layout::{Container, LayoutExt};
    /// let mut container: Container<()> = Container::new();
    /// container.layout_mut().height = Length::px(24.0);
    /// assert_eq!(container.layout_mut().height, Length::px(24.0));
    /// ```
    fn layout_mut(&mut self) -> &mut LayoutStyle;
}

/// Flex-item builders returning a [`View`] for already-built views/components.
///
/// These methods replace one metadata field and preserve the retained subtree.
/// Numeric grow/shrink values are stored according to [`FlexItemStyle`]'s
/// normalization rules.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{IntoView, View};
/// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
/// let base: View<()> = Text::new("grow").into_view();
/// let grown: View<()> = base.flex_grow();
/// assert_eq!(grown.flex_item.flex_grow, 1.0);
/// ```
pub trait FlexItemExt<A>: IntoView<A> + Sized {
    /// Sets flex-grow weight to one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("grow").into_view().flex_grow();
    /// assert_eq!(view.flex_item.flex_grow, 1.0);
    /// ```
    fn flex_grow(self) -> View<A> {
        apply_flex_item(self, |item| item.flex_grow(1.0))
    }

    /// Sets the dimensionless flex-grow weight.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("grow").into_view().flex_grow_by(3.0);
    /// assert_eq!(view.flex_item.flex_grow, 3.0);
    /// ```
    fn flex_grow_by(self, value: f32) -> View<A> {
        apply_flex_item(self, |item| item.flex_grow(value))
    }

    /// Sets the dimensionless flex-shrink weight.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("shrink").into_view().flex_shrink(2.0);
    /// assert_eq!(view.flex_item.flex_shrink, 2.0);
    /// ```
    fn flex_shrink(self, value: f32) -> View<A> {
        apply_flex_item(self, |item| item.flex_shrink(value))
    }

    /// Sets the preferred main-axis flex basis.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("basis").into_view().flex_basis(40.0);
    /// assert_eq!(view.flex_item.flex_basis, Length::px(40.0));
    /// ```
    fn flex_basis(self, value: impl Into<Length>) -> View<A> {
        apply_flex_item(self, |item| item.flex_basis(value))
    }

    /// Overrides the parent cross-axis alignment for this view.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::AlignItems;
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("end").into_view().align_self(AlignItems::End);
    /// assert_eq!(view.flex_item.align_self, Some(AlignItems::End));
    /// ```
    fn align_self(self, value: AlignItems) -> View<A> {
        apply_flex_item(self, |item| item.align_self(value))
    }

    /// Marks width as parent-fill in the view size hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("wide").into_view().fill_width();
    /// assert_eq!(view.size_hint.width, Length::Fill);
    /// ```
    fn fill_width(self) -> View<A> {
        apply_size_hint(self, |hint| LayoutSizeHint {
            width: Length::Fill,
            ..hint
        })
    }

    /// Marks height as parent-fill in the view size hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("tall").into_view().fill_height();
    /// assert_eq!(view.size_hint.height, Length::Fill);
    /// ```
    fn fill_height(self) -> View<A> {
        apply_size_hint(self, |hint| LayoutSizeHint {
            height: Length::Fill,
            ..hint
        })
    }

    /// Marks both axes as parent-fill in the view size hint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Length;
    /// use ailloli_ui_runtime::component::{IntoView, View};
    /// use ailloli_ui_widgets::{layout::FlexItemExt, text::Text};
    /// let view: View<()> = Text::new("fill").into_view().fill();
    /// assert_eq!((view.size_hint.width, view.size_hint.height), (Length::Fill, Length::Fill));
    /// ```
    fn fill(self) -> View<A> {
        apply_size_hint(self, |_| LayoutSizeHint {
            width: Length::Fill,
            height: Length::Fill,
        })
    }
}

impl<T, A> FlexItemExt<A> for T where T: IntoView<A> {}

/// Converts a child and mutates exactly its flex-item metadata.
fn apply_flex_item<A, F>(child: impl IntoView<A>, f: F) -> View<A>
where
    F: FnOnce(FlexItemStyle) -> FlexItemStyle,
{
    let mut view = child.into_view();
    view.flex_item = f(view.flex_item);
    view
}

/// Converts a child and mutates exactly its declarative size hint.
fn apply_size_hint<A, F>(child: impl IntoView<A>, f: F) -> View<A>
where
    F: FnOnce(LayoutSizeHint) -> LayoutSizeHint,
{
    let mut view = child.into_view();
    view.size_hint = f(view.size_hint);
    view
}

#[macro_export]
/// Implements [`LayoutExt`] and consuming size/flex-item
/// builders for a generic widget with `layout` and `flex_item` fields.
///
/// The target type must have one type parameter and fields of types
/// [`LayoutStyle`] and [`FlexItemStyle`], respectively.
///
/// # Examples
///
/// ```
/// use std::marker::PhantomData;
/// use ailloli_ui_core::style::{FlexItemStyle, LayoutStyle, Length};
/// use ailloli_ui_widgets::impl_layout_builders;
///
/// struct Example<A> {
///     layout: LayoutStyle,
///     flex_item: FlexItemStyle,
///     action: PhantomData<A>,
/// }
/// impl_layout_builders!(Example);
/// let widget = Example::<()> {
///     layout: LayoutStyle::default(),
///     flex_item: FlexItemStyle::default(),
///     action: PhantomData,
/// }.width(80.0);
/// assert_eq!(widget.layout.width, Length::Px(80.0));
/// ```
macro_rules! impl_layout_builders {
    ($ty:ident) => {
        impl<A: 'static> $crate::layout::layout_ext::LayoutExt for $ty<A> {
            fn layout_mut(&mut self) -> &mut ailloli_ui_core::style::LayoutStyle {
                &mut self.layout
            }
        }

        impl<A: 'static> $ty<A> {
            /// Sets preferred width from a logical-pixel or [length](ailloli_ui_core::style::Length) value.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().width(120.0);
            /// let _ = widget;
            /// ```
            pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.width = value.into();
                self
            }

            /// Sets preferred height from a logical-pixel or length value.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().height(40.0);
            /// let _ = widget;
            /// ```
            pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.height = value.into();
                self
            }

            /// Sets the minimum width bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().min_width(40.0);
            /// let _ = widget;
            /// ```
            pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_width = value.into();
                self
            }

            /// Sets the maximum width bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().max_width(320.0);
            /// let _ = widget;
            /// ```
            pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_width = value.into();
                self
            }

            /// Sets the minimum height bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().min_height(24.0);
            /// let _ = widget;
            /// ```
            pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_height = value.into();
                self
            }

            /// Sets the maximum height bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().max_height(240.0);
            /// let _ = widget;
            /// ```
            pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_height = value.into();
                self
            }

            /// Sets both preferred axes to parent fill.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().fill();
            /// let _ = widget;
            /// ```
            pub fn fill(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            /// Sets preferred width to parent fill.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().fill_width();
            /// let _ = widget;
            /// ```
            pub fn fill_width(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self
            }

            /// Sets preferred height to parent fill.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().fill_height();
            /// let _ = widget;
            /// ```
            pub fn fill_height(mut self) -> Self {
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            /// Sets uniform outer margin in logical pixels.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().margin(8.0);
            /// let _ = widget;
            /// ```
            pub fn margin(mut self, value: f32) -> Self {
                self.layout = self.layout.margin(value);
                self
            }

            /// Sets uniform inner padding in logical pixels.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().padding(8.0);
            /// let _ = widget;
            /// ```
            pub fn padding(mut self, value: f32) -> Self {
                self.layout = self.layout.padding(value);
                self
            }

            /// Sets flex-grow weight to one.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().flex_grow();
            /// let _ = widget;
            /// ```
            pub fn flex_grow(mut self) -> Self {
                self.flex_item = self.flex_item.flex_grow(1.0);
                self
            }

            /// Sets a dimensionless flex-grow weight.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().flex_grow_by(2.0);
            /// let _ = widget;
            /// ```
            pub fn flex_grow_by(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_grow(value);
                self
            }

            /// Sets a dimensionless flex-shrink weight.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().flex_shrink(1.0);
            /// let _ = widget;
            /// ```
            pub fn flex_shrink(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_shrink(value);
                self
            }

            /// Sets the preferred main-axis flex basis.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().flex_basis(80.0);
            /// let _ = widget;
            /// ```
            pub fn flex_basis(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.flex_item = self.flex_item.flex_basis(value);
                self
            }

            /// Overrides cross-axis alignment for this flex item.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::style::AlignItems;
            /// use ailloli_ui_widgets::layout::Container;
            /// let widget: Container<()> = Container::new().align_self(AlignItems::Center);
            /// let _ = widget;
            /// ```
            pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
                self.flex_item = self.flex_item.align_self(value);
                self
            }
        }
    };
}

#[macro_export]
/// Implements [`LayoutExt`] and consuming size/flex-item
/// builders for a non-generic widget with `layout` and `flex_item` fields.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{FlexItemStyle, LayoutStyle, Length};
/// use ailloli_ui_widgets::impl_layout_builders_unit;
///
/// struct Example {
///     layout: LayoutStyle,
///     flex_item: FlexItemStyle,
/// }
/// impl_layout_builders_unit!(Example);
/// let widget = Example {
///     layout: LayoutStyle::default(),
///     flex_item: FlexItemStyle::default(),
/// }.height(32.0);
/// assert_eq!(widget.layout.height, Length::Px(32.0));
/// ```
macro_rules! impl_layout_builders_unit {
    ($ty:ty) => {
        impl $crate::layout::layout_ext::LayoutExt for $ty {
            fn layout_mut(&mut self) -> &mut ailloli_ui_core::style::LayoutStyle {
                &mut self.layout
            }
        }

        impl $ty {
            /// Sets preferred width from a logical-pixel or length value.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).width(24.0);
            /// let _ = widget;
            /// ```
            pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.width = value.into();
                self
            }

            /// Sets preferred height from a logical-pixel or length value.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).height(24.0);
            /// let _ = widget;
            /// ```
            pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.height = value.into();
                self
            }

            /// Sets the minimum width bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).min_width(12.0);
            /// let _ = widget;
            /// ```
            pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_width = value.into();
                self
            }

            /// Sets the maximum width bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).max_width(32.0);
            /// let _ = widget;
            /// ```
            pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_width = value.into();
                self
            }

            /// Sets the minimum height bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).min_height(12.0);
            /// let _ = widget;
            /// ```
            pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_height = value.into();
                self
            }

            /// Sets the maximum height bound.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).max_height(32.0);
            /// let _ = widget;
            /// ```
            pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_height = value.into();
                self
            }

            /// Sets both preferred axes to parent fill.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).fill();
            /// let _ = widget;
            /// ```
            pub fn fill(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            /// Sets preferred width to parent fill.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).fill_width();
            /// let _ = widget;
            /// ```
            pub fn fill_width(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self
            }

            /// Sets preferred height to parent fill.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).fill_height();
            /// let _ = widget;
            /// ```
            pub fn fill_height(mut self) -> Self {
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            /// Sets uniform outer margin in logical pixels.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).margin(4.0);
            /// let _ = widget;
            /// ```
            pub fn margin(mut self, value: f32) -> Self {
                self.layout = self.layout.margin(value);
                self
            }

            /// Sets uniform inner padding in logical pixels.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).padding(4.0);
            /// let _ = widget;
            /// ```
            pub fn padding(mut self, value: f32) -> Self {
                self.layout = self.layout.padding(value);
                self
            }

            /// Sets flex-grow weight to one.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).flex_grow();
            /// let _ = widget;
            /// ```
            pub fn flex_grow(mut self) -> Self {
                self.flex_item = self.flex_item.flex_grow(1.0);
                self
            }

            /// Sets a dimensionless flex-grow weight.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).flex_grow_by(2.0);
            /// let _ = widget;
            /// ```
            pub fn flex_grow_by(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_grow(value);
                self
            }

            /// Sets a dimensionless flex-shrink weight.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).flex_shrink(1.0);
            /// let _ = widget;
            /// ```
            pub fn flex_shrink(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_shrink(value);
                self
            }

            /// Sets the preferred main-axis flex basis.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::IconId;
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).flex_basis(20.0);
            /// let _ = widget;
            /// ```
            pub fn flex_basis(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.flex_item = self.flex_item.flex_basis(value);
                self
            }

            /// Overrides cross-axis alignment for this flex item.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::{style::AlignItems, IconId};
            /// use ailloli_ui_widgets::primitives::Icon;
            /// let widget = Icon::new(IconId::Close).align_self(AlignItems::Center);
            /// let _ = widget;
            /// ```
            pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
                self.flex_item = self.flex_item.align_self(value);
                self
            }
        }
    };
}

/// Mutable access to row/column [`FlexStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::layout::{FlexLayoutExt, Row};
/// let mut row: Row<()> = Row::new();
/// row.flex_mut().gap = 6.0;
/// assert_eq!(row.flex_mut().gap, 6.0);
/// ```
pub trait FlexLayoutExt: Sized {
    /// Returns mutable direction, gap, and cross-axis alignment state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::AlignItems;
    /// use ailloli_ui_widgets::layout::{Column, FlexLayoutExt};
    /// let mut column: Column<()> = Column::new();
    /// column.flex_mut().align_items = AlignItems::Center;
    /// assert_eq!(column.flex_mut().align_items, AlignItems::Center);
    /// ```
    fn flex_mut(&mut self) -> &mut FlexStyle;
}

#[macro_export]
/// Implements [`FlexLayoutExt`] and consuming
/// container-flex builders for a generic widget with a `flex` field.
///
/// The invocation scope must import [`FlexStyle`] because the generated trait
/// implementation names that type directly.
///
/// # Examples
///
/// ```
/// use std::marker::PhantomData;
/// use ailloli_ui_core::style::{AlignItems, FlexStyle};
/// use ailloli_ui_widgets::impl_flex_builders;
/// use ailloli_ui_widgets::layout::FlexLayoutExt;
///
/// struct Example<A> {
///     flex: FlexStyle,
///     action: PhantomData<A>,
/// }
/// impl_flex_builders!(Example);
/// let widget = Example::<()> {
///     flex: FlexStyle::default(),
///     action: PhantomData,
/// }.align_items(AlignItems::Center);
/// assert_eq!(widget.flex.align_items, AlignItems::Center);
/// ```
macro_rules! impl_flex_builders {
    ($ty:ident) => {
        impl<A: 'static> $crate::layout::layout_ext::FlexLayoutExt for $ty<A> {
            fn flex_mut(&mut self) -> &mut FlexStyle {
                &mut self.flex
            }
        }

        impl<A: 'static> $ty<A> {
            /// Sets non-negative inter-child gap in logical pixels.
            ///
            /// Negative and `NaN` values resolve to zero through `f32::max`.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_widgets::layout::Row;
            /// let row: Row<()> = Row::new().gap(8.0);
            /// let _ = row;
            /// ```
            pub fn gap(mut self, value: f32) -> Self {
                self.flex_mut().gap = value.max(0.0);
                self
            }

            /// Sets the default child alignment on the cross axis.
            ///
            /// # Examples
            ///
            /// ```
            /// use ailloli_ui_core::style::AlignItems;
            /// use ailloli_ui_widgets::layout::Row;
            /// let row: Row<()> = Row::new().align_items(AlignItems::Center);
            /// let _ = row;
            /// ```
            pub fn align_items(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
                self.flex_mut().align_items = value;
                self
            }
        }
    };
}
