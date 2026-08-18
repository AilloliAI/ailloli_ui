use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, FlexDirection, FlexItemStyle, FlexStyle, LayoutSizeHint, LayoutStyle,
};
use ailloli_ui_core::Offset;
use ailloli_ui_runtime::component::Widget;
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutResult};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx};
use ailloli_ui_runtime::scene::PaintCtx;

fn cross_offset(align: AlignItems, cross_extent: f32, child_cross: f32) -> f32 {
    match align {
        AlignItems::Start => 0.0,
        AlignItems::Center => ((cross_extent - child_cross) * 0.5).max(0.0),
        AlignItems::End => (cross_extent - child_cross).max(0.0),
        AlignItems::Stretch => 0.0,
    }
}

pub struct FlexWidget {
    pub layout: LayoutStyle,
    pub flex: FlexStyle,
    pub items: Vec<FlexItemStyle>,
    pub child_hints: Vec<LayoutSizeHint>,
}

impl FlexWidget {
    fn main_cross(size: Size, direction: FlexDirection) -> (f32, f32) {
        match direction {
            FlexDirection::Column => (size.h, size.w),
            FlexDirection::Row => (size.w, size.h),
        }
    }

    fn set_main_cross(size: &mut Size, main: f32, cross: f32, direction: FlexDirection) {
        match direction {
            FlexDirection::Column => {
                size.h = main;
                size.w = cross;
            }
            FlexDirection::Row => {
                size.w = main;
                size.h = cross;
            }
        }
    }

    fn main_available(constraints: Constraints, direction: FlexDirection) -> f32 {
        match direction {
            FlexDirection::Column => constraints.max_h,
            FlexDirection::Row => constraints.max_w,
        }
    }

    fn effective_grow(item: FlexItemStyle, main_fill: bool) -> f32 {
        if item.flex_grow > 0.0 {
            item.flex_grow
        } else if main_fill {
            1.0
        } else {
            0.0
        }
    }

    fn base_main_from_basis(item: FlexItemStyle, main_available: f32) -> f32 {
        if item.flex_basis.is_auto() {
            0.0
        } else {
            item.flex_basis
                .resolve(main_available)
                .unwrap_or(0.0)
                .max(0.0)
        }
    }

    fn probe_constraints(
        loose: Constraints,
        direction: FlexDirection,
        item: FlexItemStyle,
        hint: LayoutSizeHint,
        main_available: f32,
    ) -> Constraints {
        let main_fill = hint.is_main_axis_fill(direction);
        let grow = Self::effective_grow(item, main_fill);
        let mut child_c = loose;

        if main_fill {
            let basis_main = Self::base_main_from_basis(item, main_available);
            match direction {
                FlexDirection::Column => {
                    child_c.max_h = basis_main;
                    child_c.min_h = 0.0;
                }
                FlexDirection::Row => {
                    child_c.max_w = basis_main;
                    child_c.min_w = 0.0;
                }
            }
        } else if let Some(basis) = item.flex_basis.resolve(main_available) {
            match direction {
                FlexDirection::Column => child_c.max_h = basis,
                FlexDirection::Row => child_c.max_w = basis,
            }
        } else if grow > 0.0 {
            // Explicit flex_grow without main-axis Fill: intrinsic probe only.
            match direction {
                FlexDirection::Column => {
                    child_c.max_h = f32::INFINITY;
                }
                FlexDirection::Row => {
                    child_c.max_w = f32::INFINITY;
                }
            }
        }

        child_c
    }

    fn compute_base_main(
        item: FlexItemStyle,
        hint: LayoutSizeHint,
        direction: FlexDirection,
        main_available: f32,
        measured_main: f32,
    ) -> f32 {
        let main_fill = hint.is_main_axis_fill(direction);
        if main_fill {
            return Self::base_main_from_basis(item, main_available);
        }
        if item.flex_basis.is_auto() {
            measured_main
        } else {
            item.flex_basis
                .resolve(main_available)
                .unwrap_or(measured_main)
                .max(measured_main)
        }
    }
}

impl<A: 'static> Widget<A> for FlexWidget {
    fn debug_name(&self) -> &'static str {
        match self.flex.direction {
            FlexDirection::Row => "Row",
            FlexDirection::Column => "Column",
        }
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let direction = self.flex.direction;
        let gap = self.flex.gap;
        let loose = Constraints::loose(constraints.max_w, constraints.max_h);
        let main_available = Self::main_available(loose, direction);

        let mut measured = Vec::with_capacity(children.len());
        let mut base_mains = Vec::with_capacity(children.len());
        let mut grow_weights = Vec::with_capacity(children.len());

        for (idx, child) in children.iter_mut().enumerate() {
            let item = self.items.get(idx).copied().unwrap_or_default();
            let hint = self.child_hints.get(idx).copied().unwrap_or_default();
            let main_fill = hint.is_main_axis_fill(direction);
            let grow = Self::effective_grow(item, main_fill);
            grow_weights.push(grow);

            let child_c = Self::probe_constraints(loose, direction, item, hint, main_available);
            let result = child.layout(engine, ctx, child_c);
            let (main, _) = Self::main_cross(result.size, direction);
            base_mains.push(Self::compute_base_main(
                item,
                hint,
                direction,
                main_available,
                main,
            ));
            measured.push(result);
        }

        let gap_total = if children.is_empty() {
            0.0
        } else {
            gap * (children.len().saturating_sub(1) as f32)
        };
        let sum_base: f32 = base_mains.iter().sum();

        let intrinsic_cross = measured
            .iter()
            .map(|r| Self::main_cross(r.size, direction).1)
            .fold(0.0f32, f32::max);

        let intrinsic_main = sum_base + gap_total;

        let resolved = self.layout.resolve(constraints);
        let (intrinsic_w, intrinsic_h) = match direction {
            FlexDirection::Column => (intrinsic_cross, intrinsic_main),
            FlexDirection::Row => (intrinsic_main, intrinsic_cross),
        };
        let slot_tight_w = constraints.min_w == constraints.max_w;
        let slot_tight_h = constraints.min_h == constraints.max_h;
        let (mut flex_w, mut flex_h) = resolved.size(intrinsic_w, intrinsic_h, constraints);
        if slot_tight_w && !self.layout.width.is_auto() {
            flex_w = constraints.max_w;
        }
        if slot_tight_h && !self.layout.height.is_auto() {
            flex_h = constraints.max_h;
        }
        let mut flex_size = Size::new(flex_w, flex_h);
        if self.layout.width.is_auto() && self.layout.height.is_auto() {
            flex_size = constraints.constrain(flex_size);
        }
        let (flex_main, flex_cross) = Self::main_cross(flex_size, direction);

        let free_main = (flex_main - sum_base - gap_total).max(0.0);
        let total_grow: f32 = grow_weights.iter().sum();
        let mut final_mains = base_mains.clone();
        if total_grow > 0.0 && free_main > 0.0 {
            for (idx, weight) in grow_weights.iter().enumerate() {
                if *weight > 0.0 {
                    final_mains[idx] += free_main * (weight / total_grow);
                }
            }
        }

        let assigned_with_gap: f32 = final_mains.iter().sum::<f32>() + gap_total;
        if assigned_with_gap > flex_main {
            let overflow = assigned_with_gap - flex_main;
            let total_shrink_weight: f32 = self
                .items
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    if item.flex_shrink > 0.0 {
                        final_mains[idx] * item.flex_shrink
                    } else {
                        0.0
                    }
                })
                .sum();
            if total_shrink_weight > 0.0 {
                for (idx, item) in self.items.iter().enumerate() {
                    if item.flex_shrink > 0.0 {
                        let weight = final_mains[idx] * item.flex_shrink;
                        final_mains[idx] -= overflow * (weight / total_shrink_weight);
                        final_mains[idx] = final_mains[idx].max(0.0);
                    }
                }
            }
        }

        let mut child_layouts = Vec::with_capacity(measured.len());
        let mut cursor_main = 0.0;

        for (idx, child) in measured.into_iter().enumerate() {
            let item = self.items.get(idx).copied().unwrap_or_default();
            let align = item.align_self.unwrap_or(self.flex.align_items);
            let mut child_size = child.size;
            let (_, mut child_cross) = Self::main_cross(child_size, direction);
            let child_main = final_mains[idx];

            if align == AlignItems::Stretch {
                child_cross = flex_cross;
            } else {
                child_cross = child_cross.min(flex_cross);
            }

            Self::set_main_cross(&mut child_size, child_main, child_cross, direction);
            let cross_off = cross_offset(align, flex_cross, child_cross);

            let offset = match direction {
                FlexDirection::Column => Offset::new(cross_off, cursor_main),
                FlexDirection::Row => Offset::new(cursor_main, cross_off),
            };

            let paint_bounds = Rect::new(offset.x, offset.y, child_size.w, child_size.h);
            child_layouts.push(ChildLayout {
                offset,
                size: child_size,
                paint_bounds,
                visual_bounds: paint_bounds,
            });

            cursor_main += child_main + gap;
        }

        for (idx, child) in children.iter_mut().enumerate() {
            let slot = &child_layouts[idx];
            let tight = Constraints::tight(slot.size.w, slot.size.h);
            let _ = child.layout(engine, ctx, tight);
        }

        LayoutResult {
            size: flex_size,
            children: child_layouts,
            paint_bounds: Rect::new(0.0, 0.0, flex_size.w, flex_size.h),
            visual_bounds: Rect::new(0.0, 0.0, flex_size.w, flex_size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }
}

/// Keeps only margin/padding for outer layout wrappers.
pub(crate) fn layout_insets_only(layout: LayoutStyle) -> LayoutStyle {
    LayoutStyle {
        margin: layout.margin,
        padding: layout.padding,
        ..LayoutStyle::default()
    }
}

/// Keeps only sizing for the inner flex widget.
pub(crate) fn layout_sizing_only(layout: LayoutStyle) -> LayoutStyle {
    LayoutStyle {
        width: layout.width,
        height: layout.height,
        min_width: layout.min_width,
        max_width: layout.max_width,
        min_height: layout.min_height,
        max_height: layout.max_height,
        ..LayoutStyle::default()
    }
}
