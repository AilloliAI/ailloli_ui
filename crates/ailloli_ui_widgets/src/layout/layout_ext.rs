use ailloli_ui_core::geometry::{Constraints, Size};
use ailloli_ui_core::style::{
    resolve_widget_size, AlignItems, FlexItemStyle, FlexStyle, LayoutSizeHint, LayoutStyle, Length,
};
use ailloli_ui_runtime::component::{IntoView, View};

/// Applies `LayoutStyle` sizing to an intrinsic size.
pub fn apply_layout_size(intrinsic: Size, layout: LayoutStyle, constraints: Constraints) -> Size {
    resolve_widget_size(intrinsic, layout, constraints)
}

/// Copies declarative flex item style onto a built [`View`].
pub fn finish_view<A>(view: View<A>, flex_item: FlexItemStyle) -> View<A> {
    finish_view_sized(view, flex_item, LayoutSizeHint::default())
}

/// Copies flex item style and declarative sizing hints onto a built [`View`].
pub fn finish_view_sized<A>(
    view: View<A>,
    flex_item: FlexItemStyle,
    size_hint: LayoutSizeHint,
) -> View<A> {
    view.with_flex_item(flex_item).with_size_hint(size_hint)
}

/// Mutable access to a declarative widget's `LayoutStyle`.
pub trait LayoutExt: Sized {
    fn layout_mut(&mut self) -> &mut LayoutStyle;
}

/// Flex item builders returning a [`View`] (for functions/components that already produce views).
pub trait FlexItemExt<A>: IntoView<A> + Sized {
    fn flex_grow(self) -> View<A> {
        apply_flex_item(self, |item| item.flex_grow(1.0))
    }

    fn flex_grow_by(self, value: f32) -> View<A> {
        apply_flex_item(self, |item| item.flex_grow(value))
    }

    fn flex_shrink(self, value: f32) -> View<A> {
        apply_flex_item(self, |item| item.flex_shrink(value))
    }

    fn flex_basis(self, value: impl Into<Length>) -> View<A> {
        apply_flex_item(self, |item| item.flex_basis(value))
    }

    fn align_self(self, value: AlignItems) -> View<A> {
        apply_flex_item(self, |item| item.align_self(value))
    }

    fn fill_width(self) -> View<A> {
        apply_size_hint(self, |hint| LayoutSizeHint {
            width: Length::Fill,
            ..hint
        })
    }

    fn fill_height(self) -> View<A> {
        apply_size_hint(self, |hint| LayoutSizeHint {
            height: Length::Fill,
            ..hint
        })
    }

    fn fill(self) -> View<A> {
        apply_size_hint(self, |_| LayoutSizeHint {
            width: Length::Fill,
            height: Length::Fill,
        })
    }
}

impl<T, A> FlexItemExt<A> for T where T: IntoView<A> {}

fn apply_flex_item<A, F>(child: impl IntoView<A>, f: F) -> View<A>
where
    F: FnOnce(FlexItemStyle) -> FlexItemStyle,
{
    let mut view = child.into_view();
    view.flex_item = f(view.flex_item);
    view
}

fn apply_size_hint<A, F>(child: impl IntoView<A>, f: F) -> View<A>
where
    F: FnOnce(LayoutSizeHint) -> LayoutSizeHint,
{
    let mut view = child.into_view();
    view.size_hint = f(view.size_hint);
    view
}

#[macro_export]
macro_rules! impl_layout_builders {
    ($ty:ident) => {
        impl<A: 'static> $crate::layout::layout_ext::LayoutExt for $ty<A> {
            fn layout_mut(&mut self) -> &mut ailloli_ui_core::style::LayoutStyle {
                &mut self.layout
            }
        }

        impl<A: 'static> $ty<A> {
            pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.width = value.into();
                self
            }

            pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.height = value.into();
                self
            }

            pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_width = value.into();
                self
            }

            pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_width = value.into();
                self
            }

            pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_height = value.into();
                self
            }

            pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_height = value.into();
                self
            }

            pub fn fill(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            pub fn fill_width(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self
            }

            pub fn fill_height(mut self) -> Self {
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            pub fn margin(mut self, value: f32) -> Self {
                self.layout = self.layout.margin(value);
                self
            }

            pub fn padding(mut self, value: f32) -> Self {
                self.layout = self.layout.padding(value);
                self
            }

            pub fn flex_grow(mut self) -> Self {
                self.flex_item = self.flex_item.flex_grow(1.0);
                self
            }

            pub fn flex_grow_by(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_grow(value);
                self
            }

            pub fn flex_shrink(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_shrink(value);
                self
            }

            pub fn flex_basis(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.flex_item = self.flex_item.flex_basis(value);
                self
            }

            pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
                self.flex_item = self.flex_item.align_self(value);
                self
            }
        }
    };
}

#[macro_export]
macro_rules! impl_layout_builders_unit {
    ($ty:ty) => {
        impl $crate::layout::layout_ext::LayoutExt for $ty {
            fn layout_mut(&mut self) -> &mut ailloli_ui_core::style::LayoutStyle {
                &mut self.layout
            }
        }

        impl $ty {
            pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.width = value.into();
                self
            }

            pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.height = value.into();
                self
            }

            pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_width = value.into();
                self
            }

            pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_width = value.into();
                self
            }

            pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.min_height = value.into();
                self
            }

            pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.layout.max_height = value.into();
                self
            }

            pub fn fill(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            pub fn fill_width(mut self) -> Self {
                self.layout.width = ailloli_ui_core::style::Length::Fill;
                self
            }

            pub fn fill_height(mut self) -> Self {
                self.layout.height = ailloli_ui_core::style::Length::Fill;
                self
            }

            pub fn margin(mut self, value: f32) -> Self {
                self.layout = self.layout.margin(value);
                self
            }

            pub fn padding(mut self, value: f32) -> Self {
                self.layout = self.layout.padding(value);
                self
            }

            pub fn flex_grow(mut self) -> Self {
                self.flex_item = self.flex_item.flex_grow(1.0);
                self
            }

            pub fn flex_grow_by(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_grow(value);
                self
            }

            pub fn flex_shrink(mut self, value: f32) -> Self {
                self.flex_item = self.flex_item.flex_shrink(value);
                self
            }

            pub fn flex_basis(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
                self.flex_item = self.flex_item.flex_basis(value);
                self
            }

            pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
                self.flex_item = self.flex_item.align_self(value);
                self
            }
        }
    };
}

/// Shared flex builders used by `Row` and `Column`.
pub trait FlexLayoutExt: Sized {
    fn flex_mut(&mut self) -> &mut FlexStyle;
}

#[macro_export]
macro_rules! impl_flex_builders {
    ($ty:ident) => {
        impl<A: 'static> $crate::layout::layout_ext::FlexLayoutExt for $ty<A> {
            fn flex_mut(&mut self) -> &mut FlexStyle {
                &mut self.flex
            }
        }

        impl<A: 'static> $ty<A> {
            pub fn gap(mut self, value: f32) -> Self {
                self.flex_mut().gap = value.max(0.0);
                self
            }

            pub fn align_items(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
                self.flex_mut().align_items = value;
                self
            }
        }
    };
}
