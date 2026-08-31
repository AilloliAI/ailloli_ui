//! Attempt-local staging for geometry-derived widget state.

use ailloli_ui_runtime::layout::{LayoutAttemptToken, LayoutCtx};

/// Value derived by one exact outer layout attempt.
///
/// Widget-owned cells are outside the Runtime overlay. Carrying the checked
/// attempt token prevents an abandoned value from being published by a later
/// bounds commit or unrelated successful traversal.
#[derive(Clone, Copy)]
pub(crate) struct TransactionalLayoutPending<T> {
    /// Outer attempt that computed `value`.
    token: LayoutAttemptToken,
    /// Widget-owned state to publish only for the matching commit hook.
    value: T,
}

impl<T> TransactionalLayoutPending<T> {
    /// Associates a value with the active outer attempt.
    pub(crate) fn new(ctx: &LayoutCtx<'_>, value: T) -> Option<Self> {
        ctx.layout_attempt_token()
            .map(|token| Self { token, value })
    }

    /// Returns the value only inside the matching successful commit hook.
    pub(crate) fn into_committed(self, ctx: &LayoutCtx<'_>) -> Option<T> {
        (ctx.layout_attempt_token() == Some(self.token)).then_some(self.value)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::rc::Rc;

    use ailloli_ui_core::{Constraints, Offset, Rect, Scale, Size};
    use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    use ailloli_ui_runtime::component::{State, View, Widget};
    use ailloli_ui_runtime::layout::{
        ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult,
    };
    use ailloli_ui_runtime::scene::PaintCtx;
    use ailloli_ui_text::TextSystem;

    use super::TransactionalLayoutPending;

    /// Returns a leaf result whose geometry is completely explicit.
    fn leaf(size: Size) -> LayoutResult {
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Leaf that stages its constrained width for the exact active attempt.
    struct StagedWidthLeaf {
        pending: Cell<Option<TransactionalLayoutPending<u32>>>,
        applied: Rc<Cell<u32>>,
        commits: Rc<Cell<u32>>,
    }

    impl Widget<()> for StagedWidthLeaf {
        fn debug_name(&self) -> &'static str {
            "StagedWidthLeaf"
        }

        fn layout(
            &self,
            _engine: &mut LayoutEngine<'_, ()>,
            ctx: &mut LayoutCtx<'_>,
            _children: &mut [LayoutChild],
            constraints: Constraints,
        ) -> LayoutResult {
            self.pending.set(TransactionalLayoutPending::new(
                ctx,
                constraints.max_w as u32,
            ));
            leaf(constraints.max_size())
        }

        fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
            self.commits.set(self.commits.get() + 1);
            if let Some(value) = self
                .pending
                .take()
                .and_then(|pending| pending.into_committed(ctx))
            {
                self.applied.set(value);
            }
        }

        fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
    }

    /// Sibling whose state can abort an outer traversal after the staged leaf.
    struct PanickingLeaf {
        panic_now: State<bool>,
    }

    impl Widget<()> for PanickingLeaf {
        fn debug_name(&self) -> &'static str {
            "PanickingLeaf"
        }

        fn layout(
            &self,
            _engine: &mut LayoutEngine<'_, ()>,
            _ctx: &mut LayoutCtx<'_>,
            _children: &mut [LayoutChild],
            _constraints: Constraints,
        ) -> LayoutResult {
            assert!(!self.panic_now.read(), "intentional sibling layout panic");
            leaf(Size::new(1.0, 1.0))
        }

        fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
    }

    /// Parent changes the staged child's constraints only for the aborted run.
    struct OffsetParent {
        child_offset: State<f32>,
    }

    impl Widget<()> for OffsetParent {
        fn debug_name(&self) -> &'static str {
            "OffsetParent"
        }

        fn layout(
            &self,
            engine: &mut LayoutEngine<'_, ()>,
            ctx: &mut LayoutCtx<'_>,
            children: &mut [LayoutChild],
            constraints: Constraints,
        ) -> LayoutResult {
            let child_width = (constraints.max_w - 90.0).max(1.0);
            let staged = children[0].layout(engine, ctx, Constraints::tight(child_width, 10.0));
            let sibling = children[1].layout(engine, ctx, Constraints::tight(1.0, 1.0));
            let offset = self.child_offset.read();
            LayoutResult {
                size: constraints.max_size(),
                children: vec![
                    ChildLayout {
                        offset: Offset::new(offset, 0.0),
                        size: staged.size,
                        paint_bounds: staged.paint_bounds,
                        visual_bounds: staged.visual_bounds,
                    },
                    ChildLayout {
                        offset: Offset::new(0.0, 20.0),
                        size: sibling.size,
                        paint_bounds: sibling.paint_bounds,
                        visual_bounds: sibling.visual_bounds,
                    },
                ],
                paint_bounds: Rect::new(0.0, 0.0, constraints.max_w, constraints.max_h),
                visual_bounds: Rect::new(0.0, 0.0, constraints.max_w, constraints.max_h),
                overlay_hit_bounds: Vec::new(),
                clip: None,
                is_window_root_clip: false,
                artifact: None,
            }
        }

        fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
    }

    /// Runs one complete runtime-owned transactional layout.
    fn layout(runtime: &mut Runtime<()>, width: f32, text: &mut TextSystem) {
        runtime.layout(Constraints::tight(width, 40.0), Scale::new(1.0), text);
    }

    #[test]
    fn abandoned_pending_state_is_not_published_by_a_later_bounds_commit() {
        let applied = Rc::new(Cell::new(0));
        let commits = Rc::new(Cell::new(0));
        let panic_now = State::new(false);
        let child_offset = State::new(0.0);
        let mut runtime = Runtime::new(RuntimeHandle::new());
        runtime.reconcile(View::node(
            OffsetParent {
                child_offset: child_offset.clone(),
            },
            vec![
                View::leaf(StagedWidthLeaf {
                    pending: Cell::new(None),
                    applied: applied.clone(),
                    commits: commits.clone(),
                }),
                View::leaf(PanickingLeaf {
                    panic_now: panic_now.clone(),
                }),
            ],
        ));
        let mut text = TextSystem::new();

        layout(&mut runtime, 100.0, &mut text);
        assert_eq!((applied.get(), commits.get()), (10, 1));

        panic_now.set(true);
        let panic = catch_unwind(AssertUnwindSafe(|| {
            layout(&mut runtime, 110.0, &mut text);
        }));
        assert!(panic.is_err());
        assert_eq!((applied.get(), commits.get()), (10, 1));

        panic_now.set(false);
        child_offset.set(5.0);
        layout(&mut runtime, 100.0, &mut text);
        assert_eq!(
            (applied.get(), commits.get()),
            (10, 2),
            "the bounds-change hook must discard C1 from the abandoned attempt"
        );
    }

    /// Equal-geometry leaf whose staged value follows one reactive metric.
    struct EqualGeometryLeaf {
        metric: State<u32>,
        pending: Cell<Option<TransactionalLayoutPending<u32>>>,
        applied: Rc<Cell<u32>>,
        commits: Rc<Cell<u32>>,
    }

    impl Widget<()> for EqualGeometryLeaf {
        fn debug_name(&self) -> &'static str {
            "EqualGeometryLeaf"
        }

        fn layout(
            &self,
            _engine: &mut LayoutEngine<'_, ()>,
            ctx: &mut LayoutCtx<'_>,
            _children: &mut [LayoutChild],
            _constraints: Constraints,
        ) -> LayoutResult {
            self.pending
                .set(TransactionalLayoutPending::new(ctx, self.metric.read()));
            leaf(Size::new(10.0, 10.0))
        }

        fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
            self.commits.set(self.commits.get() + 1);
            if let Some(value) = self
                .pending
                .take()
                .and_then(|pending| pending.into_committed(ctx))
            {
                self.applied.set(value);
            }
        }

        fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
    }

    #[test]
    fn successful_equal_geometry_callback_publishes_once_but_cache_hit_does_not() {
        let metric = State::new(1_u32);
        let applied = Rc::new(Cell::new(0));
        let commits = Rc::new(Cell::new(0));
        let mut runtime = Runtime::new(RuntimeHandle::new());
        runtime.reconcile(View::leaf(EqualGeometryLeaf {
            metric: metric.clone(),
            pending: Cell::new(None),
            applied: applied.clone(),
            commits: commits.clone(),
        }));
        let mut text = TextSystem::new();

        layout(&mut runtime, 100.0, &mut text);
        assert_eq!((applied.get(), commits.get()), (1, 1));

        metric.set(2);
        layout(&mut runtime, 100.0, &mut text);
        assert_eq!(
            (applied.get(), commits.get()),
            (2, 2),
            "a successful authoritative callback must publish despite equal geometry"
        );

        layout(&mut runtime, 100.0, &mut text);
        assert_eq!(
            (applied.get(), commits.get()),
            (2, 2),
            "a stable cache hit must not rerun layout_committed"
        );
    }
}
