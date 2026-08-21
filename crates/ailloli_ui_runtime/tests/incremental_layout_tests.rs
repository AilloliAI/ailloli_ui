use ailloli_ui_core::{Constraints, Scale};
use ailloli_ui_runtime::app::Invalidation;
mod support {
    include!("targeted_invalidation_tests.rs");
}

#[test]
fn clean_layout_is_reused_and_unchanged_bounds_are_not_recommitted() {
    let mut fixture = support::fixture();
    let before = (
        fixture.file.layouts.get(),
        fixture.file.commits.get(),
        fixture.chat.layouts.get(),
        fixture.chat.commits.get(),
    );

    for _ in 0..20 {
        fixture.runtime.layout(
            Constraints::tight(500.0, 100.0),
            Scale::new(1.0),
            &mut fixture.text,
        );
    }
    assert_eq!(
        (
            fixture.file.layouts.get(),
            fixture.file.commits.get(),
            fixture.chat.layouts.get(),
            fixture.chat.commits.get(),
        ),
        before,
    );

    let chat_id = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    fixture
        .runtime
        .runtime
        .invalidate(chat_id, Invalidation::Layout);
    fixture.runtime.layout(
        Constraints::tight(500.0, 100.0),
        Scale::new(1.0),
        &mut fixture.text,
    );
    assert_eq!(fixture.file.layouts.get(), before.0);
    assert_eq!(fixture.file.commits.get(), before.1);
    assert_eq!(fixture.chat.layouts.get(), before.2 + 1);
    assert_eq!(
        fixture.chat.commits.get(),
        before.3,
        "an unchanged layout result and absolute bounds are not recommitted",
    );
}

#[test]
fn text_metrics_revision_invalidates_the_layout_cache_key() {
    let mut fixture = support::fixture();
    let before = (
        fixture.file.layouts.get(),
        fixture.chat.layouts.get(),
        fixture.terminal.layouts.get(),
    );
    fixture.text.invalidate_metrics();
    fixture.runtime.layout(
        Constraints::tight(500.0, 100.0),
        Scale::new(1.0),
        &mut fixture.text,
    );
    assert_eq!(fixture.file.layouts.get(), before.0 + 1);
    assert_eq!(fixture.chat.layouts.get(), before.1 + 1);
    assert_eq!(fixture.terminal.layouts.get(), before.2 + 1);
}
