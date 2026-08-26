# Ailloli UI

Ailloli UI is a retained-mode desktop UI framework for Rust with native
`winit` windows and GPU rendering through `wgpu`.

> Beta status: APIs may evolve before 1.0. The minimum supported Rust version
> is Rust 1.88.

## Installation

```toml
[dependencies]
ailloli_ui = "0.1.0-beta.1"
```

## Quick start

```rust
use ailloli_ui::prelude::*;

fn main() -> ailloli_ui::Result<()> {
    App::new()
        .window(
            Window::new("main")
                .title("Hello")
                .size(800.0, 600.0)
                .content(|| Text::new("Hello from Ailloli UI")),
        )
        .run()
}
```

The façade exposes the primary application API, retained widgets, text and
editor facilities, filesystem contracts, terminal types, and native host
integration. Lower-level crates remain available for custom integrations.

[API documentation](https://ailloliai.github.io/ailloli_ui/ailloli_ui/) ·
[Repository](https://github.com/AilloliAI/ailloli_ui)

Ailloli UI is dual-licensed under Apache-2.0 or MIT, at your option.
