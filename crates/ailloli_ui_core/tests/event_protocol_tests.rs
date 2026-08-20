use ailloli_ui_core::event::{
    ActivationKind, FileEvent, ImeEvent, ImePreedit, ImePreeditError, Modifiers, MouseButton,
    PointerButton, PointerEvent, PointerId, PointerSample, PointerSampleError, PointerSource,
};
use ailloli_ui_core::{LogicalWindowId, Point, Size, UploadFile};

#[test]
fn logical_window_id_is_stable_hashable_and_provider_neutral() {
    let id = LogicalWindowId::new("main");
    let clone = id.clone();
    assert_eq!(id, clone);
    assert_eq!(id.as_str(), "main");
    assert_eq!(id.to_string(), "main");
}

#[test]
fn pointer_sample_validates_and_preserves_high_fidelity_data() {
    let sample = PointerSample::new(
        PointerId::new(42),
        PointerSource::Pen,
        Point::new(12.0, 24.0),
    )
    .unwrap()
    .with_pressure(0.75)
    .unwrap()
    .with_tilt(-20.0, 35.0)
    .unwrap()
    .with_twist(270.0)
    .unwrap()
    .with_contact_size(Size::new(4.0, 6.0))
    .unwrap()
    .with_primary(false)
    .with_activation(ActivationKind::FocusOnly);

    assert_eq!(sample.id(), PointerId::new(42));
    assert_eq!(sample.source(), PointerSource::Pen);
    assert!(!sample.is_primary());
    assert_eq!(sample.pressure(), Some(0.75));
    assert_eq!(sample.tilt(), Some((-20.0, 35.0)));
    assert_eq!(sample.twist(), Some(270.0));
    assert_eq!(sample.contact_size(), Some(Size::new(4.0, 6.0)));
    assert_eq!(sample.activation(), ActivationKind::FocusOnly);
}

#[test]
fn pointer_sample_keeps_legacy_primary_default_and_supports_explicit_secondary() {
    let legacy = PointerSample::new(
        PointerId::new(1),
        PointerSource::Mouse,
        Point::new(1.0, 2.0),
    )
    .unwrap();
    let secondary = PointerSample::new_with_primary(
        PointerId::new(2),
        PointerSource::Touch,
        Point::new(3.0, 4.0),
        false,
    )
    .unwrap();

    assert!(legacy.is_primary());
    assert!(!secondary.is_primary());
    assert!(secondary.with_primary(true).is_primary());
}

#[test]
fn pointer_event_builders_use_pointer_button_as_the_canonical_name() {
    let canonical = PointerButton::Left;
    let legacy: MouseButton = canonical;
    let event = PointerEvent::button(Point::new(1.0, 2.0), legacy, true, Modifiers::default());

    assert_eq!(event.position(), Point::new(1.0, 2.0));
    assert_eq!(event.modifiers(), Modifiers::default());
    assert_eq!(event.button_transition(), Some((canonical, true)));
    assert_eq!(event.wheel_delta(), None);
    assert!(!event.is_cancelled());
}

#[test]
fn pointer_sample_rejects_non_finite_and_out_of_range_values() {
    assert_eq!(
        PointerSample::new(
            PointerId::MOUSE,
            PointerSource::Mouse,
            Point::new(f32::NAN, 0.0),
        ),
        Err(PointerSampleError::NonFinitePosition)
    );
    let sample = PointerSample::new(
        PointerId::new(1),
        PointerSource::Touch,
        Point::new(0.0, 0.0),
    )
    .unwrap();
    assert_eq!(
        sample.with_pressure(1.01),
        Err(PointerSampleError::InvalidPressure)
    );
    assert_eq!(
        sample.with_tilt(-91.0, 0.0),
        Err(PointerSampleError::InvalidTilt)
    );
    assert_eq!(
        sample.with_twist(360.0),
        Err(PointerSampleError::InvalidTwist)
    );
}

#[test]
fn ime_preedit_validates_utf8_byte_ranges_without_echoing_text_in_errors() {
    let preedit = ImePreedit::try_new("éa", Some((0, 2))).unwrap();
    assert_eq!(preedit.text(), "éa");
    assert_eq!(preedit.selection(), Some((0, 2)));
    assert_eq!(
        ImePreedit::try_new("éa", Some((2, 1))),
        Err(ImePreeditError::ReversedSelection)
    );
    assert_eq!(
        ImePreedit::try_new("éa", Some((0, 4))),
        Err(ImePreeditError::SelectionOutOfBounds)
    );
    let error = ImePreedit::try_new("secret-é", Some((1, 8))).unwrap_err();
    assert_eq!(error, ImePreeditError::SelectionNotOnCharBoundary);
    assert!(!error.to_string().contains("secret"));

    let event = ImeEvent::try_preedit("🙂a", Some((0, 4)), None).unwrap();
    let (preedit, pos) = event.as_preedit().unwrap();
    assert_eq!(preedit.text(), "🙂a");
    assert_eq!(preedit.selection(), Some((0, 4)));
    assert_eq!(pos, None);
    assert_eq!(ImeEvent::commit("done").committed_text(), Some("done"));
}

#[test]
fn file_batches_keep_unknown_positions_unknown() {
    let files = vec![UploadFile::named("one.txt"), UploadFile::named("two.txt")];
    let entered = FileEvent::entered(None, files.clone());
    let dropped = FileEvent::dropped(Some(Point::new(5.0, 7.0)), files);
    assert_eq!(entered.pos(), None);
    assert_eq!(entered.files().len(), 2);
    assert_eq!(dropped.pos(), Some(Point::new(5.0, 7.0)));
    assert_eq!(FileEvent::Left.files(), []);
    assert!(FileEvent::left().is_left());
}
