fn main() {
    if let Err(error) = ailloli_ui_packaging::run_from_env() {
        eprintln!("cargo-ailloli-ui: {error}");
        std::process::exit(1);
    }
}
