use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ailloli_ui_core::{Constraints, Offset, Rect, Scale, Size};
use ailloli_ui_runtime::app::{
    Invalidation, InvalidationSource, Runtime, RuntimeHandle, INVALIDATION_PROVENANCE_CAPACITY,
};
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

pub(crate) struct Fixture {
    pub(crate) runtime: Runtime<()>,
    pub(crate) text: TextSystem,
    pub(crate) file: Rc<Counters>,
    pub(crate) chat: Rc<Counters>,
    pub(crate) chat_signal: Rc<RefCell<Option<Signal<u64>>>>,
    pub(crate) terminal: Rc<Counters>,
    pub(crate) terminal_signal: Rc<RefCell<Option<Signal<u64>>>>,
}

pub(crate) fn fixture() -> Fixture {
    let file = Rc::new(Counters::default());
    let chat = Rc::new(Counters::default());
    let chat_signal = Rc::new(RefCell::new(None));
    let terminal = Rc::new(Counters::default());
    let terminal_signal = Rc::new(RefCell::new(None));
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
            View::component(CountingComponent {
                counters: terminal.clone(),
                signal: Some(terminal_signal.clone()),
            })
            .key("terminal"),
        ],
    );
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile_view(root);
    let mut text = TextSystem::new();
    runtime.layout(Constraints::tight(500.0, 100.0), Scale::new(1.0), &mut text);
    Fixture {
        runtime,
        text,
        file,
        chat,
        chat_signal,
        terminal,
        terminal_signal,
    }
}

#[test]
fn one_thousand_chat_builds_do_not_touch_the_file_tree_sibling() {
    let mut fixture = fixture();
    let file_before = (
        fixture.file.builds.get(),
        fixture.file.layouts.get(),
        fixture.file.reads.get(),
    );
    let chat_builds_before = fixture.chat.builds.get();

    for revision in 1..=1_000 {
        fixture.chat_signal.borrow().as_ref().unwrap().set(revision);
        fixture.runtime.layout(
            Constraints::tight(500.0, 100.0),
            Scale::new(1.0),
            &mut fixture.text,
        );
    }

    assert_eq!(
        (
            fixture.file.builds.get(),
            fixture.file.layouts.get(),
            fixture.file.reads.get(),
        ),
        file_before,
        "a chat invalidation must not rebuild, relayout, or reread its sibling",
    );
    assert_eq!(fixture.chat.builds.get() - chat_builds_before, 1_000);
}

#[test]
fn one_thousand_terminal_builds_do_not_touch_file_tree_or_chat_siblings() {
    let mut fixture = fixture();
    let file_before = (
        fixture.file.builds.get(),
        fixture.file.layouts.get(),
        fixture.file.reads.get(),
    );
    let chat_before = (
        fixture.chat.builds.get(),
        fixture.chat.layouts.get(),
        fixture.chat.reads.get(),
    );
    let terminal_builds_before = fixture.terminal.builds.get();

    for revision in 1..=1_000 {
        fixture
            .terminal_signal
            .borrow()
            .as_ref()
            .unwrap()
            .set(revision);
        fixture.runtime.layout(
            Constraints::tight(500.0, 100.0),
            Scale::new(1.0),
            &mut fixture.text,
        );
    }

    assert_eq!(
        (
            fixture.file.builds.get(),
            fixture.file.layouts.get(),
            fixture.file.reads.get(),
        ),
        file_before,
    );
    assert_eq!(
        (
            fixture.chat.builds.get(),
            fixture.chat.layouts.get(),
            fixture.chat.reads.get(),
        ),
        chat_before,
    );
    assert_eq!(
        fixture.terminal.builds.get() - terminal_builds_before,
        1_000
    );
}

#[test]
fn paint_layout_and_build_requests_coalesce_to_the_strongest_level() {
    let fixture = fixture();
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Paint);
    assert!(fixture.runtime.runtime.frame_work_plan().needs_paint());
    assert!(!fixture.runtime.runtime.frame_work_plan().needs_layout());
    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Layout);
    assert!(fixture.runtime.runtime.frame_work_plan().needs_layout());
    assert!(!fixture.runtime.runtime.frame_work_plan().needs_build());
    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Build);
    assert!(fixture.runtime.runtime.frame_work_plan().needs_build());
}

#[test]
fn invalidation_provenance_is_bounded_and_reports_coalescing() {
    let fixture = fixture();
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    for _ in 0..1_000 {
        fixture
            .runtime
            .runtime
            .invalidate(chat, Invalidation::Paint);
    }
    let diagnostics = fixture.runtime.runtime.invalidation_diagnostics();
    assert_eq!(diagnostics.requests, 1_000);
    assert_eq!(diagnostics.paint_requests, 1_000);
    assert_eq!(diagnostics.coalesced_requests, 999);
    assert_eq!(diagnostics.records.len(), INVALIDATION_PROVENANCE_CAPACITY);
    assert_eq!(
        diagnostics.records.last().unwrap().source(),
        InvalidationSource::Runtime,
    );
    assert!(diagnostics.records.last().unwrap().was_coalesced());
}
