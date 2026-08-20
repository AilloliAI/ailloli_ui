use ailloli_ui_core::Size;
use ailloli_ui_runtime::app::{
    reduce_presentation, PendingPresentationIntents, PresentationCursor, PresentationEvent,
    PresentationGeneration, PresentationIntent, PresentationLifecycle, PresentationState,
    PresentationTransitionError, PresentationUnavailableReason, WindowChromeOp,
};

#[test]
fn attach_increments_generation_and_stale_callbacks_are_rejected() {
    let mut lifecycle = PresentationLifecycle::new("main");
    lifecycle.apply(PresentationEvent::AllowCreation).unwrap();
    lifecycle.apply(PresentationEvent::Attached).unwrap();
    let first = lifecycle.generation();
    assert_eq!(first, PresentationGeneration::new(1));
    assert!(lifecycle.accepts(first));

    lifecycle.apply(PresentationEvent::Suspend).unwrap();
    assert!(!lifecycle.accepts(first));
    lifecycle.apply(PresentationEvent::AllowCreation).unwrap();
    lifecycle.apply(PresentationEvent::Attached).unwrap();
    assert_eq!(lifecycle.generation(), PresentationGeneration::new(2));
    assert!(!lifecycle.accepts(first));
    assert!(lifecycle.accepts(PresentationGeneration::new(2)));
}

#[test]
fn duplicate_resume_suspend_and_destroy_are_deterministic() {
    let declared = PresentationState::Declared;
    let initial = PresentationGeneration::INITIAL;
    let allowed = reduce_presentation(declared, initial, PresentationEvent::AllowCreation).unwrap();
    let duplicate = reduce_presentation(
        allowed.state,
        allowed.generation,
        PresentationEvent::AllowCreation,
    )
    .unwrap();
    assert_eq!(duplicate, allowed);

    let suspended = reduce_presentation(
        duplicate.state,
        duplicate.generation,
        PresentationEvent::Suspend,
    )
    .unwrap();
    let duplicate_suspend = reduce_presentation(
        suspended.state,
        suspended.generation,
        PresentationEvent::Suspend,
    )
    .unwrap();
    assert_eq!(duplicate_suspend, suspended);

    let destroyed = reduce_presentation(
        suspended.state,
        suspended.generation,
        PresentationEvent::Destroy,
    )
    .unwrap();
    assert_eq!(
        reduce_presentation(
            destroyed.state,
            destroyed.generation,
            PresentationEvent::Destroy
        )
        .unwrap(),
        destroyed
    );
    assert_eq!(
        reduce_presentation(
            destroyed.state,
            destroyed.generation,
            PresentationEvent::AllowCreation
        ),
        Err(PresentationTransitionError::Destroyed)
    );
}

#[test]
fn unavailable_requires_retry_before_reattach() {
    let mut lifecycle = PresentationLifecycle::new("main");
    lifecycle.apply(PresentationEvent::AllowCreation).unwrap();
    lifecycle.apply(PresentationEvent::Attached).unwrap();
    lifecycle
        .apply(PresentationEvent::Unavailable(
            PresentationUnavailableReason::SurfaceLost,
        ))
        .unwrap();
    assert_eq!(
        lifecycle.apply(PresentationEvent::Attached),
        Err(PresentationTransitionError::AttachmentNotAllowed)
    );
    lifecycle.apply(PresentationEvent::Retry).unwrap();
    lifecycle.apply(PresentationEvent::Attached).unwrap();
    assert_eq!(lifecycle.generation(), PresentationGeneration::new(2));
}

#[test]
fn retained_intents_coalesce_state_and_preserve_chrome_order() {
    let mut pending = PendingPresentationIntents::default();
    pending.push(PresentationIntent::SetTitle("old".into()));
    pending.push(PresentationIntent::SetTitle("new".into()));
    pending.push(PresentationIntent::SetInnerSize(Size::new(640.0, 480.0)));
    pending.push(PresentationIntent::SetCursor(PresentationCursor::Pointer));
    pending.push(PresentationIntent::WindowChrome(WindowChromeOp::Minimize));
    pending.push(PresentationIntent::WindowChrome(
        WindowChromeOp::ToggleMaximize,
    ));
    pending.push(PresentationIntent::Redraw);
    pending.push(PresentationIntent::Redraw);

    assert_eq!(
        pending.drain(),
        vec![
            PresentationIntent::SetTitle("new".into()),
            PresentationIntent::SetInnerSize(Size::new(640.0, 480.0)),
            PresentationIntent::SetCursor(PresentationCursor::Pointer),
            PresentationIntent::WindowChrome(WindowChromeOp::Minimize),
            PresentationIntent::WindowChrome(WindowChromeOp::ToggleMaximize),
            PresentationIntent::Redraw,
        ]
    );
    assert!(pending.is_empty());
}
