//! Opt-in native captures for the framework presentation showcase.
//!
//! One deterministic window captures the branded documentation explorer and
//! all canonical resource destinations. It is ignored by default because it
//! requires a native compositor and WGPU readback.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ailloli_ui::core::event::{
    Event, Key, KeyEvent, KeyState, Modifiers, NamedKey, PointerEvent, WheelDelta,
};
use ailloli_ui::core::{Constraints, Point, Scale};
use ailloli_ui::prelude::*;
use ailloli_ui::runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui::runtime::component::{ComponentNode, Context, Signal};
use ailloli_ui::runtime::element::ElementKind;
use ailloli_ui::runtime::input::{absolute_paint_bounds, InputRouter};
use ailloli_ui::runtime::layout::LayoutArtifact;
use ailloli_ui::text::TextSystem;
use ailloli_ui::{CaptureRequestId, Command};
use ailloli_ui::{DrawCmd, IntoView};

use crate::view::showcase::{
    documentation_capture_root, interactive_scrolling_capture_root, showcase_root,
    InteractiveScrollingState, ShowcaseState,
};

/// Final reviewed presentation capture name.
const PRESENTATION_CAPTURE_NAME: &str = "public_sandbox_showcase.png";
/// Reviewed capture proving reactive geometry is coherent before resize.
const REACTIVE_LAYOUT_CAPTURE_NAME: &str = "reactive_layout_consistency.png";
/// Final reviewed interaction capture name.
const SCROLLING_CAPTURE_NAME: &str = "interactive_scrolling_showcase.png";
/// Single logical window captured by the native test.
const WINDOW_ID: &str = "sandbox-public-showcase";
/// Explicit presentation settle before issuing the readback request.
const CAPTURE_SETTLE: Duration = Duration::from_millis(400);
/// Bounded event-loop lifetime if native capture never completes.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

/// Reducer stage for settle, request, and timeout actions.
#[derive(Default)]
struct CaptureState {
    /// Advances settle, capture, scene switch, second capture, and completion.
    stage: u8,
}

/// Shared public capture services retained by the reducer and assertions.
struct CaptureServices {
    /// Public façade capture handle.
    capture: CaptureHandle,
    /// Request IDs issued only after each explicit settle period.
    request_ids: Arc<Mutex<Vec<(&'static str, CaptureRequestId)>>>,
    /// Number of renderer completions observed through the public listener.
    completed: Arc<AtomicUsize>,
    /// Retained component signal that chooses the second root in the same window.
    interactive: Rc<RefCell<Option<Signal<bool>>>>,
    /// Documentation state mutated between the first and second captures.
    presentation: ShowcaseState,
    /// Monotonic start used by every polling stage as the timeout authority.
    started_at: Instant,
    /// Records whether the bounded timeout forced event-loop exit.
    timed_out: Arc<AtomicBool>,
}

/// Captures three settled states through one public handle, window, and event loop.
fn update_capture(
    state: &mut CaptureState,
    services: &mut CaptureServices,
    _action: (),
) -> Commands<()> {
    if state.stage > 0 && services.started_at.elapsed() >= CAPTURE_TIMEOUT {
        services.timed_out.store(true, Ordering::SeqCst);
        return Commands::quit();
    }

    match state.stage {
        0 => {
            state.stage = 1;
            Commands::dispatch_after((), CAPTURE_SETTLE)
        }
        1 => {
            state.stage = 2;
            let request = services.capture.request_window(WINDOW_ID);
            services
                .request_ids
                .lock()
                .expect("capture request lock")
                .push((PRESENTATION_CAPTURE_NAME, request));
            Commands::redraw().push(Command::DispatchAfter {
                action: (),
                delay: Duration::from_millis(50),
            })
        }
        2 if services.completed.load(Ordering::SeqCst) < 1 => {
            Commands::dispatch_after((), Duration::from_millis(50))
        }
        2 => {
            state.stage = 3;
            services.presentation.select_openxr_for_capture();
            Commands::redraw().push(Command::DispatchAfter {
                action: (),
                delay: CAPTURE_SETTLE,
            })
        }
        3 => {
            state.stage = 4;
            let request = services.capture.request_window(WINDOW_ID);
            services
                .request_ids
                .lock()
                .expect("capture request lock")
                .push((REACTIVE_LAYOUT_CAPTURE_NAME, request));
            Commands::redraw().push(Command::DispatchAfter {
                action: (),
                delay: Duration::from_millis(50),
            })
        }
        4 if services.completed.load(Ordering::SeqCst) < 2 => {
            Commands::dispatch_after((), Duration::from_millis(50))
        }
        4 => {
            state.stage = 5;
            services
                .interactive
                .borrow()
                .as_ref()
                .expect("capture root signal")
                .set(true);
            Commands::redraw().push(Command::DispatchAfter {
                action: (),
                delay: CAPTURE_SETTLE,
            })
        }
        5 => {
            state.stage = 6;
            let request = services.capture.request_window(WINDOW_ID);
            services
                .request_ids
                .lock()
                .expect("capture request lock")
                .push((SCROLLING_CAPTURE_NAME, request));
            Commands::redraw().push(Command::DispatchAfter {
                action: (),
                delay: Duration::from_millis(50),
            })
        }
        6 if services.completed.load(Ordering::SeqCst) < 3 => {
            Commands::dispatch_after((), Duration::from_millis(50))
        }
        _ => Commands::quit(),
    }
}

/// Retained root whose context-owned signal can rebuild between two captures.
#[derive(Clone)]
struct CaptureRoot {
    /// Presentation surface state.
    presentation: ShowcaseState,
    /// Interaction surface state.
    scrolling: InteractiveScrollingState,
    /// Slot shared with the reducer after the first component build.
    interactive: Rc<RefCell<Option<Signal<bool>>>>,
}

impl ComponentNode<()> for CaptureRoot {
    fn build(&self, context: &mut Context<()>) -> View<()> {
        let interactive = context.signal_with_invalidation(false, Invalidation::Build);
        *self.interactive.borrow_mut() = Some(interactive.clone());
        if interactive.read() {
            interactive_scrolling_capture_root(self.scrolling.clone())
        } else {
            documentation_capture_root(self.presentation.clone())
        }
    }
}

/// Returns the framework-local directory reserved for reviewed PNG captures.
fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Checks that one capture has useful dimensions, encoded PNG data, and visual variety.
///
/// # Panics
///
/// Panics when dimensions are smaller than the documented review surface, the
/// RGBA payload is incomplete, the PNG is empty, or the sampled frame lacks
/// enough distinct colors to prove useful rendering.
fn assert_visual_frame(name: &str, width: u32, height: u32, rgba: &[u8], png: &[u8]) {
    assert_eq!(width, 1280, "{name}: width={width}");
    assert_eq!(height, 756, "{name}: height={height}");
    assert!(!png.is_empty(), "{name}: empty PNG payload");
    assert_eq!(
        rgba.len(),
        width as usize * height as usize * 4,
        "{name}: incomplete RGBA frame"
    );

    let distinct_sampled_colors = rgba
        .chunks_exact(4)
        .step_by(64)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(
        distinct_sampled_colors > 20,
        "{name}: distinct sampled colors={distinct_sampled_colors}"
    );
}

/// Returns the first absolute widget bounds for a diagnostic widget name.
fn widget_bounds(app: &Runtime<()>, debug_name: &str) -> Rect {
    if let Some(bounds) = app
        .tree
        .iter_elements()
        .find_map(|(id, element)| match &element.kind {
            ElementKind::Widget(widget) if widget.debug_name() == debug_name => {
                absolute_paint_bounds(&app.tree, id)
            }
            _ => None,
        })
    {
        return bounds;
    }
    let available = app
        .tree
        .iter_elements()
        .filter_map(|(id, element)| match &element.kind {
            ElementKind::Widget(widget) => Some((
                id,
                widget.debug_name(),
                element.layout.is_some(),
                absolute_paint_bounds(&app.tree, id),
            )),
            ElementKind::Empty | ElementKind::Component(_) => None,
        })
        .collect::<Vec<_>>();
    panic!("missing {debug_name} in scrolling showcase; available widgets={available:?}")
}

/// Extracts scrollbar-like rounded rectangles from a public painted scene.
fn scrollbar_rects(app: &Runtime<()>, text_system: &mut TextSystem) -> Vec<Rect> {
    app.paint(text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::RRect(rrect)
                if (rrect.rect.w <= 16.0 && rrect.rect.h >= 18.0)
                    || (rrect.rect.h <= 16.0 && rrect.rect.w >= 18.0) =>
            {
                Some(rrect.rect)
            }
            DrawCmd::Rect(rect)
                if (rect.rect.w <= 16.0 && rect.rect.h >= 18.0)
                    || (rect.rect.h <= 16.0 && rect.rect.w >= 18.0) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect()
}

/// Returns the shortest scrollbar-like rectangle along the widget's right edge.
fn vertical_thumb_in_bounds(app: &Runtime<()>, text_system: &mut TextSystem, bounds: Rect) -> Rect {
    scrollbar_rects(app, text_system)
        .into_iter()
        .filter(|rect| {
            rect.w <= 16.0
                && rect.h >= 18.0
                && rect.x >= bounds.right() - 20.0
                && rect.x <= bounds.right()
                && rect.y >= bounds.y
                && rect.bottom() <= bounds.bottom()
        })
        .min_by(|a, b| a.h.total_cmp(&b.h))
        .unwrap_or_else(|| panic!("missing vertical scrollbar thumb in {bounds:?}"))
}

/// Routes wheel and captured multi-frame thumb drags through every review surface.
fn assert_interactive_scrolling_input_contract(state: InteractiveScrollingState) {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(interactive_scrolling_capture_root(state).into_view());
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(1280.0, 756.0),
        ailloli_ui::core::Scale::new(1.0),
        &mut text_system,
    );
    let mut router = InputRouter::default();
    let constraints = Constraints::tight(1280.0, 756.0);

    for debug_name in [
        "ScrollView",
        "CodeEditor",
        "TextInput",
        "TerminalView",
        "TableView",
    ] {
        let bounds = widget_bounds(&app, debug_name);
        let point = Point::new(bounds.x + bounds.w * 0.5, bounds.y + bounds.h * 0.5);
        let before = scrollbar_rects(&app, &mut text_system);
        let outcome = router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Pointer(PointerEvent::Wheel {
                pos: point,
                delta: WheelDelta::PixelDelta { x: -18.0, y: -42.0 },
                modifiers: Modifiers::default(),
                precise: true,
            }),
        );
        assert!(outcome.event_dispatched, "{debug_name}: wheel not routed");
        if debug_name == "ScrollView" {
            let pre_layout = scrollbar_rects(&app, &mut text_system);
            assert!(
                pre_layout.iter().all(|rect| {
                    rect.x < bounds.right() - 20.0
                        || rect.x > bounds.right()
                        || rect.y < bounds.y
                        || rect.bottom() > bounds.bottom()
                }),
                "ScrollView: stale paint must skip its scrollbar until requested layout completes"
            );
        }
        app.layout(
            constraints,
            ailloli_ui::core::Scale::new(1.0),
            &mut text_system,
        );
        let after = scrollbar_rects(&app, &mut text_system);
        assert_ne!(
            before, after,
            "{debug_name}: wheel did not move a thumb; bounds={bounds:?}"
        );

        let thumb_before = vertical_thumb_in_bounds(&app, &mut text_system, bounds);
        let press = Point::new(
            thumb_before.x + thumb_before.w * 0.5,
            thumb_before.y + thumb_before.h * 0.75,
        );
        router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Pointer(PointerEvent::Button {
                pos: press,
                button: ailloli_ui::core::event::MouseButton::Left,
                pressed: true,
                modifiers: Modifiers::default(),
            }),
        );
        router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Pointer(PointerEvent::Moved {
                pos: Point::new(press.x, press.y - 4.0),
                modifiers: Modifiers::default(),
            }),
        );
        app.layout(
            constraints,
            ailloli_ui::core::Scale::new(1.0),
            &mut text_system,
        );
        router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Pointer(PointerEvent::Moved {
                pos: Point::new(press.x, bounds.y - 200.0),
                modifiers: Modifiers::default(),
            }),
        );
        app.layout(
            constraints,
            ailloli_ui::core::Scale::new(1.0),
            &mut text_system,
        );
        router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Pointer(PointerEvent::Button {
                pos: Point::new(press.x, bounds.y - 200.0),
                button: ailloli_ui::core::event::MouseButton::Left,
                pressed: false,
                modifiers: Modifiers::default(),
            }),
        );
        let thumb_after = vertical_thumb_in_bounds(&app, &mut text_system, bounds);
        assert!(
            thumb_after.y < thumb_before.y,
            "{debug_name}: captured drag did not move continuously; before={thumb_before:?}, after={thumb_after:?}"
        );
    }
}

#[test]
fn sandbox_scrolling_surface_routes_wheel_and_captured_thumb_drag() {
    assert_interactive_scrolling_input_contract(InteractiveScrollingState::new());
}

/// Reproduces caret navigation in the exact retained tree used by the real showcase.
#[test]
fn sandbox_quick_start_arrow_down_keeps_the_visible_viewport_stable() {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(showcase_root(ShowcaseState::new()));
    let mut text_system = TextSystem::new();
    let constraints = Constraints::tight(1280.0, 900.0);
    app.layout(
        constraints,
        ailloli_ui::core::Scale::new(1.0),
        &mut text_system,
    );

    let editor = widget_bounds(&app, "CodeEditor");
    let initial_scene = app.paint(&mut text_system);
    let line_nine_y = visible_code_line_numbers(&initial_scene, editor)
        .into_iter()
        .find_map(|(line, y)| (line == 9).then_some(y))
        .expect("visible quick-start line 9");
    let mut router = InputRouter::default();
    let line_nine = Point::new(editor.x + 100.0, line_nine_y + 9.0);
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: line_nine,
            button: ailloli_ui::core::event::MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    app.layout(
        constraints,
        ailloli_ui::core::Scale::new(1.0),
        &mut text_system,
    );
    let before = visible_code_line_numbers(&app.paint(&mut text_system), editor);

    router.route_event(
        &app.tree,
        runtime,
        &Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::ArrowDown),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: None,
        }),
    );
    app.layout(
        constraints,
        ailloli_ui::core::Scale::new(1.0),
        &mut text_system,
    );
    let after = visible_code_line_numbers(&app.paint(&mut text_system), editor);

    assert_eq!(after, before, "ArrowDown moved the quick-start viewport");
}

/// Returns visible line-number labels and their screen-space baselines.
fn visible_code_line_numbers(scene: &ailloli_ui::runtime::Scene, editor: Rect) -> Vec<(u32, f32)> {
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => text
                .layout
                .text()
                .parse::<u32>()
                .ok()
                .filter(|_| {
                    text.pos[0] >= editor.x
                        && text.pos[0] <= editor.right()
                        && text.pos[1] >= editor.y
                        && text.pos[1] <= editor.bottom()
                })
                .map(|line| (line, text.pos[1])),
            _ => None,
        })
        .collect()
}

/// Writes one reviewed frame to the repository-local capture directory.
///
/// # Panics
///
/// Panics when the capture directory cannot be created or the PNG cannot be
/// written. The helper is test-only and never writes during an ordinary run.
fn write_capture(name: &str, png: &[u8]) {
    let output = captures_dir().join(name);
    std::fs::create_dir_all(output.parent().expect("capture parent"))
        .expect("create capture directory");
    std::fs::write(&output, png).expect("write sandbox capture");
    eprintln!("wrote {}", output.display());
}

/// Returns committed absolute bounds and line count for one exact text artifact.
fn committed_text_geometry(app: &Runtime<()>, content: &str) -> (Rect, usize) {
    app.tree
        .iter_elements()
        .find_map(|(id, element)| {
            let layout = element.layout.as_ref()?;
            let LayoutArtifact::Text(text) = layout.artifact.as_ref()?;
            (text.text() == content).then(|| {
                (
                    absolute_paint_bounds(&app.tree, id).expect("absolute text bounds"),
                    text.lines.len(),
                )
            })
        })
        .unwrap_or_else(|| panic!("missing committed text artifact: {content}"))
}

/// Returns the actual logical-pixel extent emitted for one exact text command.
fn painted_text_geometry(
    scene: &ailloli_ui::runtime::Scene,
    content: &str,
    committed: Rect,
) -> Rect {
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|command| match command {
            DrawCmd::Text(text) if text.layout.text() == content => {
                let first_baseline = text
                    .layout
                    .lines
                    .first()
                    .map_or(0.0, |line| line.baseline_y);
                Some(Rect::new(
                    text.pos[0],
                    text.pos[1] - first_baseline,
                    text.layout.metrics.width,
                    text.layout.metrics.height,
                ))
            }
            _ => None,
        })
        .min_by(|left, right| {
            let left_distance = (left.x - committed.x).powi(2) + (left.y - committed.y).powi(2);
            let right_distance = (right.x - committed.x).powi(2) + (right.y - committed.y).powi(2);
            left_distance.total_cmp(&right_distance)
        })
        .unwrap_or_else(|| panic!("missing painted text command: {content}"))
}

/// Proves the documentation sibling mutation is laid out before any resize.
fn assert_reactive_layout_consistency() {
    let state = ShowcaseState::new();
    let mut app: Runtime<()> = Runtime::new(RuntimeHandle::new());
    app.reconcile(View::component(CaptureRoot {
        presentation: state.clone(),
        scrolling: InteractiveScrollingState::new(),
        interactive: Rc::new(RefCell::new(None)),
    }));
    let mut text_system = TextSystem::new();
    let constraints = Constraints::tight(1280.0, 756.0);
    app.layout(constraints, Scale::new(1.0), &mut text_system);

    state.select_openxr_for_capture();
    let prepared = app.prepare_frame();
    assert!(
        prepared.needs_layout(),
        "selection must schedule native layout"
    );
    app.layout(constraints, Scale::new(1.0), &mut text_system);

    let selected = crate::content::topic("openxr");
    let (title, title_lines) = committed_text_geometry(&app, selected.title);
    let (summary, _) = committed_text_geometry(&app, selected.summary);
    let scene = app.paint(&mut text_system);
    let painted_title = painted_text_geometry(&scene, selected.title, title);
    let painted_summary = painted_text_geometry(&scene, selected.summary, summary);
    assert_eq!(title_lines, 2, "selected title must exercise wrapping");
    assert!(
        summary.y >= title.bottom() + 10.0,
        "summary overlaps title before resize: title={title:?} summary={summary:?}"
    );
    assert!(
        painted_summary.y >= painted_title.bottom() + 10.0,
        "painted summary overlaps title before resize: title={painted_title:?} summary={painted_summary:?}; committed title={title:?} summary={summary:?}"
    );

    app.layout(
        Constraints::tight(1284.0, 760.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let (resized_title, _) = committed_text_geometry(&app, selected.title);
    let (resized_summary, _) = committed_text_geometry(&app, selected.summary);
    assert!(
        resized_summary.y >= resized_title.bottom() + 10.0,
        "resize must not be required to repair reactive geometry"
    );
}

#[test]
fn documentation_selection_reflows_before_resize() {
    assert_reactive_layout_consistency();
}

#[test]
#[ignore = "requires a native compositor and WGPU capture"]
fn sandbox_showcase_visual_capture() {
    assert_reactive_layout_consistency();
    let capture = CaptureHandle::new();
    let request_ids = Arc::new(Mutex::new(Vec::new()));
    let completed = Arc::new(AtomicUsize::new(0));
    let timed_out = Arc::new(AtomicBool::new(false));
    let showcase_state = ShowcaseState::new();
    let scrolling_state = InteractiveScrollingState::new();
    assert_interactive_scrolling_input_contract(scrolling_state.clone());
    let interactive = Rc::new(RefCell::new(None));
    let capture_root = CaptureRoot {
        presentation: showcase_state.clone(),
        scrolling: scrolling_state,
        interactive: interactive.clone(),
    };
    let completed_listener = completed.clone();
    capture.on_complete(Arc::new(move |_id, _result| {
        completed_listener.fetch_add(1, Ordering::SeqCst);
    }));

    App::new()
        .state(CaptureState::default())
        .services(CaptureServices {
            capture: capture.clone(),
            request_ids: request_ids.clone(),
            completed,
            interactive: interactive.clone(),
            presentation: showcase_state.clone(),
            started_at: Instant::now(),
            timed_out: timed_out.clone(),
        })
        .capture(capture.clone())
        .window(
            Window::new(WINDOW_ID)
                .title_text("Ailloli UI public framework showcase")
                .no_chrome()
                .size(1280.0, 756.0)
                .content(move || View::component(capture_root.clone())),
        )
        .update(update_capture)
        .startup_action(())
        .run()
        .expect("sandbox showcase capture app");

    assert!(
        !timed_out.load(Ordering::SeqCst),
        "sandbox capture exceeded the explicit 30-second timeout"
    );
    let captures = request_ids.lock().expect("capture request lock").clone();
    assert_eq!(captures.len(), 3, "all captures must be requested");
    for (name, capture_id) in captures {
        let frame = capture
            .take_result(capture_id)
            .expect("final capture slot")
            .expect("final capture result")
            .frame;
        let png = frame.png_data.as_deref().expect("final PNG data");
        assert_visual_frame(name, frame.width, frame.height, &frame.rgba, png);
        write_capture(name, png);
    }
}
