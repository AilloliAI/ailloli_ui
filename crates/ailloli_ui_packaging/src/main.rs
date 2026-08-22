//! `cargo-ailloli-ui` command-line entry point.

/// Runs the packaging library and converts a returned error into exit status `1`.
fn main() {
    if let Err(error) = ailloli_ui_packaging::run_from_env() {
        eprintln!("cargo-ailloli-ui: {error}");
        std::process::exit(1);
    }
}
