use ailloli_ui_core::style::Background;
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::{DrawCmd, DrawRRect};
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::button::{ButtonStyle, ButtonVariant};
use ailloli_ui_widgets::controls::{draw_checkbox, Button, CheckboxStyle, TextInputStyle};
use ailloli_ui_widgets::layout::Container;

#[test]
fn button_variants_use_default_theme_tokens() {
    let palette = Theme::default().palette();
    let primary = ButtonStyle::from_theme(Theme::default(), ButtonVariant::Primary);
    let secondary = ButtonStyle::from_theme(Theme::default(), ButtonVariant::Secondary);
    let destructive = ButtonStyle::from_theme(Theme::default(), ButtonVariant::Destructive);

    assert_eq!(
        primary.container.normal.background,
        Background::color(palette.accent)
    );
    assert_eq!(primary.text.normal.color, palette.text);
    assert!(secondary.container.normal.border.is_visible());
    assert_eq!(
        destructive.container.normal.background,
        Background::color(palette.danger)
    );
}

#[test]
fn button_with_label_variant_sets_matching_text_style() {
    let button = Button::<()>::with_label_variant("Delete", ButtonVariant::Destructive);
    let view = button.into_view();
    assert_eq!(view.children.len(), 1);
}

#[test]
fn text_input_style_default_comes_from_theme() {
    let palette = Theme::default().palette();
    let style = TextInputStyle::default();

    assert_eq!(style.bg, palette.surface);
    assert_eq!(style.border, palette.border);
    assert_eq!(style.border_focused, palette.focus);
    assert_eq!(style.placeholder, palette.text_muted);
    assert_eq!(style.text.color, palette.text);
}

#[test]
fn checkbox_style_default_uses_accent_for_checked_state() {
    let palette = Theme::default().palette();
    let style = CheckboxStyle::default();
    let mut text = TextSystem::new();
    let cmds = draw_checkbox(
        ailloli_ui_core::Rect::new(0.0, 0.0, 140.0, 24.0),
        true,
        Some("Checked"),
        false,
        style,
        &mut text,
    );

    assert_eq!(style.checked_bg, palette.accent);
    assert!(cmds.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::RRect(DrawRRect { color, .. }) if *color == palette.accent
    )));
    assert!(cmds.iter().any(|cmd| matches!(cmd, DrawCmd::Border(_))));
}

#[test]
fn container_surface_and_panel_are_theme_shortcuts() {
    let palette = Theme::default().palette();
    let surface = Container::<()>::surface(Theme::default());
    let panel = Container::<()>::panel(Theme::default());

    let surface_view = surface.into_view();
    let panel_view = panel.into_view();

    assert!(matches!(
        surface_view.kind,
        ailloli_ui_runtime::component::ViewKind::Widget(_)
    ));
    assert!(matches!(
        panel_view.kind,
        ailloli_ui_runtime::component::ViewKind::Widget(_)
    ));
    assert_ne!(palette.surface, Color::TRANSPARENT);
}
