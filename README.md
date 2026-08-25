# Ailloli UI

Ailloli UI is a retained-mode desktop UI framework for Rust, built for native
applications that need predictable state, targeted updates, and GPU-accelerated
rendering.

It combines a backend-neutral retained runtime with native `winit` windows, a
primary `wgpu` renderer, and reusable components for building complete desktop
applications.

> **Status:** Ailloli UI is pre-1.0 and under active development.\
> **MSRV:** Rust 1.88\
> Packages are not published on crates.io yet and currently use path or pinned
> Git dependencies.

[API Documentation](https://ailloliai.github.io/ailloli_ui/) ·
[Contributing](CONTRIBUTING.md) · [Support](SUPPORT.md) ·
[Security](SECURITY.md)

## Features

- Retained-mode UI with targeted build, layout, and paint invalidation
- Native desktop windows powered by `winit`
- GPU rendering through `wgpu`
- Backend-neutral runtime and rendering architecture
- Layouts, controls, text editing, file and terminal widgets
- Code-editor primitives with optional Tree-sitter integration
- Provider-neutral filesystem architecture
- Runtime inspection through optional DevTools
- Experimental Vulkan and OpenXR integration

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

`ailloli_ui::prelude::*` is the primary application API.

Lower-level crates remain available for custom runtimes, render hosts, platform
integrations, and reusable framework extensions.

## Architecture

Ailloli UI separates the public application API, retained runtime, platform
integration, and rendering layers.

```text
Application
    │
    ▼
ailloli_ui
    │
    ├── Core
    ├── Runtime
    ├── Widgets
    ├── Text / Editor
    ├── Files / Terminal
    └── DevTools
    │
    ▼
Platform host (`winit`)
    │
    ▼
Renderer (`wgpu`)
    │
    ├── Vulkan (experimental)
    └── OpenXR (optional)
```

The dependency direction is intentionally one-way: framework crates never
depend on consumer applications or application-owned business logic.

Ailloli UI uses targeted retained work so changes can invalidate build, layout,
or paint independently without forcing unrelated stable components to be
rebuilt.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the complete crate architecture,
runtime contracts, and design principles.

## Sandbox

The repository includes a real consumer application built exclusively through
the public Ailloli UI façade.

Run it from the workspace root:

```sh
cargo run -p sandbox_app
```

The sandbox acts as both an interactive framework showcase and a validation of
the same API surface available to external applications.

## Documentation

- [API Documentation](https://ailloliai.github.io/ailloli_ui/)
- [Architecture](ARCHITECTURE.md)
- **Ailloli UI — The Book** — coming soon
- **Ailloli UI by Example** — coming soon
- [Contributing](CONTRIBUTING.md)
- [Support](SUPPORT.md)
- [Security](SECURITY.md)

## Sponsorship

Ailloli UI is free and open source and can be used for personal, open-source,
and commercial projects without sponsoring the project.

Voluntary [GitHub Sponsorship](https://github.com/sponsors/AilloliAI) helps fund
maintenance, bug fixes, performance work, documentation, new capabilities, and
long-term stability.

Sponsorship does not provide private functionality, support guarantees, bug
priority, or roadmap control.

See [SPONSORS.md](SPONSORS.md) for the complete sponsorship policy.

## License

Ailloli UI is dual-licensed under Apache-2.0 or MIT, at your option.

See `LICENSE-APACHE`, `LICENSE-MIT`, and `NOTICE`.

Bundled third-party assets retain their respective licenses and provenance.

Copyright 2026 Rising Corporation and Ailloli UI contributors.
