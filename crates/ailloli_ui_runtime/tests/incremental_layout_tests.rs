use ailloli_ui_core::{Constraints, Scale};
use ailloli_ui_runtime::app::Invalidation;
mod support {
    include!("targeted_invalidation_tests.rs");
}

#[test]
fn clean_layout_is_reused_and_unchanged_bounds_are_not_recommitted() {
    let (mut runtime, mut text, file, chat, _signal) = support::fixture();
    let before = (
        file.layouts.get(),
        file.commits.get(),
        chat.layouts.get(),
        chat.commits.get(),
    );

    for _ in 0..20 {
        runtime.layout(Constraints::tight(500.0, 100.0), Scale::new(1.0), &mut text);
    }
    assert_eq!(
        (
            file.layouts.get(),
            file.commits.get(),
            chat.layouts.get(),
            chat.commits.get(),
        ),
        before,
    );

    let chat_id = runtime.tree.resolve_element_by_view_key("chat").unwrap();
    runtime.runtime.invalidate(chat_id, Invalidation::Layout);
    runtime.layout(Constraints::tight(500.0, 100.0), Scale::new(1.0), &mut text);
    assert_eq!(file.layouts.get(), before.0);
    assert_eq!(file.commits.get(), before.1);
    assert_eq!(chat.layouts.get(), before.2 + 1);
    assert_eq!(
        chat.commits.get(),
        before.3,
        "an unchanged layout result and absolute bounds are not recommitted",
    );
}

#[test]
fn text_metrics_revision_invalidates_the_layout_cache_key() {
    let (mut runtime, mut text, file, chat, _signal) = support::fixture();
    let before = (file.layouts.get(), chat.layouts.get());
    text.invalidate_metrics();
    runtime.layout(Constraints::tight(500.0, 100.0), Scale::new(1.0), &mut text);
    assert_eq!(file.layouts.get(), before.0 + 1);
    assert_eq!(chat.layouts.get(), before.1 + 1);
}
