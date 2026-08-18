use ailloli_ui::{Command, Commands, View, Window, WindowChrome, Windows};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Noop,
}

#[test]
fn window_builder_stores_declarative_options() {
    let window = Window::<Action>::new("main")
        .title("Sample App")
        .size(1200.0, 800.0)
        .content(View::empty);

    assert_eq!(window.id(), "main");
    assert_eq!(window.title_str(), "Sample App");
    assert_eq!(window.logical_size(), Some((1200.0, 800.0)));
    assert!(window.has_content());
    assert_eq!(window.chrome(), WindowChrome::Os);
    assert!(window.is_resizable());
    assert!(window.is_titlebar_draggable());
    assert_eq!(window.corner_radius(), 0.0);
}

#[test]
fn window_resizable_can_be_disabled() {
    let window = Window::<Action>::new("main")
        .resizable(false)
        .content(View::empty);
    assert!(!window.is_resizable());
}

#[test]
fn window_radius_default_and_clamp() {
    let w = Window::<Action>::new("main").content(View::empty);
    assert_eq!(w.corner_radius(), 0.0);

    let w = Window::<Action>::new("main")
        .radius(12.0)
        .content(View::empty);
    assert!((w.corner_radius() - 12.0).abs() < 1e-3);

    let w = Window::<Action>::new("main")
        .radius(-5.0)
        .content(View::empty);
    assert_eq!(w.corner_radius(), 0.0);

    let w = Window::<Action>::new("main")
        .radius(500.0)
        .content(View::empty);
    assert_eq!(w.corner_radius(), 128.0);
}

#[test]
fn window_titlebar_draggable_can_be_disabled() {
    let window = Window::<Action>::new("main")
        .titlebar_draggable(false)
        .content(View::empty);
    assert!(!window.is_titlebar_draggable());
}

#[test]
fn window_ailloli_ui_chrome_mode() {
    let window = Window::<Action>::new("main")
        .ailloli_ui_chrome()
        .content(View::empty);

    assert_eq!(window.chrome(), WindowChrome::AilloliUi);
    assert!(!window.has_title_bar());
}

#[test]
fn window_custom_chrome_sets_title_bar() {
    let window = Window::<Action>::new("main")
        .custom_chrome()
        .title_bar(View::empty)
        .content(View::empty);

    assert_eq!(window.chrome(), WindowChrome::Custom);
    assert!(window.has_title_bar());
}

#[test]
fn windows_collects_multiple_window_specs() {
    let windows = Windows::new()
        .push(Window::<Action>::new("main").content(View::empty))
        .push(Window::<Action>::new("settings").content(View::empty));

    let ids: Vec<_> = windows.iter().map(Window::id).collect();
    assert_eq!(windows.len(), 2);
    assert_eq!(ids, vec!["main", "settings"]);
}

#[test]
fn commands_support_none_quit_redraw_and_dispatch() {
    assert!(Commands::<Action>::none().is_empty());

    let quit = Commands::<Action>::quit();
    assert_eq!(quit.len(), 1);
    assert!(matches!(quit.iter().next(), Some(Command::Quit)));

    let redraw = Commands::<Action>::redraw();
    assert_eq!(redraw.len(), 1);
    assert!(matches!(redraw.iter().next(), Some(Command::Redraw)));

    let dispatch = Commands::dispatch(Action::Noop);
    assert_eq!(dispatch.len(), 1);
    assert!(matches!(
        dispatch.iter().next(),
        Some(Command::Dispatch(Action::Noop))
    ));

    let delayed = Commands::dispatch_after(Action::Noop, Duration::from_millis(500));
    assert_eq!(delayed.len(), 1);
    assert!(matches!(
        delayed.iter().next(),
        Some(Command::DispatchAfter {
            action: Action::Noop,
            delay,
        }) if *delay == Duration::from_millis(500)
    ));
}
