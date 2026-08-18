use ailloli_ui_runtime::app::RuntimeHandle;
use ailloli_ui_runtime::component::{Binding, Context, State};

#[test]
fn binding_static_from_str_reads() {
    let b: Binding<String> = "hello".into();
    assert_eq!(b.read(), "hello".to_string());
}

#[test]
fn binding_signal_reads_latest_value() {
    let runtime = RuntimeHandle::<()>::new();
    let element_id = ailloli_ui_core::ids::ElementId(1);
    let mut ctx = Context::new(element_id, runtime.clone());

    let s = ctx.signal(String::new());
    let b: Binding<String> = s.clone().into();
    assert_eq!(b.read(), "".to_string());

    s.set("abc".to_string());
    assert_eq!(b.read(), "abc".to_string());
}

#[test]
fn binding_state_reads_latest_value() {
    let state = State::new(10u32);
    let binding: Binding<u32> = state.clone().into();
    assert_eq!(binding.read(), 10);

    state.set(42);
    assert_eq!(binding.read(), 42);
}

#[test]
fn binding_memo_reads_computed_value() {
    let runtime = RuntimeHandle::<()>::new();
    let element_id = ailloli_ui_core::ids::ElementId(1);
    let mut ctx = Context::new(element_id, runtime);

    let s = ctx.signal(1u32);
    let m = s.map(|v| v + 1);
    let b: Binding<u32> = m.into();
    assert_eq!(b.read(), 2);
}
