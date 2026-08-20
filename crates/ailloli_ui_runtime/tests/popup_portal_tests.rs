use ailloli_ui_core::{ElementId, LogicalWindowId, Point, Rect, Size};
use ailloli_ui_runtime::app::{PresentationGeneration, RuntimeHandle};
use ailloli_ui_runtime::component::{View, ViewKind};
use ailloli_ui_runtime::popup::{
    ElementTreeId, PopupAlignment, PopupContent, PopupDismissReason, PopupFocusPolicy, PopupId,
    PopupIntent, PopupMountPolicy, PopupOwner, PopupPlacement, PopupPlacementSpec, PopupPortal,
    PopupPortalError, PopupRequest, PopupRole, PopupSemantics,
};

fn owner(window: &str, generation: u64, tree: u64, element: u64) -> PopupOwner {
    PopupOwner::new(
        window,
        PresentationGeneration::new(generation),
        ElementTreeId::new(tree),
        ElementId(element),
    )
}

fn request(id: u64, owner: PopupOwner) -> PopupRequest<()> {
    PopupRequest::new(PopupId::new(id), owner, PopupContent::new(View::empty))
}

#[test]
fn content_factory_and_request_metadata_are_retained() {
    let popup_owner = owner("main", 3, 7, 11);
    let semantics = PopupSemantics::new()
        .with_role(PopupRole::Menu)
        .with_focus_policy(PopupFocusPolicy::TrapWithinPopup);
    let request = request(5, popup_owner.clone())
        .with_anchor(Rect::new(10.0, 20.0, 30.0, 40.0))
        .with_semantics(semantics);
    let mut portal: PopupPortal<()> = PopupPortal::new();
    portal.register(request).unwrap();

    let stored = portal.request(PopupId::new(5)).unwrap();
    assert_eq!(stored.owner(), &popup_owner);
    assert_eq!(stored.anchor(), Some(Rect::new(10.0, 20.0, 30.0, 40.0)));
    assert_eq!(stored.semantics().role(), PopupRole::Menu);
    assert_eq!(
        stored.semantics().focus_policy(),
        PopupFocusPolicy::TrapWithinPopup
    );
    assert!(matches!(
        portal.build_content(PopupId::new(5)).unwrap().kind,
        ViewKind::Empty
    ));
}

#[test]
fn replacing_content_preserves_open_geometry_and_uses_the_new_factory() {
    let popup_id = PopupId::new(6);
    let mut portal: PopupPortal<()> = PopupPortal::new();
    portal
        .register(PopupRequest::new(
            popup_id,
            owner("main", 3, 7, 12),
            PopupContent::new(|| {
                let mut view = View::empty();
                view.key = Some("before".into());
                view
            }),
        ))
        .unwrap();
    portal
        .set_bounds(popup_id, Rect::new(12.0, 24.0, 80.0, 40.0))
        .unwrap();
    portal.open(popup_id).unwrap();

    portal
        .set_content(
            popup_id,
            PopupContent::new(|| {
                let mut view = View::empty();
                view.key = Some("after".into());
                view
            }),
        )
        .unwrap();

    assert!(portal.is_open(popup_id));
    assert_eq!(
        portal.bounds(popup_id),
        Some(Rect::new(12.0, 24.0, 80.0, 40.0))
    );
    assert_eq!(
        portal.build_content(popup_id).unwrap().key.as_deref(),
        Some("after")
    );
}

#[test]
fn ids_register_open_raise_and_close_without_z_order_duplicates() {
    let mut portal = PopupPortal::new();
    assert_eq!(portal.allocate_id().unwrap(), PopupId::new(1));
    portal
        .register(request(10, owner("main", 1, 1, 1)))
        .unwrap();
    portal
        .register(request(11, owner("main", 1, 1, 2)))
        .unwrap();
    assert_eq!(portal.allocate_id().unwrap(), PopupId::new(12));
    assert_eq!(
        portal.register(request(10, owner("main", 1, 1, 3))),
        Err(PopupPortalError::DuplicateId)
    );

    let opened = portal.open(PopupId::new(10)).unwrap();
    assert!(opened.handled());
    assert_eq!(
        opened.intents(),
        &[PopupIntent::Present {
            popup_id: PopupId::new(10)
        }]
    );
    portal.open(PopupId::new(11)).unwrap();
    let raised = portal.open(PopupId::new(10)).unwrap();
    assert!(raised.handled());
    assert!(raised.intents().is_empty());
    assert_eq!(
        portal.open_ids().collect::<Vec<_>>(),
        [PopupId::new(11), PopupId::new(10)]
    );

    let closed = portal.close(PopupId::new(10));
    assert_eq!(
        closed.intents(),
        &[
            PopupIntent::Dismiss {
                popup_id: PopupId::new(10),
                reason: PopupDismissReason::Programmatic,
            },
            PopupIntent::RestoreFocus {
                owner: owner("main", 1, 1, 1),
            },
        ]
    );
    assert!(!portal.is_open(PopupId::new(10)));
    assert_eq!(portal.topmost(), Some(PopupId::new(11)));
}

#[test]
fn mixed_mount_policies_keep_fixed_strata_and_raise_within_each_stratum() {
    let mut portal = PopupPortal::new();
    for (id, mount_policy) in [
        (1, PopupMountPolicy::ProceduralFallback),
        (2, PopupMountPolicy::ProceduralFallback),
        (3, PopupMountPolicy::RetainedOverlay),
        (4, PopupMountPolicy::RetainedOverlay),
    ] {
        portal
            .register(request(id, owner("main", 1, 1, id)).with_mount_policy(mount_policy))
            .unwrap();
    }

    for id in [3, 1, 4, 2] {
        portal.open(PopupId::new(id)).unwrap();
    }
    assert_eq!(
        portal.open_ids().collect::<Vec<_>>(),
        [
            PopupId::new(1),
            PopupId::new(2),
            PopupId::new(3),
            PopupId::new(4),
        ]
    );
    assert_eq!(portal.topmost(), Some(PopupId::new(4)));

    portal.open(PopupId::new(1)).unwrap();
    assert_eq!(
        portal.open_ids().collect::<Vec<_>>(),
        [
            PopupId::new(2),
            PopupId::new(1),
            PopupId::new(3),
            PopupId::new(4),
        ]
    );

    portal.open(PopupId::new(3)).unwrap();
    assert_eq!(
        portal.open_ids().collect::<Vec<_>>(),
        [
            PopupId::new(2),
            PopupId::new(1),
            PopupId::new(4),
            PopupId::new(3),
        ]
    );
    assert_eq!(portal.topmost(), Some(PopupId::new(3)));
}

#[test]
fn retained_stratum_wins_mixed_hit_outside_dismissal_and_escape() {
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let procedural = PopupId::new(1);
    let retained = PopupId::new(2);
    let mut portal = PopupPortal::new();
    portal
        .register(
            request(1, owner("main", 1, 1, 1))
                .with_mount_policy(PopupMountPolicy::ProceduralFallback),
        )
        .unwrap();
    portal.register(request(2, owner("main", 1, 1, 2))).unwrap();
    portal.open(retained).unwrap();
    portal.open(procedural).unwrap();
    portal
        .set_bounds(procedural, Rect::new(0.0, 0.0, 80.0, 80.0))
        .unwrap();
    portal
        .set_bounds(retained, Rect::new(0.0, 0.0, 40.0, 40.0))
        .unwrap();

    assert_eq!(
        portal.hit_test(&window, generation, Point::new(20.0, 20.0)),
        Some(retained)
    );
    let inside = portal.handle_pointer_press_with_backend_hit(
        &window,
        generation,
        Point::new(20.0, 20.0),
        Some(procedural),
    );
    assert!(inside.handled());
    assert!(portal.is_open(retained));
    assert!(portal.is_open(procedural));

    let procedural_only = portal.handle_pointer_press(&window, generation, Point::new(60.0, 60.0));
    assert!(procedural_only.handled());
    assert!(!portal.is_open(retained));
    assert!(portal.is_open(procedural));

    portal.open(retained).unwrap();
    let escaped = portal.handle_escape(&window, generation);
    assert!(escaped.handled());
    assert!(!portal.is_open(retained));
    assert!(portal.is_open(procedural));
    assert_eq!(portal.topmost(), Some(procedural));

    assert_eq!(
        portal.hit_test(&window, generation, Point::new(60.0, 60.0)),
        Some(procedural)
    );
    let outside = portal.handle_pointer_press(&window, generation, Point::new(90.0, 90.0));
    assert!(outside.handled());
    assert!(!portal.is_open(procedural));
}

#[test]
fn nested_popups_require_an_open_parent_on_the_same_presentation() {
    let mut portal = PopupPortal::new();
    assert_eq!(
        portal.register(request(2, owner("main", 1, 1, 2)).with_parent(PopupId::new(1))),
        Err(PopupPortalError::UnknownParent)
    );

    portal.register(request(1, owner("main", 1, 1, 1))).unwrap();
    assert_eq!(
        portal.register(request(3, owner("other", 1, 1, 3)).with_parent(PopupId::new(1))),
        Err(PopupPortalError::ParentPresentationMismatch)
    );
    portal
        .register(request(2, owner("main", 1, 1, 2)).with_parent(PopupId::new(1)))
        .unwrap();
    assert!(matches!(
        portal.open(PopupId::new(2)),
        Err(PopupPortalError::ParentNotOpen)
    ));

    portal.open(PopupId::new(1)).unwrap();
    portal.open(PopupId::new(2)).unwrap();
    let outcome = portal.close(PopupId::new(1));
    assert_eq!(
        outcome.intents(),
        &[
            PopupIntent::Dismiss {
                popup_id: PopupId::new(2),
                reason: PopupDismissReason::ParentClosed,
            },
            PopupIntent::Dismiss {
                popup_id: PopupId::new(1),
                reason: PopupDismissReason::Programmatic,
            },
            PopupIntent::RestoreFocus {
                owner: owner("main", 1, 1, 1),
            },
        ]
    );
}

#[test]
fn pointer_routing_consumes_inside_and_dismisses_outside() {
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let mut portal = PopupPortal::new();
    portal.register(request(1, owner("main", 1, 1, 1))).unwrap();
    portal.open(PopupId::new(1)).unwrap();
    portal
        .set_bounds(PopupId::new(1), Rect::new(10.0, 10.0, 100.0, 80.0))
        .unwrap();

    let inside = portal.handle_pointer_press(&window, generation, Point::new(20.0, 20.0));
    assert!(inside.handled());
    assert!(inside.intents().is_empty());
    assert!(portal.is_open(PopupId::new(1)));

    let outside = portal.handle_pointer_press(&window, generation, Point::new(200.0, 200.0));
    assert!(outside.handled());
    assert_eq!(
        outside.intents(),
        &[
            PopupIntent::Dismiss {
                popup_id: PopupId::new(1),
                reason: PopupDismissReason::OutsidePress,
            },
            PopupIntent::RestoreFocus {
                owner: owner("main", 1, 1, 1),
            },
        ]
    );
}

#[test]
fn backend_hit_is_authoritative_before_first_bounds_commit() {
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let mut portal = PopupPortal::new();
    portal.register(request(1, owner("main", 1, 1, 1))).unwrap();
    portal.open(PopupId::new(1)).unwrap();
    assert_eq!(portal.bounds(PopupId::new(1)), None);

    let inside = portal.handle_pointer_press_with_backend_hit(
        &window,
        generation,
        Point::new(20.0, 20.0),
        Some(PopupId::new(1)),
    );
    assert!(inside.handled());
    assert!(inside.intents().is_empty());
    assert!(portal.is_open(PopupId::new(1)));

    let outside = portal.handle_pointer_press_with_backend_hit(
        &window,
        generation,
        Point::new(20.0, 20.0),
        None,
    );
    assert!(outside.handled());
    assert!(!portal.is_open(PopupId::new(1)));
}

#[test]
fn clicking_parent_closes_only_the_child_above_it() {
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let mut portal = PopupPortal::new();
    portal.register(request(1, owner("main", 1, 1, 1))).unwrap();
    portal
        .register(request(2, owner("main", 1, 1, 2)).with_parent(PopupId::new(1)))
        .unwrap();
    portal.open(PopupId::new(1)).unwrap();
    portal.open(PopupId::new(2)).unwrap();
    portal
        .set_bounds(PopupId::new(1), Rect::new(0.0, 0.0, 100.0, 100.0))
        .unwrap();
    portal
        .set_bounds(PopupId::new(2), Rect::new(110.0, 0.0, 80.0, 80.0))
        .unwrap();

    let outcome = portal.handle_pointer_press(&window, generation, Point::new(50.0, 50.0));
    assert!(outcome.handled());
    assert!(!portal.is_open(PopupId::new(2)));
    assert!(portal.is_open(PopupId::new(1)));
    assert_eq!(
        outcome.intents(),
        &[
            PopupIntent::Dismiss {
                popup_id: PopupId::new(2),
                reason: PopupDismissReason::OutsidePress,
            },
            PopupIntent::RestoreFocus {
                owner: owner("main", 1, 1, 2),
            },
        ]
    );
}

#[test]
fn escape_respects_topmost_semantics_and_requests_focus_restoration() {
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let mut portal = PopupPortal::new();
    portal.register(request(1, owner("main", 1, 1, 1))).unwrap();
    portal
        .register(
            request(2, owner("main", 1, 1, 2))
                .with_semantics(PopupSemantics::new().dismiss_on_escape(false)),
        )
        .unwrap();
    portal.open(PopupId::new(1)).unwrap();
    portal.open(PopupId::new(2)).unwrap();

    assert!(!portal.handle_escape(&window, generation).handled());
    assert!(portal.is_open(PopupId::new(2)));
    portal.close(PopupId::new(2));

    let outcome = portal.handle_escape(&window, generation);
    assert!(outcome.handled());
    assert_eq!(
        outcome.intents(),
        &[
            PopupIntent::Dismiss {
                popup_id: PopupId::new(1),
                reason: PopupDismissReason::Escape,
            },
            PopupIntent::RestoreFocus {
                owner: owner("main", 1, 1, 1),
            },
        ]
    );
}

#[test]
fn stale_generation_and_removed_owner_cannot_leave_registered_popups() {
    let mut portal = PopupPortal::new();
    portal.register(request(1, owner("main", 1, 1, 1))).unwrap();
    portal.register(request(2, owner("main", 2, 1, 2))).unwrap();
    portal
        .register(request(3, owner("other", 1, 1, 3)))
        .unwrap();
    portal.open(PopupId::new(1)).unwrap();
    portal.open(PopupId::new(2)).unwrap();
    portal.open(PopupId::new(3)).unwrap();

    let stale = portal.close_stale_presentations(
        &LogicalWindowId::new("main"),
        PresentationGeneration::new(2),
    );
    assert!(stale.handled());
    assert_eq!(
        stale.intents(),
        &[PopupIntent::Dismiss {
            popup_id: PopupId::new(1),
            reason: PopupDismissReason::PresentationStale,
        }]
    );
    assert!(!portal.contains(PopupId::new(1)));
    assert!(portal.contains(PopupId::new(2)));
    assert!(portal.contains(PopupId::new(3)));

    let removed = portal.prune_stale_owners(|owner| owner.element_id() != ElementId(2));
    assert!(removed.handled());
    assert_eq!(
        removed.intents(),
        &[PopupIntent::Dismiss {
            popup_id: PopupId::new(2),
            reason: PopupDismissReason::OwnerRemoved,
        }]
    );
    assert!(!portal.contains(PopupId::new(2)));
    assert!(portal.contains(PopupId::new(3)));
}

#[test]
fn tree_scoped_owner_pruning_preserves_sibling_runtime_trees() {
    let mut portal = PopupPortal::new();
    portal
        .register(request(1, owner("main", 1, 1, 10)))
        .unwrap();
    portal
        .register(request(2, owner("other", 1, 2, 10)))
        .unwrap();
    portal.open(PopupId::new(1)).unwrap();
    portal.open(PopupId::new(2)).unwrap();

    let removed = portal.prune_stale_owners_in_tree(ElementTreeId::new(1), |_| false);
    assert!(removed.handled());
    assert!(!portal.contains(PopupId::new(1)));
    assert!(portal.contains(PopupId::new(2)));
    assert!(portal.is_open(PopupId::new(2)));
}

#[test]
fn tooltip_semantics_do_not_capture_or_restore_focus() {
    let semantics = PopupSemantics::tooltip();
    assert_eq!(semantics.role(), PopupRole::Tooltip);
    assert!(!semantics.dismisses_on_outside_press());
    assert!(!semantics.consumes_pointer_input());
    assert!(!semantics.restores_focus_on_close());

    let mut portal = PopupPortal::new();
    portal
        .register(request(1, owner("main", 1, 1, 1)).with_semantics(semantics))
        .unwrap();
    portal.open(PopupId::new(1)).unwrap();
    portal
        .set_bounds(PopupId::new(1), Rect::new(10.0, 10.0, 100.0, 20.0))
        .unwrap();

    let outcome = portal.handle_pointer_press(
        &LogicalWindowId::new("main"),
        PresentationGeneration::new(1),
        Point::new(20.0, 15.0),
    );
    assert!(!outcome.handled());
    assert!(portal.is_open(PopupId::new(1)));
    let closed = portal.close(PopupId::new(1));
    assert_eq!(
        closed.intents(),
        &[PopupIntent::Dismiss {
            popup_id: PopupId::new(1),
            reason: PopupDismissReason::Programmatic,
        }]
    );
}

#[test]
fn bounds_validation_and_unregister_are_typed_and_deterministic() {
    let mut portal = PopupPortal::new();
    portal.register(request(1, owner("main", 1, 1, 1))).unwrap();
    assert_eq!(
        portal.set_bounds(PopupId::new(1), Rect::new(0.0, 0.0, -1.0, 2.0)),
        Err(PopupPortalError::InvalidBounds)
    );
    assert_eq!(
        portal.clear_bounds(PopupId::new(99)),
        Err(PopupPortalError::UnknownPopup)
    );
    assert_eq!(
        portal.set_anchor(PopupId::new(1), Some(Rect::new(f32::NAN, 0.0, 1.0, 2.0))),
        Err(PopupPortalError::InvalidBounds)
    );

    let anchor = Rect::new(4.0, 5.0, 20.0, 12.0);
    portal.set_anchor(PopupId::new(1), Some(anchor)).unwrap();
    assert_eq!(
        portal.request(PopupId::new(1)).unwrap().anchor(),
        Some(anchor)
    );

    let outcome = portal.unregister(PopupId::new(1));
    assert!(outcome.handled());
    assert!(outcome.intents().is_empty());
    assert!(!portal.contains(PopupId::new(1)));
    assert!(!portal.unregister(PopupId::new(1)).handled());
}

#[test]
fn runtime_handle_owns_ids_and_records_portal_intents() {
    let runtime = RuntimeHandle::<()>::new();
    let element = ElementId(42);
    let popup_id = runtime.popup_id_for_element(element).unwrap();
    assert_eq!(runtime.popup_id_for_element(element).unwrap(), popup_id);

    runtime
        .register_popup(PopupRequest::new(
            popup_id,
            owner("main", 2, 0, 42),
            PopupContent::new(View::empty),
        ))
        .unwrap();
    let anchor = Rect::new(10.0, 20.0, 80.0, 30.0);
    let bounds = Rect::new(10.0, 54.0, 160.0, 120.0);
    runtime.open_popup(popup_id, anchor, bounds).unwrap();

    assert!(runtime.popup_is_open(popup_id));
    assert_eq!(
        runtime.popup_portal().borrow().bounds(popup_id),
        Some(bounds)
    );
    assert_eq!(
        runtime.take_popup_intents(),
        [PopupIntent::Present { popup_id }]
    );

    runtime.close_popup(popup_id, PopupDismissReason::Escape);
    assert!(!runtime.popup_is_open(popup_id));
    assert!(runtime.take_popup_intents().iter().any(|intent| matches!(
        intent,
        PopupIntent::Dismiss {
            popup_id: id,
            reason: PopupDismissReason::Escape,
        } if *id == popup_id
    )));
    assert!(runtime.take_popup_errors().is_empty());
}

#[test]
fn runtime_handle_opens_a_popup_from_provider_neutral_placement_inputs() {
    let runtime = RuntimeHandle::<()>::new();
    let popup_id = PopupId::new(12);
    runtime
        .register_popup(request(12, owner("main", 1, 0, 12)))
        .unwrap();
    {
        let portal = runtime.popup_portal();
        portal
            .borrow_mut()
            .set_bounds(popup_id, Rect::new(1.0, 2.0, 3.0, 4.0))
            .unwrap();
    }

    let anchor = Rect::new(75.0, 80.0, 0.0, 0.0);
    let desired_size = Size::new(252.0, 93.0);
    runtime
        .open_popup_placed(
            popup_id,
            PopupPlacementSpec::new(anchor, desired_size)
                .with_placement(PopupPlacement::Bottom)
                .with_alignment(PopupAlignment::Start)
                .with_gap(0.0)
                .with_flip(true),
        )
        .unwrap();

    let portal = runtime.popup_portal();
    let portal = portal.borrow();
    let stored = portal.request(popup_id).unwrap();
    assert_eq!(stored.anchor(), Some(anchor));
    assert_eq!(stored.desired_size(), Some(desired_size));
    assert_eq!(stored.placement(), PopupPlacement::Bottom);
    assert_eq!(stored.alignment(), PopupAlignment::Start);
    assert_eq!(portal.bounds(popup_id), None);
    assert!(portal.is_open(popup_id));
    drop(portal);

    assert_eq!(
        runtime.take_popup_intents(),
        [PopupIntent::Present { popup_id }]
    );
    assert!(runtime.take_popup_errors().is_empty());
}
