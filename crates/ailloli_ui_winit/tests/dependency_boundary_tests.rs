//! Static dependency-boundary scenarios for public framework and sandbox sources.

use std::fs;
use std::path::{Path, PathBuf};

/// Resolves the framework workspace root from the winit crate manifest.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Recursively collects Rust sources while skipping build-output directories.
fn visit_rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

/// Detects direct `winit` imports, paths, and extern declarations in source text.
fn has_direct_winit_dependency(source: &str) -> bool {
    let mut dependency_section = false;
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            dependency_section = line
                .trim_matches(['[', ']'])
                .split('.')
                .any(|segment| segment == "dependencies" || segment == "dev-dependencies");
            continue;
        }
        if dependency_section
            && line
                .split_once('=')
                .is_some_and(|(key, _)| key.trim().split('.').next() == Some("winit"))
        {
            return true;
        }
    }
    false
}

/// Detects a token-delimited `winit::` path without matching longer identifiers.
fn has_winit_path(source: &str) -> bool {
    source.match_indices("winit::").any(|(index, _)| {
        source[..index]
            .chars()
            .next_back()
            .is_none_or(|character| !(character.is_alphanumeric() || character == '_'))
    })
}

#[test]
fn winit_code_and_direct_dependency_are_confined_to_the_adapter() {
    let root = workspace_root();
    let crates_dir = root.join("crates");
    let mut violations = Vec::new();

    for entry in fs::read_dir(&crates_dir).expect("workspace crates directory") {
        let path = entry.expect("crate entry").path();
        if !path.is_dir()
            || path
                .file_name()
                .is_some_and(|name| name == "ailloli_ui_winit")
        {
            continue;
        }

        let manifest = path.join("Cargo.toml");
        if manifest.exists() {
            let source = fs::read_to_string(&manifest).expect("crate manifest");
            if has_direct_winit_dependency(&source) {
                violations.push(format!("direct dependency in {}", manifest.display()));
            }
        }

        let mut rust_files = Vec::new();
        visit_rust_files(&path.join("src"), &mut rust_files);
        for file in rust_files {
            let source = fs::read_to_string(&file).expect("Rust source");
            if has_winit_path(&source) {
                violations.push(format!("winit code in {}", file.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "winit boundary violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn boundary_matchers_distinguish_features_facade_paths_and_direct_dependencies() {
    assert!(!has_direct_winit_dependency(
        "[features]\nwinit = [\"dep:ailloli_ui_winit\"]\n"
    ));
    assert!(has_direct_winit_dependency(
        "[target.'cfg(unix)'.dependencies]\nwinit.workspace = true\n"
    ));
    assert!(!has_winit_path("ailloli_ui_winit::UiApp"));
    assert!(has_winit_path("winit::window::Window"));
}

#[test]
fn winit_host_is_the_only_high_level_application_handler() {
    let root = workspace_root();
    let ui_app = fs::read_to_string(root.join("crates/ailloli_ui_winit/src/ui_app.rs"))
        .expect("UiApp source");
    let host = fs::read_to_string(root.join("crates/ailloli_ui_winit/src/host.rs"))
        .expect("WinitHost source");

    assert!(
        !ui_app.contains("ApplicationHandler for UiApp"),
        "UiApp must remain retained state, not a second native handler"
    );
    assert!(
        host.contains("ApplicationHandler for WinitHost"),
        "WinitHost must own the high-level native handler contract"
    );
}
