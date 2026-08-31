//! Presentation-scoped retained-work aggregation regressions.

use std::cell::Cell;
use std::rc::Rc;

use ailloli_ui_core::{ElementId, LogicalWindowId};
use ailloli_ui_runtime::app::{Invalidation, PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{Context, View};
use ailloli_ui_runtime::popup::{
    PopupContent, PopupDismissReason, PopupId, PopupOwner, PopupRequest,
};

#[test]
fn pending_work_is_sorted_merged_and_limited_to_scoped_dirty_presentations() {
    let shared = RuntimeHandle::<()>::new();
    let alpha_owner = Runtime::new(shared.clone());
    let alpha_popup = Runtime::new(shared.clone());
    let beta_owner = Runtime::new(shared.clone());
    let stable = Runtime::new(shared.clone());
    let unscoped = Runtime::new(shared);
    let alpha = LogicalWindowId::new("alpha");
    let beta = LogicalWindowId::new("beta");
    let gamma = LogicalWindowId::new("gamma");
    let generation = PresentationGeneration::new(4);

    alpha_owner
        .runtime
        .set_presentation_scope(alpha.clone(), generation);
    alpha_popup
        .runtime
        .set_presentation_scope(alpha.clone(), generation);
    beta_owner
        .runtime
        .set_presentation_scope(beta.clone(), generation);
    stable.runtime.set_presentation_scope(gamma, generation);

    alpha_owner
        .runtime
        .invalidate(ElementId(1), Invalidation::Paint);
    alpha_popup
        .runtime
        .invalidate(ElementId(2), Invalidation::Layout);
    beta_owner
        .runtime
        .invalidate(ElementId(3), Invalidation::Build);
    unscoped
        .runtime
        .invalidate(ElementId(4), Invalidation::Build);

    let plans = alpha_owner.runtime.pending_presentation_work();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].logical_window_id(), &alpha);
    assert_eq!(plans[0].presentation_generation(), generation);
    assert!(!plans[0].frame_work_plan().needs_build());
    assert!(plans[0].frame_work_plan().needs_layout());
    assert_eq!(plans[1].logical_window_id(), &beta);
    assert!(plans[1].frame_work_plan().needs_build());

    assert_eq!(alpha_owner.runtime.pending_presentation_work(), plans);
}

#[test]
fn clearing_or_releasing_a_scope_stops_only_that_presentation_wake() {
    let shared = RuntimeHandle::<()>::new();
    let alpha = Runtime::new(shared.clone());
    let beta = Runtime::new(shared);
    let alpha_id = LogicalWindowId::new("alpha");
    let beta_id = LogicalWindowId::new("beta");
    let generation = PresentationGeneration::new(2);

    alpha
        .runtime
        .set_presentation_scope(alpha_id.clone(), generation);
    beta.runtime
        .set_presentation_scope(beta_id.clone(), generation);
    alpha.runtime.request_repaint(ElementId(1));
    beta.runtime.request_repaint(ElementId(2));

    alpha.runtime.clear_presentation_scope();
    let plans = beta.runtime.pending_presentation_work();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].logical_window_id(), &beta_id);

    alpha.runtime.take_dirty_elements();
    beta.runtime.take_dirty_elements();
    drop(alpha);
    beta.runtime.request_layout(ElementId(3));

    let plans = beta.runtime.pending_presentation_work();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].logical_window_id(), &beta_id);
    assert!(plans[0].frame_work_plan().needs_layout());
    assert!(!plans
        .iter()
        .any(|plan| plan.logical_window_id() == &alpha_id));
}

#[test]
fn changed_context_service_builds_only_its_exact_presentation_owner() {
    let shared = RuntimeHandle::<()>::new();
    let alpha = Runtime::new(shared.clone());
    let beta = Runtime::new(shared);
    let alpha_id = LogicalWindowId::new("alpha");
    let beta_id = LogicalWindowId::new("beta");
    let generation = PresentationGeneration::new(6);
    alpha
        .runtime
        .set_presentation_scope(alpha_id.clone(), generation);
    beta.runtime
        .set_presentation_scope(beta_id.clone(), generation);

    let changed = Rc::new(Cell::new(false));
    let changed_for_service = Rc::clone(&changed);
    let service: Rc<dyn Fn() -> bool> = Rc::new(move || changed_for_service.replace(false));
    let _registration =
        Context::new(ElementId(11), alpha.runtime.clone()).register_ui_service(&service);

    assert!(!alpha.runtime.service_ui_sources());
    assert!(alpha.runtime.pending_presentation_work().is_empty());

    changed.set(true);
    assert!(alpha.runtime.service_ui_sources());
    let plans = alpha.runtime.pending_presentation_work();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].logical_window_id(), &alpha_id);
    assert_eq!(plans[0].presentation_generation(), generation);
    assert!(plans[0].frame_work_plan().needs_build());
    assert!(!plans
        .iter()
        .any(|plan| plan.logical_window_id() == &beta_id));
}

#[test]
fn low_level_ui_service_never_guesses_a_presentation_owner() {
    let runtime = RuntimeHandle::<()>::new();
    let service: Rc<dyn Fn() -> bool> = Rc::new(|| true);
    let _registration = runtime.register_ui_service(&service);

    assert!(runtime.service_ui_sources());
    assert!(!runtime.has_dirty_elements());
    assert!(runtime.pending_presentation_work().is_empty());
}

#[test]
fn programmatic_popup_lifecycle_wakes_only_the_current_owner_generation() {
    let shared = RuntimeHandle::<()>::new();
    let alpha = Runtime::new(shared.clone());
    let beta = Runtime::new(shared);
    let alpha_id = LogicalWindowId::new("alpha");
    let beta_id = LogicalWindowId::new("beta");
    let current_generation = PresentationGeneration::new(9);
    alpha
        .runtime
        .set_presentation_scope(alpha_id.clone(), current_generation);
    beta.runtime
        .set_presentation_scope(beta_id.clone(), current_generation);

    let popup_id = PopupId::new(91);
    alpha
        .runtime
        .register_popup(PopupRequest::new(
            popup_id,
            PopupOwner::new(
                alpha_id.clone(),
                current_generation,
                alpha.runtime.element_tree_id(),
                ElementId(21),
            ),
            PopupContent::new(View::empty),
        ))
        .unwrap();

    alpha.runtime.open_popup_unpositioned(popup_id).unwrap();
    let open = alpha.runtime.pending_presentation_work();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].logical_window_id(), &alpha_id);
    assert_eq!(open[0].presentation_generation(), current_generation);
    assert!(!open[0].frame_work_plan().needs_build());
    assert!(!open[0].frame_work_plan().needs_layout());
    assert!(open[0].frame_work_plan().needs_paint());
    assert!(!open.iter().any(|plan| plan.logical_window_id() == &beta_id));

    assert_eq!(alpha.runtime.take_dirty_elements(), [ElementId(21)]);
    alpha.runtime.open_popup_unpositioned(popup_id).unwrap();
    assert!(alpha.runtime.pending_presentation_work().is_empty());

    alpha
        .runtime
        .close_popup(popup_id, PopupDismissReason::Programmatic);
    let close = alpha.runtime.pending_presentation_work();
    assert_eq!(close.len(), 1);
    assert_eq!(close[0].logical_window_id(), &alpha_id);
    assert_eq!(close[0].presentation_generation(), current_generation);
    assert!(close[0].frame_work_plan().needs_paint());
    alpha.runtime.take_dirty_elements();

    let stale_popup_id = PopupId::new(92);
    alpha
        .runtime
        .register_popup(PopupRequest::new(
            stale_popup_id,
            PopupOwner::new(
                alpha_id,
                PresentationGeneration::new(8),
                alpha.runtime.element_tree_id(),
                ElementId(22),
            ),
            PopupContent::new(View::empty),
        ))
        .unwrap();
    alpha
        .runtime
        .open_popup_unpositioned(stale_popup_id)
        .unwrap();
    assert!(alpha.runtime.pending_presentation_work().is_empty());
}
