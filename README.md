# Ailloli UI

Ailloli UI is a retained-mode desktop UI framework for Rust. It combines a
backend-neutral retained tree with native `winit` windows, a primary `wgpu`
renderer, and optional Vulkan and OpenXR hosts.

The project is pre-1.0 and under active development. Every package in this
workspace is currently `publish = false`; consumers use a path or pinned Git
dependency until a separate publication phase is authorized.

## Quick start

Add the façade crate from a local checkout:

```toml
[dependencies]
ailloli_ui = { path = "../ailloli-ui/crates/ailloli_ui" }
```

Then build an application through the public prelude:

```rust
use ailloli_ui::prelude::*;

fn main() -> ailloli_ui::Result<()> {
    App::new()
        .window(
            Window::new("main")
                .title("Hello")
                .size(800.0, 600.0)
                .ailloli_ui_chrome()
                .content(|| {
                    Column::new()
                        .padding(16.0)
                        .gap(8.0)
                        .child(Text::new("Hello from Ailloli UI"))
                        .child(Button::with_label("Continue"))
                }),
        )
        .run()
}
```

`ailloli_ui::prelude::*` is the intended application entry point. Lower-level
crates remain available for custom runtimes, render hosts, and reusable
framework extensions.

## Architecture

The dependency direction is intentionally one-way:

```text
consumer application
        |
        v
    ailloli_ui
        |
        +--> core --> runtime --> widgets --> winit
        |                 |                     |
        +--> text --------+                     +--> render_wgpu
        +--> editor ------+
        +--> fs / fs_local
        +--> terminal_core / terminal_pty
        +--> app_storage / icon / devtools

OpenXR --> core/runtime/widgets + render_vulkan or render_wgpu target
```

The public framework never depends on an application or an application-owned
crate. Provider choice, business workflows, credentials, and product policy
belong in the consuming application.

## Workspace packages

The workspace contains exactly 21 public-framework packages:

| Package | Responsibility |
| --- | --- |
| `ailloli_ui` | Public façade, prelude, typed application and window API. |
| `ailloli_ui_core` | Geometry, events, identity, styles, themes, and value types. |
| `ailloli_ui_runtime` | Retained tree, reconciliation, layout, paint, and input routing. |
| `ailloli_ui_app_storage` | Application-neutral XDG paths and persisted documents. |
| `ailloli_ui_icon` | Bounded SVG validation and deterministic rasterization. |
| `ailloli_ui_text` | Font discovery, shaping, layout, and editable text buffers. |
| `ailloli_ui_editor` | Backend-neutral editor engine and code-oriented data models. |
| `ailloli_ui_fs` | UI-free file-provider contracts. |
| `ailloli_ui_fs_local` | Local implementation of the file-provider contracts. |
| `ailloli_ui_terminal_core` | Process-independent terminal parser, grid, and snapshots. |
| `ailloli_ui_terminal_pty` | Optional PTY process contracts and portable backend. |
| `ailloli_ui_widgets` | Generic layouts, controls, editors, files, and terminal views. |
| `ailloli_ui_winit` | Native windows, event loop, clipboard, capture, and renderer host. |
| `ailloli_ui_render_wgpu` | Primary GPU renderer. |
| `ailloli_ui_render_vulkan` | Experimental Vulkan/SPIR-V renderer. |
| `ailloli_ui_openxr` | Optional OpenXR and immersive host integration. |
| `ailloli_ui_devtools_core` | Backend-neutral inspection snapshots and state. |
| `ailloli_ui_devtools_ui` | Generic inspection overlay and panels. |
| `ailloli_ui_devicons_font` | Audited, reduced file-icon font asset. |
| `ailloli_ui_packaging` | Packaging library and `cargo-ailloli-ui` subcommand. |
| `ailloli_ui_bench` | Opt-in structured performance measurements. |

## Façade features

The façade exposes only generic framework capabilities:

| Feature | Purpose |
| --- | --- |
| `winit` | Native windows and the primary WGPU renderer; enabled by default. |
| `native-overlay` | Platform-capability-gated native overlay support. |
| `files` | Provider-neutral file widgets. |
| `files-local` | File widgets with the local provider. |
| `tree-sitter` | Syntax parsing support in editor widgets. |
| `terminal-pty` | PTY contracts through the façade. |
| `terminal-pty-portable` | Portable PTY implementation. |
| `devtools` | Runtime inspection core and UI; disabled by default. |
| `devtools-terminal` | Terminal snapshots in DevTools. |

## Development

Run commands from this directory:

```sh
cargo metadata --locked --format-version 1
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo check --locked --workspace --all-targets --all-features
cargo test --locked --workspace --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --all-features --no-deps
```

Visual capture tests are opt-in and ignored by default. Their artifacts are
written under `artifacts/captures/` inside this workspace.

## Assets and license

Framework-authored code is dual-licensed under Apache-2.0 or MIT, at your
option. See `LICENSE-APACHE`, `LICENSE-MIT`, and `NOTICE`.

Bundled fonts retain their own licenses and provenance beside the assets:

- `crates/ailloli_ui_text/assets/fonts/` contains JetBrains Mono and its OFL.
- `crates/ailloli_ui_devicons_font/` contains the reduced file-icon font,
  reproducible generation tooling, source hashes, and third-party notices.

Copyright 2026 Rising Corporation and Ailloli UI contributors.
