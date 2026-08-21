use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ailloli_ui_core::{Constraints, Offset, Rect, Scale, Size};
use ailloli_ui_runtime::app::{Invalidation, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, View, Widget};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;

#[derive(Default)]
pub(crate) struct Counters {
    pub(crate) builds: Cell<u64>,
    pub(crate) layouts: Cell<u64>,
    pub(crate) commits: Cell<u64>,
    pub(crate) reads: Cell<u64>,
}

struct CountingWidget(Rc<Counters>);

impl Widget<()> for CountingWidget {
    fn debug_name(&self) -> &'static str {
        "CountingWidget"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.0.layouts.set(self.0.layouts.get() + 1);
        let size = constraints.constrain(Size::new(120.0, 40.0));
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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.0.commits.set(self.0.commits.get() + 1);
    }
}

struct CountingComponent {
    counters: Rc<Counters>,
    signal: Option<Rc<RefCell<Option<Signal<u64>>>>>,
}

impl ComponentNode<()> for CountingComponent {
    fn build(&self, context: &mut Context<()>) -> View<()> {
        self.counters.builds.set(self.counters.builds.get() + 1);
        self.counters.reads.set(self.counters.reads.get() + 1);
        if let Some(slot) = &self.signal {
            *slot.borrow_mut() = Some(context.signal(0_u64));
        }
        View::leaf(CountingWidget(self.counters.clone()))
    }
}

struct HorizontalRoot;

impl Widget<()> for HorizontalRoot {
    fn debug_name(&self) -> &'static str {
        "HorizontalRoot"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut child_layouts = Vec::new();
        for child in children {
            let result = child.layout(engine, ctx, constraints.loosen());
            child_layouts.push(ChildLayout {
                offset: Offset::new(x, 0.0),
                size: result.size,
                paint_bounds: result.paint_bounds,
                visual_bounds: result.visual_bounds,
            });
            x += result.size.w;
            height = height.max(result.size.h);
        }
        let size = constraints.constrain(Size::new(x, height));
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

pub(crate) type Fixture = (
    Runtime<()>,
    TextSystem,
    Rc<Counters>,
    Rc<Counters>,
    Rc<RefCell<Option<Signal<u64>>>>,
);

pub(crate) fn fixture() -> Fixture {
    let file = Rc::new(Counters::default());
    let chat = Rc::new(Counters::default());
    let chat_signal = Rc::new(RefCell::new(None));
    let root = View::node(
        HorizontalRoot,
        vec![
            View::component(CountingComponent {
                counters: file.clone(),
                signal: None,
            })
            .key("file-tree"),
            View::component(CountingComponent {
                counters: chat.clone(),
                signal: Some(chat_signal.clone()),
            })
            .key("chat"),
        ],
    );
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile_view(root);
    let mut text = TextSystem::new();
    runtime.layout(Constraints::tight(500.0, 100.0), Scale::new(1.0), &mut text);
    (runtime, text, file, chat, chat_signal)
}

#[test]
fn one_thousand_chat_builds_do_not_touch_the_file_tree_sibling() {
    let (mut runtime, mut text, file, chat, chat_signal) = fixture();
    let file_before = (file.builds.get(), file.layouts.get(), file.reads.get());
    let chat_builds_before = chat.builds.get();

    for revision in 1..=1_000 {
        chat_signal.borrow().as_ref().unwrap().set(revision);
        runtime.layout(Constraints::tight(500.0, 100.0), Scale::new(1.0), &mut text);
    }

    assert_eq!(
        (file.builds.get(), file.layouts.get(), file.reads.get()),
        file_before,
        "a chat invalidation must not rebuild, relayout, or reread its sibling",
    );
    assert_eq!(chat.builds.get() - chat_builds_before, 1_000);
}

#[test]
fn paint_layout_and_build_requests_coalesce_to_the_strongest_level() {
    let (runtime, _text, _file, _chat, _signal) = fixture();
    let chat = runtime.tree.resolve_element_by_view_key("chat").unwrap();
    runtime.runtime.invalidate(chat, Invalidation::Paint);
    assert!(runtime.runtime.frame_work_plan().needs_paint());
    assert!(!runtime.runtime.frame_work_plan().needs_layout());
    runtime.runtime.invalidate(chat, Invalidation::Layout);
    assert!(runtime.runtime.frame_work_plan().needs_layout());
    assert!(!runtime.runtime.frame_work_plan().needs_build());
    runtime.runtime.invalidate(chat, Invalidation::Build);
    assert!(runtime.runtime.frame_work_plan().needs_build());
}
