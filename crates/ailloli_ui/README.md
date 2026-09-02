# Ailloli UI

Ailloli UI is a retained-mode desktop UI framework for Rust with native
`winit` windows and GPU rendering through `wgpu`.

> Beta status: `0.1.0-beta.2` was frozen on 2026-09-02. APIs may evolve before
> 1.0. The minimum supported Rust version is Rust 1.88. Confirm registry
> availability before installing the release candidate from crates.io.

## Installation

After crates.io lists beta.2, use:

```toml
[dependencies]
ailloli_ui = "0.1.0-beta.2"
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

Beta.2 adds public scrollbar geometry and drag primitives, composed retained
dialogs through `Dialog::modal_content`, an Enter fallback through
`Dialog::on_submit`, configurable editor caret-follow margins, capturing
component render closures, and the `LayoutPass` contract for custom widgets.
It also makes reactive invalidation, layout publication, paint, and
multi-presentation wakeups consistent without removing or renaming a documented
beta.1 façade item. Documented beta.1 consumers require no source migration.

[0.1.0-beta.2 release notes](https://github.com/AilloliAI/ailloli_ui/blob/main/CHANGELOG.md) ·
[Security support](https://github.com/AilloliAI/ailloli_ui/blob/main/SECURITY.md)

[API documentation](https://ailloliai.github.io/ailloli_ui/ailloli_ui/) ·
[Repository](https://github.com/AilloliAI/ailloli_ui)

Ailloli UI is dual-licensed under Apache-2.0 or MIT, at your option.
