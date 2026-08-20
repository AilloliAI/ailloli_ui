use ailloli_ui_core::{ElementId, Rect, Size};
use ailloli_ui_runtime::app::PresentationGeneration;
use ailloli_ui_runtime::component::View;
use ailloli_ui_runtime::popup::{
    resolve_popup_placement, ElementTreeId, PopupAlignment, PopupBackend, PopupBackendCapabilities,
    PopupContent, PopupId, PopupOwner, PopupPlacement, PopupPlacementError, PopupPlacementInput,
    PopupPlacementSpec, PopupPortal, PopupRequest,
};

fn resolve(input: PopupPlacementInput) -> ailloli_ui_runtime::popup::ResolvedPopupPlacement {
    resolve_popup_placement(input, PopupBackendCapabilities::overlay_only()).unwrap()
}

fn owner(element: u64) -> PopupOwner {
    PopupOwner::new(
        "main",
        PresentationGeneration::new(3),
        ElementTreeId::new(7),
        ElementId(element),
    )
}

fn content() -> PopupContent<()> {
    PopupContent::new(View::empty)
}

#[test]
fn top_and_bottom_respect_anchor_and_gap() {
    let anchor = Rect::new(40.0, 40.0, 20.0, 10.0);
    let viewport = Rect::new(0.0, 0.0, 200.0, 200.0);
    let desired = Size::new(30.0, 15.0);

    let bottom = resolve(
        PopupPlacementInput::new(anchor, desired, viewport)
            .with_alignment(PopupAlignment::Start)
            .with_placement(PopupPlacement::Bottom)
            .with_gap(5.0),
    );
    assert_eq!(bottom.bounds(), Rect::new(40.0, 55.0, 30.0, 15.0));
    assert_eq!(bottom.placement(), PopupPlacement::Bottom);
    assert!(!bottom.flipped());
    assert!(!bottom.clamped());

    let top = resolve(
        PopupPlacementInput::new(anchor, desired, viewport)
            .with_alignment(PopupAlignment::Start)
            .with_placement(PopupPlacement::Top)
            .with_gap(5.0),
    );
    assert_eq!(top.bounds(), Rect::new(40.0, 20.0, 30.0, 15.0));
    assert_eq!(top.placement(), PopupPlacement::Top);
    assert!(!top.flipped());
}

#[test]
fn start_center_and_end_align_against_anchor() {
    let anchor = Rect::new(40.0, 20.0, 20.0, 10.0);
    let viewport = Rect::new(0.0, 0.0, 200.0, 200.0);
    let desired = Size::new(30.0, 15.0);

    let bounds = |alignment| {
        resolve(
            PopupPlacementInput::new(anchor, desired, viewport)
                .with_placement(PopupPlacement::Bottom)
                .with_alignment(alignment),
        )
        .bounds()
    };
    assert_eq!(bounds(PopupAlignment::Start).x, 40.0);
    assert_eq!(bounds(PopupAlignment::Center).x, 35.0);
    assert_eq!(bounds(PopupAlignment::End).x, 30.0);
}

#[test]
fn vertical_overflow_flips_only_when_opposite_side_is_better() {
    let viewport = Rect::new(0.0, 0.0, 100.0, 100.0);
    let bottom_anchor = Rect::new(10.0, 80.0, 20.0, 10.0);
    let desired = Size::new(30.0, 30.0);

    let flipped = resolve(
        PopupPlacementInput::new(bottom_anchor, desired, viewport)
            .with_alignment(PopupAlignment::Start)
            .with_placement(PopupPlacement::Bottom)
            .with_gap(4.0),
    );
    assert_eq!(flipped.placement(), PopupPlacement::Top);
    assert_eq!(flipped.bounds(), Rect::new(10.0, 46.0, 30.0, 30.0));
    assert!(flipped.flipped());

    let kept = resolve(
        PopupPlacementInput::new(bottom_anchor, desired, viewport)
            .with_alignment(PopupAlignment::Start)
            .with_placement(PopupPlacement::Bottom)
            .with_gap(4.0)
            .with_flip(false),
    );
    assert_eq!(kept.placement(), PopupPlacement::Bottom);
    assert_eq!(kept.bounds(), Rect::new(10.0, 70.0, 30.0, 30.0));
    assert!(!kept.flipped());
    assert!(kept.clamped());
}

#[test]
fn oversize_and_off_edge_geometry_is_clamped_to_viewport() {
    let resolved = resolve(
        PopupPlacementInput::new(
            Rect::new(95.0, 30.0, 10.0, 10.0),
            Size::new(60.0, 120.0),
            Rect::new(10.0, 10.0, 100.0, 80.0),
        )
        .with_alignment(PopupAlignment::Start)
        .with_placement(PopupPlacement::Bottom)
        .with_flip(false),
    );

    assert_eq!(resolved.bounds(), Rect::new(50.0, 10.0, 60.0, 80.0));
    assert!(resolved.clamped());
}

#[test]
fn invalid_and_empty_viewports_are_rejected_without_panicking() {
    let anchor = Rect::new(0.0, 0.0, 10.0, 10.0);
    let desired = Size::new(20.0, 10.0);
    let resolve_error = |input| {
        resolve_popup_placement(input, PopupBackendCapabilities::overlay_only()).unwrap_err()
    };

    assert_eq!(
        resolve_error(PopupPlacementInput::new(
            anchor,
            desired,
            Rect::new(0.0, 0.0, 0.0, 100.0),
        )),
        PopupPlacementError::EmptyViewport
    );
    assert_eq!(
        resolve_error(PopupPlacementInput::new(
            anchor,
            desired,
            Rect::new(0.0, 0.0, -1.0, 100.0),
        )),
        PopupPlacementError::InvalidViewport
    );
    assert_eq!(
        resolve_error(PopupPlacementInput::new(
            anchor,
            desired,
            Rect::new(f32::NAN, 0.0, 100.0, 100.0),
        )),
        PopupPlacementError::InvalidViewport
    );
    assert_eq!(
        resolve_error(
            PopupPlacementInput::new(
                Rect::new(0.0, 0.0, -1.0, 10.0),
                desired,
                Rect::new(0.0, 0.0, 100.0, 100.0),
            )
            .with_gap(f32::NAN),
        ),
        PopupPlacementError::InvalidAnchor,
        "validation order is deterministic"
    );
    assert_eq!(
        resolve_error(
            PopupPlacementInput::new(
                anchor,
                Size::new(f32::INFINITY, 10.0),
                Rect::new(0.0, 0.0, 100.0, 100.0),
            )
            .with_gap(-1.0),
        ),
        PopupPlacementError::InvalidDesiredSize
    );
}

#[test]
fn native_preference_falls_back_unless_host_explicitly_supports_it() {
    let input = PopupPlacementInput::new(
        Rect::new(10.0, 10.0, 20.0, 10.0),
        Size::new(30.0, 20.0),
        Rect::new(0.0, 0.0, 100.0, 100.0),
    )
    .with_backend(PopupBackend::Native);

    let fallback =
        resolve_popup_placement(input, PopupBackendCapabilities::overlay_only()).unwrap();
    assert_eq!(fallback.backend().requested(), PopupBackend::Native);
    assert_eq!(fallback.backend().selected(), PopupBackend::Overlay);
    assert!(fallback.backend().fell_back());

    let native =
        resolve_popup_placement(input, PopupBackendCapabilities::native_and_overlay()).unwrap();
    assert_eq!(native.backend().selected(), PopupBackend::Native);
    assert!(!native.backend().fell_back());
}

#[test]
fn popup_request_retains_positioning_and_parent_contract() {
    let parent_id = PopupId::new(1);
    let child_id = PopupId::new(2);
    let mut portal = PopupPortal::new();
    portal
        .register(PopupRequest::new(parent_id, owner(10), content()))
        .unwrap();

    let request = PopupRequest::new(child_id, owner(11), content())
        .with_parent(parent_id)
        .with_anchor(Rect::new(80.0, 70.0, 10.0, 10.0))
        .with_desired_size(Size::new(30.0, 25.0))
        .with_placement(PopupPlacement::Bottom)
        .with_alignment(PopupAlignment::End)
        .with_gap(5.0)
        .with_flip(true)
        .with_backend(PopupBackend::Native);
    assert_eq!(request.parent(), Some(parent_id));
    assert_eq!(request.desired_size(), Some(Size::new(30.0, 25.0)));
    assert_eq!(request.placement(), PopupPlacement::Bottom);
    assert_eq!(request.alignment(), PopupAlignment::End);
    assert_eq!(request.gap(), 5.0);
    assert!(request.allows_flip());
    assert_eq!(request.backend(), PopupBackend::Native);

    let resolved = request
        .resolve_placement(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            PopupBackendCapabilities::overlay_only(),
        )
        .unwrap();
    assert_eq!(resolved.placement(), PopupPlacement::Top);
    assert_eq!(resolved.bounds(), Rect::new(60.0, 40.0, 30.0, 25.0));
    assert!(resolved.backend().fell_back());

    portal.register(request).unwrap();
    assert_eq!(portal.request(child_id).unwrap().parent(), Some(parent_id));
}

#[test]
fn incomplete_request_reports_typed_missing_geometry() {
    let request = PopupRequest::new(PopupId::new(1), owner(1), content());
    assert_eq!(
        request.resolve_placement(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            PopupBackendCapabilities::overlay_only(),
        ),
        Err(PopupPlacementError::MissingAnchor)
    );
    let request = request.with_anchor(Rect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(
        request.resolve_placement(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            PopupBackendCapabilities::overlay_only(),
        ),
        Err(PopupPlacementError::MissingDesiredSize)
    );
}

#[test]
fn placement_request_replaces_semantic_geometry_and_invalidates_backend_bounds() {
    let popup_id = PopupId::new(1);
    let mut portal = PopupPortal::new();
    portal
        .register(
            PopupRequest::new(popup_id, owner(1), content()).with_backend(PopupBackend::Native),
        )
        .unwrap();
    portal
        .set_resolved_bounds(
            popup_id,
            Rect::new(0.0, 0.0, 640.0, 480.0),
            Rect::new(10.0, 10.0, 20.0, 20.0),
        )
        .unwrap();

    let anchor = Rect::new(270.0, 180.0, 0.0, 0.0);
    let desired_size = Size::new(80.0, 60.0);
    portal
        .set_placement_request(
            popup_id,
            PopupPlacementSpec::new(anchor, desired_size)
                .with_placement(PopupPlacement::Bottom)
                .with_alignment(PopupAlignment::Start)
                .with_gap(0.0)
                .with_flip(true),
        )
        .unwrap();

    let request = portal.request(popup_id).unwrap();
    assert_eq!(request.anchor(), Some(anchor));
    assert_eq!(request.desired_size(), Some(desired_size));
    assert_eq!(request.placement(), PopupPlacement::Bottom);
    assert_eq!(request.alignment(), PopupAlignment::Start);
    assert_eq!(request.gap(), 0.0);
    assert!(request.allows_flip());
    assert_eq!(request.backend(), PopupBackend::Native);
    assert_eq!(portal.bounds(popup_id), None);
    assert_eq!(portal.resolved_viewport(popup_id), None);

    let viewport = Rect::new(0.0, 0.0, 300.0, 200.0);
    let resolved = request
        .resolve_placement(viewport, PopupBackendCapabilities::overlay_only())
        .unwrap();
    assert_eq!(resolved.bounds(), Rect::new(220.0, 120.0, 80.0, 60.0));
    assert_eq!(resolved.placement(), PopupPlacement::Top);
    assert!(resolved.flipped());
    assert!(resolved.clamped());
}

#[test]
fn rejected_placement_request_preserves_previous_geometry_and_backend_result() {
    let popup_id = PopupId::new(1);
    let previous_anchor = Rect::new(10.0, 20.0, 30.0, 12.0);
    let previous_size = Size::new(90.0, 45.0);
    let previous_viewport = Rect::new(0.0, 0.0, 320.0, 240.0);
    let previous_bounds = Rect::new(10.0, 35.0, 90.0, 45.0);
    let mut portal = PopupPortal::new();
    portal
        .register(
            PopupRequest::new(popup_id, owner(1), content())
                .with_anchor(previous_anchor)
                .with_desired_size(previous_size)
                .with_placement(PopupPlacement::Top)
                .with_alignment(PopupAlignment::End)
                .with_gap(3.0)
                .with_flip(false),
        )
        .unwrap();
    portal
        .set_resolved_bounds(popup_id, previous_viewport, previous_bounds)
        .unwrap();

    assert_eq!(
        portal.set_placement_request(
            popup_id,
            PopupPlacementSpec::new(Rect::new(4.0, 5.0, 0.0, 0.0), Size::new(f32::NAN, 20.0),)
                .with_placement(PopupPlacement::Bottom)
                .with_alignment(PopupAlignment::Start)
                .with_gap(0.0)
                .with_flip(true),
        ),
        Err(ailloli_ui_runtime::popup::PopupPortalError::InvalidBounds)
    );

    let request = portal.request(popup_id).unwrap();
    assert_eq!(request.anchor(), Some(previous_anchor));
    assert_eq!(request.desired_size(), Some(previous_size));
    assert_eq!(request.placement(), PopupPlacement::Top);
    assert_eq!(request.alignment(), PopupAlignment::End);
    assert_eq!(request.gap(), 3.0);
    assert!(!request.allows_flip());
    assert_eq!(portal.bounds(popup_id), Some(previous_bounds));
    assert_eq!(portal.resolved_viewport(popup_id), Some(previous_viewport));

    assert_eq!(
        portal.set_resolved_bounds(
            popup_id,
            Rect::new(0.0, 0.0, 0.0, 240.0),
            Rect::new(1.0, 2.0, 3.0, 4.0),
        ),
        Err(ailloli_ui_runtime::popup::PopupPortalError::InvalidBounds)
    );
    assert_eq!(portal.bounds(popup_id), Some(previous_bounds));
    assert_eq!(portal.resolved_viewport(popup_id), Some(previous_viewport));

    let explicit_bounds = Rect::new(7.0, 8.0, 9.0, 10.0);
    portal.set_bounds(popup_id, explicit_bounds).unwrap();
    assert_eq!(portal.bounds(popup_id), Some(explicit_bounds));
    assert_eq!(portal.resolved_viewport(popup_id), None);

    portal
        .set_resolved_bounds(popup_id, previous_viewport, previous_bounds)
        .unwrap();
    portal.clear_bounds(popup_id).unwrap();
    assert_eq!(portal.bounds(popup_id), None);
    assert_eq!(portal.resolved_viewport(popup_id), None);
}

#[test]
fn identical_placement_republication_preserves_host_resolved_geometry() {
    let popup_id = PopupId::new(1);
    let placement =
        PopupPlacementSpec::new(Rect::new(75.0, 80.0, 0.0, 0.0), Size::new(252.0, 93.0))
            .with_placement(PopupPlacement::Bottom)
            .with_alignment(PopupAlignment::Start)
            .with_gap(0.0)
            .with_flip(true);
    let viewport = Rect::new(0.0, 0.0, 640.0, 480.0);
    let resolved_bounds = Rect::new(75.0, 80.0, 252.0, 93.0);
    let mut portal = PopupPortal::new();
    portal
        .register(PopupRequest::new(popup_id, owner(1), content()))
        .unwrap();
    portal.set_placement_request(popup_id, placement).unwrap();
    portal
        .set_resolved_bounds(popup_id, viewport, resolved_bounds)
        .unwrap();

    portal.set_placement_request(popup_id, placement).unwrap();

    assert_eq!(portal.bounds(popup_id), Some(resolved_bounds));
    assert_eq!(portal.resolved_viewport(popup_id), Some(viewport));

    portal
        .set_placement_request(popup_id, placement.with_flip(false))
        .unwrap();
    assert_eq!(portal.bounds(popup_id), None);
    assert_eq!(portal.resolved_viewport(popup_id), None);
}
