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
ailloli_ui = { path = "crates/ailloli_ui" }
```

Then build an application through the public prelude:

```rust
use ailloli_ui::prelude::*;

fn main() -> ailloli_ui::Result<()> {
    let headline = State::new(
        "Build native Rust interfaces".to_string(),
    );
    let preview = headline.clone();

    App::new()
        .window(
            Window::new("main")
                .title("Hello")
                .size(800.0, 600.0)
                .ailloli_ui_chrome()
                .content(move || {
                    Column::new()
                        .padding(16.0)
                        .gap(12.0)
                        .child(TextInput::<()>::new().bind(headline.clone()))
                        .child(Text::new(preview.clone()).size(24.0))
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

## Sandbox application

The repository includes a real consumer package at `apps/sandbox_app`. Run it
from the workspace root with:

```sh
cargo run -p sandbox_app
```

Or run `cargo run` directly from `apps/sandbox_app`. The sandbox is a curated,
interactive documentation showcase built only through the public `ailloli_ui`
façade, so it exercises the same API and dependency direction as an external
application. Its editable quick start, retained-state preview, architecture
explorer, guide, and resource cards present real framework contracts instead
of synthetic product data. GitHub and crates.io links use their reserved public
destinations; hosted API documentation and the Ailloli UI Book stay visibly
marked as coming soon until canonical sites exist.

It is a workspace application rather than a crate-local Cargo example because
its role is to validate the complete consumer experience across framework
packages.

## Workspace packages

The workspace contains 22 public-framework packages plus the non-publishable
`sandbox_app` consumer application:

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
| `ailloli_ui_fs_runtime` | Bounded worker queues and wake delivery for filesystem sources. |
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
| `sandbox_app` | Public consumer sandbox used to exercise the façade from the repository. |

## Façade features

The façade exposes only generic framework capabilities:

| Feature | Purpose |
| --- | --- |
| `winit` | Native windows and the primary WGPU renderer; enabled by default. |
| `native_overlay` | Platform-capability-gated native overlay support. |
| `files` | Provider-neutral file widgets. |
| `files_local` | File widgets with the local provider. |
| `tree_sitter` | Syntax parsing support in editor widgets. |
| `terminal_pty` | PTY contracts through the façade. |
| `terminal_pty_portable` | Portable PTY implementation. |
| `devtools` | Runtime inspection core and UI; disabled by default. |
| `devtools_terminal` | Terminal snapshots in DevTools. |

### Cargo feature migration

Phase 126 removes the former kebab-case first-party feature aliases. Update
consumer manifests and `--features` arguments by replacing the complete legacy
name with its snake_case counterpart:

| Legacy | Current | Legacy | Current |
|---|---|---|---|
| `desktop-calibration` | `desktop_calibration` | `devtools-terminal` | `devtools_terminal` |
| `files-local` | `files_local` | `full-local` | `full_local` |
| `linux-portal-input` | `linux_portal_input` | `mock-transport` | `mock_transport` |
| `native-overlay` | `native_overlay` | `openssh-sftp` | `openssh_sftp` |
| `remote-openssh-sftp` | `remote_openssh_sftp` | `remote-sftp` | `remote_sftp` |
| `remote-sftp-vendored-openssl` | `remote_sftp_vendored_openssl` | `smoke-ui` | `smoke_ui` |
| `ssh-exec` | `ssh_exec` | `terminal-portable` | `terminal_portable` |
| `terminal-pty` | `terminal_pty` | `terminal-pty-portable` | `terminal_pty_portable` |
| `test-support` | `test_support` | `tree-sitter` | `tree_sitter` |
| `tree-sitter-bash` | `tree_sitter_bash` | `tree-sitter-c` | `tree_sitter_c` |
| `tree-sitter-css` | `tree_sitter_css` | `tree-sitter-go` | `tree_sitter_go` |
| `tree-sitter-html` | `tree_sitter_html` | `tree-sitter-java` | `tree_sitter_java` |
| `tree-sitter-javascript` | `tree_sitter_javascript` | `tree-sitter-json` | `tree_sitter_json` |
| `tree-sitter-markdown` | `tree_sitter_markdown` | `tree-sitter-php` | `tree_sitter_php` |
| `tree-sitter-python` | `tree_sitter_python` | `tree-sitter-ruby` | `tree_sitter_ruby` |
| `tree-sitter-swift` | `tree_sitter_swift` | `tree-sitter-toml` | `tree_sitter_toml` |
| `tree-sitter-typescript` | `tree_sitter_typescript` | `tree-sitter-yaml` | `tree_sitter_yaml` |
| `vendored-openssl` | `vendored_openssl` | `wgpu-target` | `wgpu_target` |

Upstream Cargo package names such as `tree-sitter-*`, `raw-window-handle`, and
`openssh-sftp-client` are unchanged. The human-facing CLI binaries also remain
`ailloli-ui-bench` and `cargo-ailloli-ui`.

## Targeted work and retained trees

Ailloli UI treats build, layout, and paint as distinct units of retained work.
The following invariants are part of the framework contract:

- invalidating one component never rebuilds or lays out a stable sibling whose
  inputs, constraints, and dependencies did not change;
- a virtualized component's per-frame cost depends on its viewport and
  overscan, not on the total number of items in its retained model;
- build, layout, paint, and hit testing never perform filesystem I/O. Sources
  are owned by workers and deliver owned deltas through bounded queues.

`Invalidation::Paint` reuses both the reconciled tree and layout,
`Invalidation::Layout` recomputes the affected ancestor path, and
`Invalidation::Build` reconciles the owning component before layout. Existing
`Context::signal` calls remain build-invalidating for compatibility. Use
`Context::signal_with_invalidation` when a signal has a narrower contract, and
use `Signal::set_if_changed` only for small values where `PartialEq` is cheap;
large trees should be represented by a retained handle and monotonic revision.

For large trees, migrate from repeatedly constructing `Vec<TreeNode<_>>` to a
UI-local `TreeModelHandle` and apply atomic deltas:

```rust
use ailloli_ui::prelude::*;

let model = TreeModelHandle::new(TreeModel::new());
model
    .apply_batch([
        TreeMutation::Insert {
            parent: None,
            index: 0,
            item: TreeItem::branch(1_u64, "src"),
        },
        TreeMutation::Insert {
            parent: Some(1),
            index: 0,
            item: TreeItem::leaf(2, "lib.rs"),
        },
        TreeMutation::SetExpanded {
            id: 1,
            expanded: true,
        },
    ])
    .expect("valid retained tree batch");

let tree = TreeView::new().model(model).virtualized(true);
# let _ = tree;
```

`TreeView::nodes` and `TreeView::bind_nodes` remain suitable for small
snapshots, but conversion happens only when their source changes. A
`TreeModelHandle` is UI-local and must not cross a worker thread. Filesystem
workers exchange requests and owned `FileTreeDelta` values, then wake the UI;
the UI applies those deltas to its model.

Filesystem watch payloads are non-exhaustive. Consumers that previously used
struct literals should use `WatchEvent::new(...)`, optional builders such as
`with_previous_uri(...)` and `with_identity(...)`, and accessors. Match
`WatchEventKind` with a wildcard arm so future provider-neutral events remain
source compatible.

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

### Performance regression gates

`ailloli_ui_bench` records versioned JSONL runs through a bounded writer queue.
Application runs opt in explicitly and must use a new destination:

```sh
AILLOLI_UI_BENCH=1 \
AILLOLI_UI_BENCH_PATH=artifacts/bench/manual/sandbox.jsonl \
cargo run -p sandbox_app
```

For reproducible native comparisons, build the measured child once in release
mode, then run it through the feature-gated CLI. Each scenario gets its own
directory, process, `RunEnd`, SHA-256 index, backend, dimensions, and observed
scale factor:

```sh
CARGO_INCREMENTAL=0 cargo build --release --locked \
  -p ailloli_ui_winit --features test_support \
  --example winit_regression_bench

CARGO_INCREMENTAL=0 cargo run --release --locked \
  -p ailloli_ui_bench --features cli --bin ailloli-ui-bench -- \
  run-matrix \
  --output-root artifacts/bench/phase125 \
  --phase candidate --winit-version 0.30.13 --backend wayland \
  --profile release --harness winit_regression_bench \
  --target x86_64-unknown-linux-gnu --machine local-wayland-01 \
  --scenario wake_single --mode steady \
  --warmups 3 --samples 30 --duration-ms 1200 \
  -- target/release/examples/winit_regression_bench

CARGO_INCREMENTAL=0 cargo run --release --locked \
  -p ailloli_ui_bench --features cli --bin ailloli-ui-bench -- \
  summarize --input \
  artifacts/bench/phase125/candidate/winit-0.30.13/wayland/wake_single
```

Run Wayland and X11 separately and compare only artifacts produced by the same
schema, harness, machine, GPU/driver, profile, geometry, and DPR. The CLI
rejects incomplete sessions and correctness counters above zero. The old
`OCTAVUI_BENCH_*`, `UI_BENCH*`, and `BENCH_*` names remain lower-priority
compatibility fallbacks; new integrations use `AILLOLI_UI_BENCH_*`, retain the
`BenchInit` guard, and call `finish()` so write, flush, sync, and publication
errors cannot be lost. The deprecated append-only `init_from_env` path is not
a regression gate.

## Assets and license

Framework-authored code is dual-licensed under Apache-2.0 or MIT, at your
option. See `LICENSE-APACHE`, `LICENSE-MIT`, and `NOTICE`.

Bundled fonts retain their own licenses and provenance beside the assets:

- `crates/ailloli_ui_text/assets/fonts/` contains JetBrains Mono and its OFL.
- `crates/ailloli_ui_devicons_font/` contains the reduced file-icon font,
  reproducible generation tooling, source hashes, and third-party notices.

Copyright 2026 Rising Corporation and Ailloli UI contributors.
