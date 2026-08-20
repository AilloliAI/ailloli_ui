const CARGO_TOML: &str = include_str!("../Cargo.toml");

#[test]
fn public_facade_path_dependencies_stay_in_the_framework_namespace() {
    let dependencies = CARGO_TOML
        .split_once("[dependencies]")
        .expect("dependencies section")
        .1;

    for line in dependencies
        .lines()
        .take_while(|line| !line.starts_with('['))
    {
        if !line.contains("path =") {
            continue;
        }
        let (name, declaration) = line.split_once('=').expect("dependency assignment");
        assert!(
            name.trim().starts_with("ailloli_ui"),
            "path dependency must be framework-owned: {line}"
        );
        assert!(
            declaration.contains("../ailloli_ui"),
            "path dependency must stay inside framework/crates: {line}"
        );
    }
}

#[test]
fn public_facade_exposes_only_generic_framework_features() {
    let expected = [
        "default",
        "devtools",
        "devtools_terminal",
        "files",
        "files_local",
        "native_overlay",
        "terminal_pty",
        "terminal_pty_portable",
        "tree_sitter",
        "winit",
    ];
    let mut actual = CARGO_TOML
        .split_once("[features]")
        .expect("features section")
        .1
        .lines()
        .take_while(|line| !line.starts_with('['))
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.trim()))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    actual.sort_unstable();

    assert_eq!(actual, expected);
}

#[test]
fn framework_prelude_compiles_standalone() {
    use ailloli_ui::prelude::*;

    let _buffer = TextBuffer::new();
    let _color = Color::hex("#101010").expect("valid color");
}

#[test]
fn public_facade_keeps_framework_owned_opt_in_features() {
    assert!(CARGO_TOML.contains("tree_sitter = [\"ailloli_ui_widgets/tree_sitter\"]"));
    assert!(CARGO_TOML.lines().any(|line| line.trim() == "devtools = ["));
    assert!(CARGO_TOML
        .lines()
        .any(|line| line.trim() == "devtools_terminal = ["));
}
