use std::rc::Rc;
use std::time::{Duration, Instant};

use ailloli_ui_core::{ElementId, LogicalWindowId};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{component, Context, View};
use ailloli_ui_runtime::popup::{PopupContent, PopupIntent, PopupOwner, PopupRequest};

fn bool_root(context: &mut Context<()>, (): ()) -> View<()> {
    let value = context.signal(false);
    assert!(!value.read());
    View::empty()
}

fn string_root(context: &mut Context<()>, (): ()) -> View<()> {
    let value = context.signal(String::from("second tree"));
    assert_eq!(value.read(), "second tree");
    View::empty()
}

fn two_trees() -> (RuntimeHandle<()>, Runtime<()>, Runtime<()>) {
    let shared = RuntimeHandle::new();
    let first = Runtime::new(shared.clone());
    let second = Runtime::new(shared.clone());
    assert_ne!(
        first.runtime.element_tree_id(),
        second.runtime.element_tree_id()
    );
    (shared, first, second)
}

#[test]
fn equal_slots_with_different_types_are_isolated_by_tree() {
    let (_shared, mut first, mut second) = two_trees();

    let first_root = first.reconcile(component((), bool_root));
    let second_root = second.reconcile(component((), string_root));

    assert_eq!(first_root, second_root, "element ids are tree-local");
    // A second build reuses each tree's own state value and type.
    first.reconcile(component((), bool_root));
    second.reconcile(component((), string_root));
}

#[test]
fn dirty_elements_are_consumed_only_by_their_tree() {
    let (_shared, first, second) = two_trees();
    let element = ElementId(7);

    first.runtime.mark_dirty(element);

    assert!(first.runtime.has_dirty_elements());
    assert!(!second.runtime.has_dirty_elements());
    assert!(second.runtime.take_dirty_elements().is_empty());
    assert_eq!(first.runtime.take_dirty_elements(), [element]);
}

#[test]
fn repaint_timers_keep_their_tree_when_promoted_globally() {
    let (shared, first, second) = two_trees();
    let element = ElementId(9);

    first.runtime.request_repaint_after(element, Duration::ZERO);
    second
        .runtime
        .request_repaint_after(element, Duration::ZERO);

    assert_eq!(shared.promote_due_scheduled_repaints(Instant::now()), 2);
    assert_eq!(first.runtime.take_dirty_elements(), [element]);
    assert_eq!(second.runtime.take_dirty_elements(), [element]);
    assert!(shared.next_scheduled_repaint_due_global().is_none());
}

#[test]
fn focus_requests_are_isolated_by_tree() {
    let (_shared, first, second) = two_trees();

    first.runtime.request_focus_key("first-focus");
    second.runtime.request_focus_key("second-focus");

    assert_eq!(
        second.runtime.take_focus_key_request().as_deref(),
        Some("second-focus")
    );
    assert_eq!(
        first.runtime.take_focus_key_request().as_deref(),
        Some("first-focus")
    );
    assert!(first.runtime.take_focus_key_request().is_none());
    assert!(second.runtime.take_focus_key_request().is_none());
}

#[test]
fn popup_ids_are_stable_per_element_and_distinct_between_trees() {
    let (_shared, first, second) = two_trees();
    let element = ElementId(11);

    let first_id = first.runtime.popup_id_for_element(element).unwrap();
    let second_id = second.runtime.popup_id_for_element(element).unwrap();

    assert_ne!(first_id, second_id);
    assert_eq!(
        first.runtime.popup_id_for_element(element).unwrap(),
        first_id
    );
    assert_eq!(
        second.runtime.popup_id_for_element(element).unwrap(),
        second_id
    );
    assert!(Rc::ptr_eq(
        &first.runtime.popup_portal(),
        &second.runtime.popup_portal()
    ));
}

#[test]
fn dropping_runtime_releases_only_its_complete_tree_namespace() {
    let (shared, mut first, mut sibling) = two_trees();
    let first_root = first.reconcile(component((), bool_root));
    let sibling_root = sibling.reconcile(component((), bool_root));
    let first_handle = first.runtime.clone();
    let sibling_handle = sibling.runtime.clone();
    let first_tree = first_handle.element_tree_id();
    let sibling_tree = sibling_handle.element_tree_id();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(5);

    let first_popup = first_handle.popup_id_for_element(first_root).unwrap();
    let sibling_popup = sibling_handle.popup_id_for_element(sibling_root).unwrap();
    let nested_popup = shared.popup_portal().borrow_mut().allocate_id().unwrap();
    {
        let portal = shared.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .register(PopupRequest::new(
                first_popup,
                PopupOwner::new(window.clone(), generation, first_tree, first_root),
                PopupContent::new(View::empty),
            ))
            .unwrap();
        portal
            .register(
                PopupRequest::new(
                    nested_popup,
                    PopupOwner::new(window.clone(), generation, first_tree, ElementId(99)),
                    PopupContent::new(View::empty),
                )
                .with_parent(first_popup),
            )
            .unwrap();
        portal
            .register(PopupRequest::new(
                sibling_popup,
                PopupOwner::new(window.clone(), generation, sibling_tree, sibling_root),
                PopupContent::new(View::empty),
            ))
            .unwrap();
    }
    first_handle.open_popup_unpositioned(first_popup).unwrap();
    first_handle.open_popup_unpositioned(nested_popup).unwrap();
    sibling_handle
        .open_popup_unpositioned(sibling_popup)
        .unwrap();

    first_handle.mark_dirty(first_root);
    first_handle.request_repaint_after(first_root, Duration::from_secs(60));
    first_handle.request_focus_key("first-focus");
    first_handle.set_presentation_scope(window.clone(), generation);
    sibling_handle.mark_dirty(sibling_root);
    sibling_handle.request_repaint_after(sibling_root, Duration::from_secs(60));
    sibling_handle.request_focus_key("sibling-focus");
    sibling_handle.set_presentation_scope(window.clone(), generation);

    let sibling_state = shared.states().borrow_mut().signal_scoped(
        sibling_tree,
        sibling_root,
        0,
        false,
        Rc::new(|| {}),
    );
    sibling_state.set(true);

    drop(first);

    assert!(!first_handle.has_dirty_elements());
    assert!(first_handle.next_scheduled_repaint_due().is_none());
    assert!(first_handle.take_focus_key_request().is_none());
    assert_eq!(first_handle.presentation_scope(), None);
    assert!(!shared.popup_portal().borrow().contains(first_popup));
    assert!(!shared.popup_portal().borrow().contains(nested_popup));
    assert_ne!(
        first_handle.popup_id_for_element(first_root).unwrap(),
        first_popup,
        "the stable popup-id mapping belongs to the released namespace"
    );
    let replacement_state = shared.states().borrow_mut().signal_scoped(
        first_tree,
        first_root,
        0,
        String::from("released"),
        Rc::new(|| {}),
    );
    assert_eq!(replacement_state.read(), "released");

    assert!(sibling_handle.has_dirty_elements());
    assert!(sibling_handle.next_scheduled_repaint_due().is_some());
    assert_eq!(
        sibling_handle.take_focus_key_request().as_deref(),
        Some("sibling-focus")
    );
    assert_eq!(
        sibling_handle.presentation_scope(),
        Some((window, generation))
    );
    assert!(shared.popup_portal().borrow().contains(sibling_popup));
    assert_eq!(
        sibling_handle.popup_id_for_element(sibling_root).unwrap(),
        sibling_popup
    );
    assert!(sibling_state.read(), "sibling state must survive cleanup");
    assert_eq!(
        shared.take_popup_intents(),
        [PopupIntent::Present {
            popup_id: sibling_popup
        }],
        "diagnostic intents for released popup ids are purged"
    );
}
