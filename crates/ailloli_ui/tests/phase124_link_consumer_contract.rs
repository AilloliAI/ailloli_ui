//! Public facade link-construction contract retained from Phase 124.
//!
//! The scenarios ensure both canonical link forms compile while the link
//! builder does not absorb application-level action or router shortcuts.

use ailloli_ui::prelude::*;

#[test]
fn facade_compiles_both_canonical_link_forms() {
    let _: View<()> = Link::with_label("Documentation")
        .href("https://docs.ailloli.ai")
        .into_view();

    let _: View<()> = Link::new()
        .child(
            Row::new()
                .gap(6.0)
                .child(Icon::new(IconId::Plus))
                .child(Text::new("GitHub")),
        )
        .href("https://github.com/ailloli")
        .into_view();

    let _: ailloli_ui::Link<()> = ailloli_ui::Link::new();
    let _style = ailloli_ui::LinkStyle::default();
    let _decoration = TextDecoration::Underline;
}

#[test]
fn link_public_builder_does_not_grow_action_or_router_shortcuts() {
    let source = include_str!("../../ailloli_ui_widgets/src/controls/link.rs");
    assert!(!source.contains("pub fn route"));
    assert!(!source.contains("pub fn on_activate"));
    assert!(!source.contains("pub fn external"));
}
