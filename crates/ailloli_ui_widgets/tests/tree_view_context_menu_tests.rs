//! Tree row/blank right-click context-menu selection and geometry scenarios.

use ailloli_ui_core::event::{Event, Modifiers, MouseButton, PointerEvent};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Point;
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{TreeContextMenu, TreeNode, TreeView};

#[derive(Debug, Clone, PartialEq)]
enum Action {
    Select(&'static str),
    Context(TreeContextMenu<&'static str>),
}

#[test]
fn tree_view_context_menu_right_click_selects_unselected_row_then_emits_row_request() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::new()
            .node(TreeNode::leaf("a", "alpha"))
            .node(TreeNode::leaf("b", "beta"))
            .on_select(Action::Select)
            .on_context_menu(Action::Context)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &right_click(20.0, 48.0));

    let actions = runtime.take_actions();
    assert_eq!(actions.first(), Some(&Action::Select("b")));
    assert!(matches!(
        actions.get(1),
        Some(Action::Context(TreeContextMenu::Row { row_id: "b", .. }))
    ));
}

#[test]
fn tree_view_context_menu_right_click_blank_emits_blank_request() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        TreeView::new()
            .node(TreeNode::leaf("a", "alpha"))
            .on_context_menu(Action::Context)
            .height(160.0)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &right_click(20.0, 120.0));

    assert!(matches!(
        runtime.take_actions().as_slice(),
        [Action::Context(TreeContextMenu::Blank { .. })]
    ));
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(320.0, 180.0),
        Scale::new(1.0),
        &mut text_system,
    );
}

fn right_click(x: f32, y: f32) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Right,
        pressed: true,
        modifiers: Modifiers::default(),
    })
}
