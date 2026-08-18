use ailloli_ui_runtime::component::State;

#[test]
fn state_reads_sets_and_updates_value() {
    let state = State::new(1);

    assert_eq!(state.read(), 1);

    state.set(2);
    assert_eq!(state.read(), 2);

    state.update(|value| *value += 3);
    assert_eq!(state.read(), 5);
}

#[test]
fn state_maps_to_derived_value() {
    let initial = "ailloli_ui";
    let state = State::new(initial.to_string());
    let len = state.map(|value| value.len());

    assert_eq!(len.read(), initial.len());

    state.set("ui".to_string());
    assert_eq!(len.read(), 2);
}
