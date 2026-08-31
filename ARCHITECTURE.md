# Ailloli UI architecture

Ailloli UI is an application-neutral retained-mode framework. It separates the
public application API, retained runtime, reusable capabilities, native host,
and rendering backends so each layer can evolve behind explicit contracts.

Related operational guidance lives in the [migration guide](MIGRATION.md) and
the [benchmarking guide](BENCHMARKING.md).

## Dependency direction

The public dependency direction is intentionally one-way:

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

The `ailloli_ui` façade and its prelude are the primary application API.
Lower-level crates remain available for custom runtimes, render hosts, platform
integrations, and reusable framework extensions.

Framework crates never depend on a consumer application or application-owned
business logic. Provider selection, credentials, product policy, and business
workflows belong in consuming applications. The public `sandbox_app` validates
this boundary by depending directly only on the `ailloli_ui` façade.

## Workspace packages

The workspace contains 22 publishable framework packages plus two
non-publishable tools: the `sandbox_app` consumer application and `xtask`
release helper:

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
| `sandbox_app` | Public consumer sandbox used to exercise the façade. |
| `xtask` | Repository, package-archive, and release-contract validation. |

## Public façade features

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

Feature-name compatibility changes are documented in
[MIGRATION.md](MIGRATION.md).

## Targeted work and retained trees

Ailloli UI treats build, layout, and paint as distinct units of retained work.
The following invariants are part of the framework contract:

- invalidating one component never rebuilds or lays out a stable sibling whose
  inputs, constraints, and dependencies did not change;
- a virtualized component's per-frame cost depends on its viewport and
  overscan, not on the total number of items in its retained model;
- build, layout, paint, and hit testing never perform filesystem I/O. Sources
  are owned by workers and deliver owned deltas through bounded queues.

Reactive state is UI-thread-local. Reading a `Signal`, `State`, reactive
`Binding`, or derived `Memo` during a retained build, layout, or paint callback
automatically associates that mounted consumer with the sources actually read.
An ordinary read outside those callbacks is passive: it returns the value but
does not schedule future work. Owner-provided invalidators installed by runtime
contexts remain active for compatibility and are independent of automatic
observation.

Dependencies are replaced only after a callback succeeds. Consequently, a
conditional callback that changes from reading `A` to reading `B` stops reacting
to `A` and starts reacting to `B` as one update; a panicking callback keeps its
previous dependencies. Mutations commit the value and revision before
notification, release state and subscriber borrows, notify retained consumers,
then invoke the owner-provided invalidator. Notification is synchronous and may
be reentrant. Mounted consumers are held weakly and are disconnected on
unmount, while independently owned state remains usable.

`Invalidation::Paint` reuses both the reconciled tree and layout.
`Invalidation::Layout` recomputes the affected ancestor path, while
`Invalidation::Build` reconciles the owning component before layout. Existing
`Context::signal` calls remain build-invalidating for compatibility. Use
`Context::signal_with_invalidation` when a signal has a narrower contract, and
use `Signal::set_if_changed` only for small values where `PartialEq` is cheap.
Large trees should be represented by a retained handle and monotonic revision.

Layout dependency observation is transactional. Speculative measurements stage
their reads without changing the active dependency set. Reads from measurements
that contribute to the accepted result are combined with reads from the
authoritative allocation and published only when the complete layout attempt
succeeds. Abandoned alternatives, superseded attempts, and panics preserve the
last committed dependency set and geometry.

Paint consumes committed layout results and their reusable artifacts. A widget
must not reshape newly read geometry-dependent content into bounds produced for
older content. If a layout dependency changes before paint, the runtime requests
layout for a later traversal and either replays a coherent committed artifact or
skips unsafe paint. It never starts recursive layout from paint.

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

## Native host and renderers

`ailloli_ui_winit` owns the native event loop, window lifecycle, clipboard,
capture integration, and renderer hosting. `ailloli_ui_render_wgpu` is the
primary renderer. Vulkan and OpenXR integrations remain experimental or
optional and preserve the same application-neutral runtime boundary.

Performance comparisons across these hosts and renderers must follow the
reproducibility contract in [BENCHMARKING.md](BENCHMARKING.md).
